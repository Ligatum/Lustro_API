"""
lustro.dll speed test (ctypes), normalized per engine round. lustro.dll must be in the same directory.
Stream batch calls use the `steps` suggested by the library for the given N.
Multi-stream sections run one single stream per process and report the aggregate.
Optional arguments: section number (0-10) and variant (1-5, or auto, scalar, st, compare, simd).
"""

import array
import contextlib
import ctypes
import multiprocessing
import os
import queue
import random
import sys
import time

# =========================================================
# CONFIG
# =========================================================
MIN_TIME = 1.0
MIN_ITERS = 100

# Batch size column: input bytes for HASH rows (N * msg_len), output bytes for stream rows
# (N * steps * 32). The largest sizes are meant to go past L3.
# HASH_BATCH_SIZES is N for msg_len 32: 64 KiB ... 32 MiB of input.
HASH_BATCH_SIZES = [2048, 4096, 8192, 16384, 32768, 65536, 131072, 196608, 1 << 20]
STREAM_BATCH_SIZES = [8, 16, 32, 64, 128, 256, 1024, 4096, 16384, 65536, 131072, 196608, 1 << 20]

# Exact multiple of 32 B: no terminator round (absorb_with_domain, api.rs).
HASH_MSG_LEN_BASELINE = 32

# Lengths on both sides of a 32 B boundary (31/32/33, 64/65) to separate
# terminator cost from round count.
HASH_MSG_LENGTHS = [16, 31, 32, 33, 64, 65, 128, 256, 512, 768, 1024, 2048, 4096]

# Length sweep: constant total input, N = HASH_SWEEP_BYTES // msg_len, so only the
# message length changes between rows.
HASH_SWEEP_BYTES = 4 << 20

# Var sections: lengths uniform in 0..2*mean, fixed seed, the same set on every run.
# Rounds and Data rate follow the actual lengths.
HASH_VAR_SEED = 12345

# Mean message length for the var sweep (constant total input, as for the fixed sweep).
HASH_VAR_MEANS = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096]

# Single-stream calls: buffer sizes for lustro_prng_fill / lustro_xof_fill. One ctypes
# call is timed, so small sizes are dominated by call overhead.
SINGLE_FILL_SIZES = [4096, 65536, 1 << 20, 16 << 20]

# Multi-stream: one stream and one buffer per process. 64 KiB is cache-resident, 16 MiB is not.
# Memory use ~ procs * size.
MULTI_FILL_SIZES = [64 << 10, 16 << 20]
MULTI_TIME = 2.0
MULTI_WARMUP = 0.2
MULTI_BARRIER_TIMEOUT = 120.0


def hash_rounds_for_len(msg_len: int) -> int:
    """
    Rounds for one message (absorb_with_domain / finalize_terminator, api.rs):
    msg_len // 32, plus one terminator round unless msg_len is a non-zero
    multiple of 32.
    """
    full_blocks = msg_len // 32
    remainder = msg_len % 32
    if remainder == 0 and msg_len > 0:
        return full_blocks
    return full_blocks + 1


HW_THREADS = os.cpu_count() or 1

DLL_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "lustro.dll")

ENV_VARS = ("LUSTRO_DISABLE_MT", "LUSTRO_DISABLE_SIMD", "LUSTRO_NUM_THREADS")

# Variants: environment of the worker process. "compare" times auto and st side by side,
# "simd" auto and scalar.
ST_ENV = {"LUSTRO_DISABLE_MT": "1"}
SCALAR_ENV = {"LUSTRO_DISABLE_SIMD": "1"}
MODE_ENV = {
    "auto": {},
    "scalar": SCALAR_ENV,
    "st": ST_ENV,
    "compare": {},
    "simd": {},
}
# Side-by-side variants: (column label, environment of the reference run).
COMPARE = {
    "compare": ("vs ST", ST_ENV),
    "simd": ("vs scalar", SCALAR_ENV),
}
MODES = tuple(MODE_ENV)
VARIANT_HELP = {
    "auto": "library defaults",
    "scalar": "AVX2 off (stream batches and hash batches)",
    "st": "Rayon off, calling thread only",
    "compare": "auto and st side by side, 'vs ST' column",
    "simd": "auto and scalar side by side, 'vs scalar' column",
}
PLAIN_MODES = ("auto",)
HASH_MODES = MODES
STREAM_MODES = MODES


def proc_counts():
    """1, 2, 4, ... below HW_THREADS, then HW_THREADS itself."""
    counts = []
    t = 1
    while t < HW_THREADS:
        counts.append(t)
        t *= 2
    counts.append(HW_THREADS)
    return counts


# =========================================================
# UTILS
# =========================================================
def format_size(size_bytes):
    if size_bytes < 1024:
        return f"{size_bytes} B"
    if size_bytes < 1024 ** 2:
        return f"{(size_bytes + 512) // 1024} KiB"
    return f"{(size_bytes + 512 * 1024) // (1024 ** 2)} MiB"


def percentile(sorted_vals, q):
    k = (len(sorted_vals) - 1) * q / 100.0
    lo = int(k)
    hi = min(lo + 1, len(sorted_vals) - 1)
    return sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (k - lo)


def timed_loop(fn):
    for _ in range(20):
        fn()
    times = []
    total_start = time.perf_counter()
    while True:
        start = time.perf_counter_ns()
        fn()
        end = time.perf_counter_ns()
        times.append((end - start) * 1e-9)
        if (time.perf_counter() - total_start) > MIN_TIME and len(times) >= MIN_ITERS:
            break
    times.sort()
    return times


def summarize(times, n_elements, rounds_per_elem, total_bytes):
    """
    Everything is per engine round (n_elements * rounds_per_elem), so the
    numbers compare across pipelines.
    """
    p50 = percentile(times, 50)
    p95 = percentile(times, 95)
    rounds_total = n_elements * rounds_per_elem

    return {
        "gib_s": total_bytes / p50 / (1024 ** 3),
        "ns_per_round": (p50 * 1e9) / rounds_total,
        "p50_ms": p50 * 1000,
        "p95_ms": p95 * 1000,
        "rounds_total": rounds_total,
    }


def summarize_multi(results, size):
    """
    results: one {"calls", "elapsed"} per process. Aggregate rate is the sum of the
    per-process rates, each over its own timed window.
    """
    rounds_per_call = size // 32
    rates = [r["calls"] * rounds_per_call / r["elapsed"] for r in results]
    agg = sum(rates)
    return {
        "procs": len(results),
        "agg_rps": agg,
        "rounds_total": sum(r["calls"] for r in results) * rounds_per_call,
        "gib_s": agg * 32 / (1024 ** 3),
        "ns_agg": 1e9 / agg,
        "ns_proc": sum(1e9 / r for r in rates) / len(rates),
        "min_max": min(rates) / max(rates),
    }


COL = {
    "pipeline": 13,
    "size": 10,
    "n": 8,
    "steps": 5,
    "rounds": 10,
    "p50": 10,
    "p95": 10,
    "tp": 12,
    "ns": 10,
    "vs": 10,
}
WIDTH = sum(COL.values()) - COL["vs"] + 3 * (len(COL) - 2)
WIDTH_CMP = WIDTH + 3 + COL["vs"]

COL_MULTI = {
    "pipeline": 10,
    "size": 10,
    "procs": 6,
    "rounds": 12,
    "tp": 12,
    "ns_agg": 12,
    "ns_proc": 12,
    "scale": 8,
    "minmax": 8,
}
WIDTH_MULTI = sum(COL_MULTI.values()) + 3 * (len(COL_MULTI) - 1)


def make_header(mode="auto"):
    head = (
        f"{'Pipeline':<{COL['pipeline']}} | {'Batch size':<{COL['size']}} | "
        f"{'N':<{COL['n']}} | {'Steps':>{COL['steps']}} | {'Rounds':>{COL['rounds']}} | "
        f"{'p50 (ms)':>{COL['p50']}} | {'p95 (ms)':>{COL['p95']}} | "
        f"{'Data rate':>{COL['tp']}} | {'ns/round':>{COL['ns']}}"
    )
    if mode in COMPARE:
        head += f" | {COMPARE[mode][0]:>{COL['vs']}}"
    return head


def table_width(mode):
    return WIDTH_CMP if mode in COMPARE else WIDTH


def table_start(title, mode):
    width = table_width(mode)
    print(f"\n{title:^{width}}")
    print(make_header(mode))
    print("-" * width)


def make_multi_header():
    return (
        f"{'Pipeline':<{COL_MULTI['pipeline']}} | {'Size/call':<{COL_MULTI['size']}} | "
        f"{'Procs':>{COL_MULTI['procs']}} | {'Rounds':>{COL_MULTI['rounds']}} | "
        f"{'Data rate':>{COL_MULTI['tp']}} | {'ns/round agg':>{COL_MULTI['ns_agg']}} | "
        f"{'ns/round proc':>{COL_MULTI['ns_proc']}} | {'Scaling':>{COL_MULTI['scale']}} | "
        f"{'Min/Max':>{COL_MULTI['minmax']}}"
    )


def print_row(label, n, steps, size_bytes, data, vs_ref=None):
    tp_str = f"{data['gib_s']:.2f} GiB/s"
    steps_str = "-" if steps is None else str(steps)
    line = (
        f"{label:<{COL['pipeline']}} | {format_size(size_bytes):<{COL['size']}} | "
        f"{n:<{COL['n']}} | {steps_str:>{COL['steps']}} | {data['rounds_total']:>{COL['rounds']}} | "
        f"{data['p50_ms']:>{COL['p50']}.4f} | {data['p95_ms']:>{COL['p95']}.4f} | "
        f"{tp_str:>{COL['tp']}} | {data['ns_per_round']:>{COL['ns']}.2f}"
    )
    if vs_ref is not None:
        line += f" | {f'{vs_ref:.2f}x':>{COL['vs']}}"
    print(line)


def print_multi_row(label, size_bytes, data, scaling):
    tp_str = f"{data['gib_s']:.2f} GiB/s"
    scale_str = f"{scaling * 100:.0f}%"
    minmax_str = f"{data['min_max'] * 100:.0f}%"
    print(
        f"{label:<{COL_MULTI['pipeline']}} | {format_size(size_bytes):<{COL_MULTI['size']}} | "
        f"{data['procs']:>{COL_MULTI['procs']}} | {data['rounds_total']:>{COL_MULTI['rounds']}} | "
        f"{tp_str:>{COL_MULTI['tp']}} | {data['ns_agg']:>{COL_MULTI['ns_agg']}.2f} | "
        f"{data['ns_proc']:>{COL_MULTI['ns_proc']}.2f} | {scale_str:>{COL_MULTI['scale']}} | "
        f"{minmax_str:>{COL_MULTI['minmax']}}"
    )


# =========================================================
# DLL BINDING: 1:1 with the public C API
# =========================================================
U8P = ctypes.POINTER(ctypes.c_uint8)
SizeT = ctypes.c_size_t
OUTP = ctypes.POINTER(ctypes.c_void_p)


def new_ctx(fn, *args):
    """Constructor LustroError f(..., T **out_ctx): returns the context or raises."""
    slot = ctypes.c_void_p()
    err = fn(*args, ctypes.byref(slot))
    if err != 0 or not slot.value:
        raise RuntimeError(f"{fn.__name__} returned error {err}")
    return slot.value


def load_lib():
    if not os.path.exists(DLL_PATH):
        raise FileNotFoundError(f"not found: {DLL_PATH}")
    lib = ctypes.CDLL(DLL_PATH)

    lib.lustro_api_version.restype = ctypes.c_uint32
    lib.lustro_api_version.argtypes = []

    lib.lustro_runtime_backend.restype = ctypes.c_char_p
    lib.lustro_runtime_backend.argtypes = []
    lib.lustro_runtime_threads.restype = SizeT
    lib.lustro_runtime_threads.argtypes = []

    lib.lustro_hash256_many_fixed.restype = ctypes.c_int32
    lib.lustro_hash256_many_fixed.argtypes = [U8P, SizeT, SizeT, U8P]

    lib.lustro_hash256_many_var.restype = ctypes.c_int32
    lib.lustro_hash256_many_var.argtypes = [ctypes.POINTER(U8P), SizeT, ctypes.POINTER(SizeT), U8P]

    # Single stream: new(..., &ctx) / fill(ctx, out, out_len) / free.
    lib.lustro_prng_new.restype = ctypes.c_int32
    lib.lustro_prng_new.argtypes = [U8P, ctypes.c_uint64, ctypes.c_uint64, OUTP]
    lib.lustro_prng_free.restype = None
    lib.lustro_prng_free.argtypes = [ctypes.c_void_p]
    lib.lustro_prng_fill.restype = ctypes.c_int32
    lib.lustro_prng_fill.argtypes = [ctypes.c_void_p, U8P, SizeT]

    lib.lustro_xof_new.restype = ctypes.c_int32
    lib.lustro_xof_new.argtypes = [U8P, SizeT, OUTP]
    lib.lustro_xof_free.restype = None
    lib.lustro_xof_free.argtypes = [ctypes.c_void_p]
    lib.lustro_xof_fill.restype = ctypes.c_int32
    lib.lustro_xof_fill.argtypes = [ctypes.c_void_p, U8P, SizeT]

    # fill_blocks(ctx, out, out_len, steps), output is step-major.
    # suggested_steps(ctx) is the `steps` the library recommends for len(ctx).
    lib.lustro_prng_batch_new_range.restype = ctypes.c_int32
    lib.lustro_prng_batch_new_range.argtypes = [U8P, ctypes.c_uint64, ctypes.c_uint64, SizeT, OUTP]
    lib.lustro_prng_batch_free.restype = None
    lib.lustro_prng_batch_free.argtypes = [ctypes.c_void_p]
    lib.lustro_prng_batch_fill_blocks.restype = ctypes.c_int32
    lib.lustro_prng_batch_fill_blocks.argtypes = [ctypes.c_void_p, U8P, SizeT, SizeT]
    lib.lustro_prng_batch_suggested_steps.restype = SizeT
    lib.lustro_prng_batch_suggested_steps.argtypes = [ctypes.c_void_p]

    lib.lustro_xof_batch_new.restype = ctypes.c_int32
    lib.lustro_xof_batch_new.argtypes = [ctypes.POINTER(U8P), ctypes.POINTER(SizeT), SizeT, OUTP]
    lib.lustro_xof_batch_free.restype = None
    lib.lustro_xof_batch_free.argtypes = [ctypes.c_void_p]
    lib.lustro_xof_batch_fill_blocks.restype = ctypes.c_int32
    lib.lustro_xof_batch_fill_blocks.argtypes = [ctypes.c_void_p, U8P, SizeT, SizeT]
    lib.lustro_xof_batch_suggested_steps.restype = SizeT
    lib.lustro_xof_batch_suggested_steps.argtypes = [ctypes.c_void_p]

    return lib


def u8_buf(data: bytes):
    n = len(data)
    return (ctypes.c_uint8 * n).from_buffer_copy(data) if n else (ctypes.c_uint8 * 0)()


def open_single(lib, kind):
    """Single PRNG or XOF stream: (ctx, fill, free, fill_name)."""
    if kind == "PRNG":
        seed = u8_buf(bytes(range(32)))
        ctx = new_ctx(lib.lustro_prng_new, seed, 0, 0)
        return ctx, lib.lustro_prng_fill, lib.lustro_prng_free, "lustro_prng_fill"
    msg = u8_buf(b"m0")
    ctx = new_ctx(lib.lustro_xof_new, msg, 2)
    return ctx, lib.lustro_xof_fill, lib.lustro_xof_free, "lustro_xof_fill"


# =========================================================
# WORKERS
# =========================================================
def info_worker(q):
    try:
        lib = load_lib()
        q.put({
            "backend": lib.lustro_runtime_backend().decode(),
            "threads": lib.lustro_runtime_threads(),
        })
    except Exception as e:
        q.put({"error": repr(e)})


def hash_batch_worker(n, msg_len, q):
    try:
        lib = load_lib()

        data = u8_buf(os.urandom(n * msg_len))
        data_ptr = ctypes.cast(data, U8P)

        out = (ctypes.c_uint8 * (n * 32))()
        out_ptr = ctypes.cast(out, U8P)

        def call():
            err = lib.lustro_hash256_many_fixed(data_ptr, n, msg_len, out_ptr)
            if err != 0:
                raise RuntimeError(f"lustro_hash256_many_fixed returned error {err}")

        times = timed_loop(call)
        total_bytes = n * msg_len
        result = summarize(times, n, hash_rounds_for_len(msg_len), total_bytes)
        q.put({"result": result, "steps": None, "size_bytes": total_bytes})
    except Exception as e:
        q.put({"error": repr(e)})


def hash_var_worker(n, mean_len, q):
    """
    lustro_hash256_many_var on n messages with lengths uniform in 0..2*mean_len (fixed
    seed). Rounds and data rate are counted from the actual lengths.
    """
    try:
        lib = load_lib()

        rng = random.Random(HASH_VAR_SEED)
        lens = [rng.randint(0, 2 * mean_len) for _ in range(n)]
        total_bytes = sum(lens)

        data = u8_buf(os.urandom(total_bytes))
        pos = ctypes.addressof(data)
        offsets = []
        for length in lens:
            offsets.append(pos)
            pos += length
        ptr_array = (SizeT * n)(*offsets)
        len_array = (SizeT * n)(*lens)
        ptrs = ctypes.cast(ptr_array, ctypes.POINTER(U8P))

        out = (ctypes.c_uint8 * (n * 32))()
        out_ptr = ctypes.cast(out, U8P)

        def call():
            err = lib.lustro_hash256_many_var(ptrs, n, len_array, out_ptr)
            if err != 0:
                raise RuntimeError(f"lustro_hash256_many_var returned error {err}")

        times = timed_loop(call)
        rounds = sum(hash_rounds_for_len(length) for length in lens)
        result = summarize(times, 1, rounds, total_bytes)
        q.put({"result": result, "steps": None, "size_bytes": total_bytes})
    except Exception as e:
        q.put({"error": repr(e)})


def fill_single_worker(kind, size, q):
    """
    One lustro_prng_fill / lustro_xof_fill call on a single stream.
    size is a multiple of 32, so rounds = size // 32 (one round per 32 B block).
    """
    try:
        lib = load_lib()
        ctx, fill, free, fill_name = open_single(lib, kind)

        try:
            out = (ctypes.c_uint8 * size)()
            out_ptr = ctypes.cast(out, U8P)

            def call():
                err = fill(ctx, out_ptr, size)
                if err != 0:
                    raise RuntimeError(f"{fill_name} returned error {err}")

            times = timed_loop(call)
            result = summarize(times, 1, size // 32, size)
            q.put({"result": result, "steps": None, "size_bytes": size})
        finally:
            free(ctx)
    except Exception as e:
        q.put({"error": repr(e)})


def multi_fill_worker(kind, size, barrier, q):
    """
    One process of the multi-stream test: own stream, own buffer. All processes
    start the timed window together (barrier) and fill for MULTI_TIME.
    """
    try:
        lib = load_lib()
        ctx, fill, free, fill_name = open_single(lib, kind)

        try:
            out = (ctypes.c_uint8 * size)()
            out_ptr = ctypes.cast(out, U8P)

            def call():
                err = fill(ctx, out_ptr, size)
                if err != 0:
                    raise RuntimeError(f"{fill_name} returned error {err}")

            warm_end = time.perf_counter() + MULTI_WARMUP
            call()
            while time.perf_counter() < warm_end:
                call()

            barrier.wait(timeout=MULTI_BARRIER_TIMEOUT)

            calls = 0
            start = time.perf_counter()
            deadline = start + MULTI_TIME
            while True:
                call()
                calls += 1
                now = time.perf_counter()
                if now >= deadline:
                    break
            q.put({"calls": calls, "elapsed": now - start})
        finally:
            free(ctx)
    except Exception as e:
        try:
            barrier.abort()
        except Exception:
            pass
        q.put({"error": repr(e)})


def _make_xof_batch(lib, n):
    # One flat buffer of n fixed 8 B messages; pointers are offsets into it.
    blob = u8_buf(array.array("Q", range(n)).tobytes())
    base = ctypes.addressof(blob)
    ptr_array = (SizeT * n)(*range(base, base + 8 * n, 8))
    len_array = (SizeT * n)(*([8] * n))
    return new_ctx(
        lib.lustro_xof_batch_new, ctypes.cast(ptr_array, ctypes.POINTER(U8P)), len_array, n
    )


def stream_batch_worker(kind, n, q):
    """
    One fill_blocks() call: each of the n streams advances `steps` rounds,
    n * steps blocks out (step-major). `steps` comes from the library.
    kind is "PRNG" or "XOF".
    """
    try:
        lib = load_lib()

        if kind == "PRNG":
            seed = u8_buf(bytes(range(32)))
            batch_ctx = new_ctx(lib.lustro_prng_batch_new_range, seed, 0, 0, n)
            fill = lib.lustro_prng_batch_fill_blocks
            free = lib.lustro_prng_batch_free
            suggested = lib.lustro_prng_batch_suggested_steps
            fill_name = "lustro_prng_batch_fill_blocks"
        else:
            batch_ctx = _make_xof_batch(lib, n)
            fill = lib.lustro_xof_batch_fill_blocks
            free = lib.lustro_xof_batch_free
            suggested = lib.lustro_xof_batch_suggested_steps
            fill_name = "lustro_xof_batch_fill_blocks"

        try:
            steps = suggested(batch_ctx)
            if steps == 0:
                raise RuntimeError("suggested_steps returned 0")

            out_len = n * steps * 32
            out = (ctypes.c_uint8 * out_len)()
            out_ptr = ctypes.cast(out, U8P)

            def call():
                err = fill(batch_ctx, out_ptr, out_len, steps)
                if err != 0:
                    raise RuntimeError(f"{fill_name} returned error {err}")

            times = timed_loop(call)
            result = summarize(times, n, steps, out_len)
            q.put({"result": result, "steps": steps, "size_bytes": out_len})
        finally:
            free(batch_ctx)
    except Exception as e:
        q.put({"error": repr(e)})


@contextlib.contextmanager
def child_env(extra):
    """Sets `extra` in this process while a child is started; the child inherits it."""
    saved = {k: os.environ.get(k) for k in extra}
    os.environ.update(extra)
    try:
        yield
    finally:
        for k, v in saved.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v


def run_worker(func, args, env=None):
    q = multiprocessing.Queue()
    p = multiprocessing.Process(target=func, args=(*args, q))
    with child_env(env or {}):
        p.start()
    res = None
    while res is None:
        try:
            res = q.get(timeout=1.0)
        except queue.Empty:
            if not p.is_alive():
                # Result may have landed just before exit.
                try:
                    res = q.get(timeout=1.0)
                except queue.Empty:
                    p.join()
                    raise RuntimeError(f"worker {func.__name__} exited with code {p.exitcode}")
    p.join()
    if "error" in res:
        raise RuntimeError(f"worker {func.__name__} returned error: {res['error']}")
    return res


def run_multi(kind, size, procs):
    """Runs `procs` multi_fill_worker processes together, returns their results."""
    barrier = multiprocessing.Barrier(procs)
    q = multiprocessing.Queue()
    workers = [
        multiprocessing.Process(target=multi_fill_worker, args=(kind, size, barrier, q))
        for _ in range(procs)
    ]
    for p in workers:
        p.start()

    results = []
    while len(results) < procs:
        try:
            results.append(q.get(timeout=1.0))
        except queue.Empty:
            if not any(p.is_alive() for p in workers):
                try:
                    results.append(q.get(timeout=1.0))
                except queue.Empty:
                    break
    for p in workers:
        p.join()

    if len(results) < procs:
        raise RuntimeError(f"multi worker: {len(results)} of {procs} processes reported")
    # A broken barrier is a consequence; report the original error.
    errors = [r["error"] for r in results if "error" in r]
    if errors:
        primary = [e for e in errors if "BrokenBarrierError" not in e] or errors
        raise RuntimeError(f"multi worker returned error: {primary[0]}")
    return results


# =========================================================
# PIPELINE SECTIONS: one per menu entry, each prints its own table.
# =========================================================
def emit(label, n, func, args, mode):
    res = run_worker(func, args, MODE_ENV[mode])
    vs_ref = None
    if mode in COMPARE:
        ref = run_worker(func, args, COMPARE[mode][1])
        vs_ref = ref["result"]["p50_ms"] / res["result"]["p50_ms"]
    print_row(label, n, res["steps"], res["size_bytes"], res["result"], vs_ref)


def section_hash_baseline(mode):
    table_start(
        f"HASH256_MANY_FIXED  ({hash_rounds_for_len(HASH_MSG_LEN_BASELINE)} round / message, "
        f"msg_len={HASH_MSG_LEN_BASELINE})",
        mode,
    )
    for n in HASH_BATCH_SIZES:
        emit("HASH", n, hash_batch_worker, (n, HASH_MSG_LEN_BASELINE), mode)


def section_hash_sweep(mode):
    table_start(
        f"HASH256_MANY_FIXED -- MSG LENGTH SWEEP (input ~{format_size(HASH_SWEEP_BYTES)}; label = msg_len)",
        mode,
    )
    for msg_len in HASH_MSG_LENGTHS:
        n = HASH_SWEEP_BYTES // msg_len
        emit(f"HASH_L{msg_len}", n, hash_batch_worker, (n, msg_len), mode)


def section_hash_var_baseline(mode):
    table_start(
        "HASH256_MANY_VAR  (lengths uniform in 0..64, mean 32; same N as the fixed baseline)",
        mode,
    )
    for n in HASH_BATCH_SIZES:
        emit("HASH_VAR", n, hash_var_worker, (n, HASH_MSG_LEN_BASELINE), mode)


def section_hash_var_sweep(mode):
    table_start(
        f"HASH256_MANY_VAR -- MEAN LENGTH SWEEP (input ~{format_size(HASH_SWEEP_BYTES)}; "
        "label = mean msg_len, lengths 0..2*mean)",
        mode,
    )
    for mean_len in HASH_VAR_MEANS:
        n = HASH_SWEEP_BYTES // mean_len
        emit(f"VAR_L{mean_len}", n, hash_var_worker, (n, mean_len), mode)


def make_fill_single_section(kind):
    def section(mode):
        table_start(f"{kind}_FILL  (single stream, N=1; includes ctypes call overhead)", mode)
        for size in SINGLE_FILL_SIZES:
            emit(f"{kind}_FILL", 1, fill_single_worker, (kind, size), mode)
    return section


def make_stream_section(kind):
    def section(mode):
        table_start(f"{kind}_BATCH_FILL_BLOCKS  (steps chosen by the library)", mode)
        for n in STREAM_BATCH_SIZES:
            emit(kind, n, stream_batch_worker, (kind, n), mode)
    return section


def make_multi_section(kind):
    def section(mode):
        title = (
            f"{kind}_FILL x N PROCESSES  (one single stream per process, aggregate; "
            f"{MULTI_TIME:g} s window)"
        )
        print(f"\n{title:^{WIDTH_MULTI}}")
        print(make_multi_header())
        print("-" * WIDTH_MULTI)
        for size in MULTI_FILL_SIZES:
            base = None
            for procs in proc_counts():
                data = summarize_multi(run_multi(kind, size, procs), size)
                if base is None:
                    base = data["agg_rps"]
                scaling = data["agg_rps"] / (procs * base)
                print_multi_row(f"{kind}_MULTI", size, data, scaling)
            print("-" * WIDTH_MULTI)
    return section


SECTIONS = {
    1: ("HASH256_MANY_FIXED (baseline, msg_len=32)", section_hash_baseline, HASH_MODES),
    2: ("HASH256_MANY_FIXED message-length sweep", section_hash_sweep, HASH_MODES),
    3: ("HASH256_MANY_VAR (baseline, mean msg_len=32)", section_hash_var_baseline, HASH_MODES),
    4: ("HASH256_MANY_VAR mean-length sweep", section_hash_var_sweep, HASH_MODES),
    5: ("PRNG_BATCH_FILL_BLOCKS", make_stream_section("PRNG"), STREAM_MODES),
    6: ("XOF_BATCH_FILL_BLOCKS", make_stream_section("XOF"), STREAM_MODES),
    7: ("PRNG_FILL (single stream)", make_fill_single_section("PRNG"), PLAIN_MODES),
    8: ("XOF_FILL (single stream)", make_fill_single_section("XOF"), PLAIN_MODES),
    9: ("PRNG_FILL (many single streams, one per process)", make_multi_section("PRNG"), PLAIN_MODES),
    10: ("XOF_FILL (many single streams, one per process)", make_multi_section("XOF"), PLAIN_MODES),
}
LAST_SECTION = max(SECTIONS)


# =========================================================
# SELECTION MENU
# =========================================================
def parse_variant(raw):
    raw = raw.strip().lower()
    if not raw:
        return "auto"
    if raw.isdigit():
        idx = int(raw)
        if idx not in range(1, len(MODES) + 1):
            print(f"ERROR: invalid variant {idx}, expected a number 1-{len(MODES)}")
            sys.exit(1)
        return MODES[idx - 1]
    if raw not in MODES:
        print(f"ERROR: invalid variant '{raw}', expected 1-{len(MODES)} or one of: {', '.join(MODES)}")
        sys.exit(1)
    return raw


def prompt_selection():
    print("Select which test to run:")
    print("  0 - RUN ALL (default)")
    for i in range(1, LAST_SECTION + 1):
        print(f"  {i} - {SECTIONS[i][0]}")

    interactive = len(sys.argv) <= 1
    if interactive:
        raw = input(f"Choice [0-{LAST_SECTION}, Enter = 0]: ").strip() or "0"
    else:
        raw = sys.argv[1]

    try:
        choice = int(raw)
    except ValueError:
        print(f"ERROR: invalid choice '{raw}', expected a number 0-{LAST_SECTION}")
        sys.exit(1)

    if choice not in range(0, LAST_SECTION + 1):
        print(f"ERROR: invalid choice {choice}, expected a number 0-{LAST_SECTION}")
        sys.exit(1)

    if interactive:
        if choice != 0 and SECTIONS[choice][2] == PLAIN_MODES:
            return choice, "auto"
        print("Select variant:")
        for i, name in enumerate(MODES, 1):
            default = " (default)" if name == "auto" else ""
            print(f"  {i} - {name}: {VARIANT_HELP[name]}{default}")
        mode = parse_variant(input(f"Variant [1-{len(MODES)}, Enter = 1]: "))
    else:
        mode = parse_variant(sys.argv[2]) if len(sys.argv) > 2 else "auto"

    if choice != 0 and mode not in SECTIONS[choice][2]:
        print(f"ERROR: section {choice} is not affected by variant '{mode}'")
        sys.exit(1)

    return choice, mode


def runtime_lines(mode):
    if mode in COMPARE:
        ref_name = "st" if mode == "compare" else "scalar"
        runs = [("auto", {}), (ref_name, COMPARE[mode][1])]
    else:
        runs = [(mode, MODE_ENV[mode])]
    lines = []
    for name, env in runs:
        info = run_worker(info_worker, (), env)
        lines.append(f"{name}: backend={info['backend']}, threads={info['threads']}")
    return lines


# =========================================================
# MAIN
# =========================================================
def main():
    if not os.path.exists(DLL_PATH):
        print(f"ERROR: not found: {DLL_PATH}")
        sys.exit(1)

    try:
        load_lib()
    except (OSError, AttributeError) as e:
        print(f"ERROR: cannot use {DLL_PATH}: {e}")
        sys.exit(1)

    choice, mode = prompt_selection()
    width = max(WIDTH_CMP, WIDTH_MULTI)

    print("=" * width)
    print(f"{'LUSTRO.DLL -- SPEED TEST (normalized per round)':^{width}}")
    print(f"{'DLL: ' + DLL_PATH:^{width}}")
    print(f"{'HW logical threads: ' + str(HW_THREADS):^{width}}")
    env = [f"{k}={os.environ[k]}" for k in ENV_VARS if k in os.environ]
    if env:
        print(f"{'Env: ' + ' '.join(env):^{width}}")
    print(f"{'Variant: ' + mode:^{width}}")
    for line in runtime_lines(mode):
        print(f"{line:^{width}}")
    if choice == 0:
        print(f"{'Running: ALL sections':^{width}}")
    else:
        print(f"{'Running: section ' + str(choice) + ' - ' + SECTIONS[choice][0]:^{width}}")
    print("=" * width)

    sections_to_run = range(1, LAST_SECTION + 1) if choice == 0 else [choice]
    for i in sections_to_run:
        title, fn, modes = SECTIONS[i]
        if mode not in modes:
            print(f"\n[{i}] {title}: skipped, not affected by variant '{mode}'")
            continue
        fn(mode)

    print("\n" + "=" * width)
    print(
        "ns/round is per engine round, comparable across tables.\n"
        "Data rate: input bytes for HASH rows, output bytes for stream rows.\n"
        "Steps: value suggested by the library for that N.\n"
        "Small calls and single-stream rows (*_FILL) include ctypes call overhead.\n"
        "Calls are timed back to back, so the thread pool stays active between them.\n"
        "Variants: scalar = AVX2 off, st = Rayon off. Side by side: vs ST = p50 with Rayon off /\n"
        "p50 of auto (compare), vs scalar = p50 with AVX2 off / p50 of auto (simd).\n"
        "AVX2 is used for stream batches of 4 or more lanes and for hash batches (fixed and var)\n"
        "of 4 or more messages; single streams are scalar.\n"
        "HASH_VAR / VAR_L rows: lengths uniform in 0..2*mean; Rounds and Data rate follow the actual\n"
        "lengths, ns/round is per actual round.\n"
        "Multi-stream: ns/round agg = wall time per round over all procs, ns/round proc = mean\n"
        "per proc, Scaling = agg / (procs * agg at 1 proc), Min/Max = slowest / fastest proc.\n"
        "Procs are logical threads, SMT and clocks lower Scaling above the physical core count."
    )
    print("=" * width)


if __name__ == "__main__":
    multiprocessing.set_start_method("spawn", force=True)
    main()