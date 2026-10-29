"""
Lustro speed test, normalized per engine round. One script, three targets:
  rust    the native API, examples/speed.rs run as a separate process per measurement
  python  the `lustro` module (NumPy), GIL held for the whole call (release_gil=False),
          input read in place, output written in place into a preallocated buffer
  c       lustro.dll through ctypes
Same sections, tables and row labels for all, so the outputs can be compared line by line.
Same inputs too: HASH_VAR lengths are generated here and handed to every target.
Stream batch calls use the `steps` suggested by the library for the given N.
Multi-stream sections run one single stream per process and report the aggregate.
Usage: [rust|python|c] [section 0-10] [variant 1-5, or auto, scalar, st, compare, simd]
Without arguments the script asks. With arguments and no target, rust is used.
lustro.dll is searched only in <script directory>/target/release and
<script directory>/target/release-ffi (newest loadable build with the C API).
The Rust worker is <script directory>/target/release/examples/speed;
build it with `cargo build --release --example speed` from that project directory.
"""

import array
import contextlib
import ctypes
import multiprocessing
import os
import queue
import random
import struct
import subprocess
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

TARGETS = ("rust", "python", "c")
TARGET_WORDS = {"rust": "rust", "python": "python", "py": "python", "c": "c"}
TARGET_HELP = {
    "rust": "native Rust API, examples/speed.rs in a separate process",
    "python": "lustro module, GIL held during the calls (release_gil=False)",
    "c": "lustro.dll through ctypes",
}
DEFAULT_TARGET = TARGETS[0]
TARGET = DEFAULT_TARGET

if sys.platform == "win32":
    DLL_NAME = "lustro.dll"
    EXE_NAME = "speed.exe"
elif sys.platform == "darwin":
    DLL_NAME = "liblustro.dylib"
    EXE_NAME = "speed"
else:
    DLL_NAME = "liblustro.so"
    EXE_NAME = "speed"


PROFILES = ("release", "release-ffi")

def dll_candidates():
    """Only use target/<profile> next to this script."""
    here = os.path.dirname(os.path.abspath(__file__))
    return [os.path.join(here, "target", profile, DLL_NAME) for profile in PROFILES]


def exe_candidates():
    """Only use target/release/examples next to this script."""
    here = os.path.dirname(os.path.abspath(__file__))
    return [os.path.join(here, "target", "release", "examples", EXE_NAME)]


DLL_ERRORS = []


def find_dll():
    """Newest candidate that loads and exports the C API; why the others did not is kept in
    DLL_ERRORS."""
    best = None
    for path in dll_candidates():
        if not os.path.isfile(path):
            continue
        try:
            if hasattr(os, "add_dll_directory"):
                os.add_dll_directory(os.path.dirname(path))
            lib = ctypes.CDLL(path)
            if not hasattr(lib, "lustro_api_version"):
                DLL_ERRORS.append(f"{path}: no C API exports (built without --features ffi?)")
                continue
        except OSError as e:
            DLL_ERRORS.append(f"{path}: {e}")
            continue
        if best is None or os.path.getmtime(path) > os.path.getmtime(best):
            best = path
    return best


DLL_PATH = find_dll()


def find_exe():
    """Newest existing Rust worker."""
    found = [p for p in exe_candidates() if os.path.isfile(p)]
    return max(found, key=os.path.getmtime) if found else None


EXE_PATH = find_exe()

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
    return summarize_pcts(
        percentile(times, 50), percentile(times, 95), n_elements, rounds_per_elem, total_bytes
    )


def summarize_pcts(p50, p95, n_elements, rounds_per_elem, total_bytes):
    """summarize() from percentiles in seconds."""
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
    if not DLL_PATH:
        raise FileNotFoundError(f"no loadable {DLL_NAME} found")
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
# PYTHON BINDING (target "python")
# =========================================================
def load_py():
    """Imports lustro and NumPy inside the worker, after its environment is set."""
    import numpy as np
    import lustro

    return lustro, np


# =========================================================
# RUST BINDING (target "rust"): examples/speed.rs, timing runs inside that process
# =========================================================
def rust_cmd(*args):
    if not EXE_PATH:
        raise FileNotFoundError(f"no {EXE_NAME} found")
    return [EXE_PATH, *map(str, args)]


def rust_call(*args, stdin=None):
    """One run of the worker; its last stdout line is `key=value ...`."""
    proc = subprocess.run(rust_cmd(*args), input=stdin, capture_output=True)
    if proc.returncode != 0:
        err = proc.stderr.decode(errors="replace").strip()
        raise RuntimeError(f"{EXE_NAME} {args[0]} exited with code {proc.returncode}: {err}")
    lines = proc.stdout.decode().split()
    return dict(item.split("=", 1) for item in lines)


def rust_summary(res, n_elements, rounds_per_elem, total_bytes):
    return summarize_pcts(float(res["p50"]), float(res["p95"]), n_elements, rounds_per_elem, total_bytes)


# =========================================================
# WORKERS: each setup returns (call, free) for the chosen target; timing is shared.
# =========================================================
def info_worker(target, q):
    try:
        if target == "rust":
            info = rust_call("info")
            q.put({
                "backend": info["backend"],
                "threads": int(info["threads"]),
                "lines": [
                    f"lustro {info['version']}, {info['build']} build: {EXE_PATH}",
                ],
            })
        elif target == "c":
            lib = load_lib()
            q.put({
                "backend": lib.lustro_runtime_backend().decode(),
                "threads": lib.lustro_runtime_threads(),
                "lines": [f"DLL: {DLL_PATH}"],
            })
        else:
            lustro, np = load_py()
            info = lustro.runtime_info()
            q.put({
                "backend": info["backend"],
                "threads": info["threads"],
                "lines": [
                    f"lustro {lustro.__version__}: {getattr(lustro, '__file__', '?')}",
                    f"Python {sys.version.split()[0]}, NumPy {np.__version__}",
                ],
            })
    except Exception as e:
        q.put({"error": repr(e)})


def setup_hash_fixed(target, n, msg_len):
    raw = os.urandom(n * msg_len)
    if target == "c":
        lib = load_lib()
        data = u8_buf(raw)
        data_ptr = ctypes.cast(data, U8P)
        out = (ctypes.c_uint8 * (n * 32))()
        out_ptr = ctypes.cast(out, U8P)

        def call():
            err = lib.lustro_hash256_many_fixed(data_ptr, n, msg_len, out_ptr)
            if err != 0:
                raise RuntimeError(f"lustro_hash256_many_fixed returned error {err}")

        call.keep = (data, out)
        return call, lambda: None

    lustro, np = load_py()
    data = np.frombuffer(raw, np.uint8).reshape(n, msg_len)
    out = np.empty((n, 32), np.uint8)

    def call():
        lustro.hash256_many_fixed_into(data, out, release_gil=False)

    call()
    if bytes(out[0]) != lustro.hash256(raw[:msg_len]):
        raise RuntimeError("hash256_many_fixed_into: row 0 differs from hash256")
    return call, lambda: None


def hash_batch_worker(target, n, msg_len, q):
    try:
        total_bytes = n * msg_len
        rounds = hash_rounds_for_len(msg_len)
        if target == "rust":
            res = rust_call("hash_fixed", MIN_TIME, MIN_ITERS, n, msg_len)
            result = rust_summary(res, n, rounds, total_bytes)
        else:
            call, free = setup_hash_fixed(target, n, msg_len)
            try:
                times = timed_loop(call)
            finally:
                free()
            result = summarize(times, n, rounds, total_bytes)
        q.put({"result": result, "steps": None, "size_bytes": total_bytes})
    except Exception as e:
        q.put({"error": repr(e)})


def setup_hash_var(target, lens):
    n = len(lens)
    raw = os.urandom(sum(lens))
    if target == "c":
        lib = load_lib()
        data = u8_buf(raw)
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

        call.keep = (data, ptr_array, len_array, out)
        return call, lambda: None

    lustro, np = load_py()
    messages = []
    pos = 0
    for length in lens:
        messages.append(raw[pos:pos + length])
        pos += length
    out = np.empty((n, 32), np.uint8)

    def call():
        lustro.hash256_many_var_into(messages, out, release_gil=False)

    call()
    for k in (0, n - 1):
        if bytes(out[k]) != lustro.hash256(messages[k]):
            raise RuntimeError(f"hash256_many_var_into: row {k} differs from hash256")
    return call, lambda: None


def hash_var_worker(target, n, mean_len, q):
    """
    hash256_many_var on n messages with lengths uniform in 0..2*mean_len (fixed seed).
    Rounds and data rate are counted from the actual lengths.
    """
    try:
        rng = random.Random(HASH_VAR_SEED)
        lens = [rng.randint(0, 2 * mean_len) for _ in range(n)]
        total_bytes = sum(lens)
        rounds = sum(hash_rounds_for_len(length) for length in lens)
        if target == "rust":
            res = rust_call(
                "hash_var", MIN_TIME, MIN_ITERS, stdin=struct.pack(f"<{n}I", *lens)
            )
            result = rust_summary(res, 1, rounds, total_bytes)
        else:
            call, free = setup_hash_var(target, lens)
            try:
                times = timed_loop(call)
            finally:
                free()
            result = summarize(times, 1, rounds, total_bytes)
        q.put({"result": result, "steps": None, "size_bytes": total_bytes})
    except Exception as e:
        q.put({"error": repr(e)})


def setup_single(target, kind, size):
    """One stream and one buffer of `size` bytes: the call fills the buffer."""
    if target == "c":
        lib = load_lib()
        ctx, fill, free, fill_name = open_single(lib, kind)
        out = (ctypes.c_uint8 * size)()
        out_ptr = ctypes.cast(out, U8P)

        def call():
            err = fill(ctx, out_ptr, size)
            if err != 0:
                raise RuntimeError(f"{fill_name} returned error {err}")

        call.keep = (out,)
        return call, lambda: free(ctx)

    lustro, np = load_py()
    stream = lustro.LustroPrng(bytes(range(32))) if kind == "PRNG" else lustro.LustroXof(b"m0")
    buf = bytearray(size)

    def call():
        stream.fill_into(buf)

    return call, lambda: None


def fill_single_worker(target, kind, size, q):
    """
    One prng/xof fill call on a single stream.
    size is a multiple of 32, so rounds = size // 32 (one round per 32 B block).
    """
    try:
        if target == "rust":
            res = rust_call("single", MIN_TIME, MIN_ITERS, kind.lower(), size)
            result = rust_summary(res, 1, size // 32, size)
        else:
            call, free = setup_single(target, kind, size)
            try:
                times = timed_loop(call)
            finally:
                free()
            result = summarize(times, 1, size // 32, size)
        q.put({"result": result, "steps": None, "size_bytes": size})
    except Exception as e:
        q.put({"error": repr(e)})


def multi_fill_worker(target, kind, size, barrier, q):
    """
    One process of the multi-stream test: own stream, own buffer. All processes
    start the timed window together (barrier) and fill for MULTI_TIME.
    """
    try:
        if target == "rust":
            raise RuntimeError("rust multi runs through run_multi_rust")
        call, free = setup_single(target, kind, size)

        try:
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
            free()
    except Exception as e:
        try:
            barrier.abort()
        except Exception:
            pass
        q.put({"error": repr(e)})


def setup_stream_batch(target, kind, n):
    """n streams; returns (call, free, steps, out_len). The call advances every stream `steps` rounds."""
    if target == "c":
        lib = load_lib()
        if kind == "PRNG":
            seed = u8_buf(bytes(range(32)))
            keep = (seed,)
            ctx = new_ctx(lib.lustro_prng_batch_new_range, seed, 0, 0, n)
            fill = lib.lustro_prng_batch_fill_blocks
            free = lib.lustro_prng_batch_free
            suggested = lib.lustro_prng_batch_suggested_steps
            fill_name = "lustro_prng_batch_fill_blocks"
        else:
            # One flat buffer of n fixed 8 B messages; pointers are offsets into it.
            blob = u8_buf(array.array("Q", range(n)).tobytes())
            base = ctypes.addressof(blob)
            ptr_array = (SizeT * n)(*range(base, base + 8 * n, 8))
            len_array = (SizeT * n)(*([8] * n))
            keep = (blob, ptr_array, len_array)
            ctx = new_ctx(
                lib.lustro_xof_batch_new, ctypes.cast(ptr_array, ctypes.POINTER(U8P)), len_array, n
            )
            fill = lib.lustro_xof_batch_fill_blocks
            free = lib.lustro_xof_batch_free
            suggested = lib.lustro_xof_batch_suggested_steps
            fill_name = "lustro_xof_batch_fill_blocks"
        try:
            steps = suggested(ctx)
            if steps == 0:
                raise RuntimeError("suggested_steps returned 0")
            out_len = n * steps * 32
            out = (ctypes.c_uint8 * out_len)()
            out_ptr = ctypes.cast(out, U8P)
        except Exception:
            free(ctx)
            raise

        def call():
            err = fill(ctx, out_ptr, out_len, steps)
            if err != 0:
                raise RuntimeError(f"{fill_name} returned error {err}")

        call.keep = (out, keep)
        return call, (lambda: free(ctx)), steps, out_len

    lustro, np = load_py()
    if kind == "PRNG":
        batch = lustro.LustroPrngBatch.new_range(bytes(range(32)), 0, n)
    else:
        batch = lustro.LustroXofBatch([i.to_bytes(8, "little") for i in range(n)])
    steps = batch.suggested_steps()
    if steps == 0:
        raise RuntimeError("suggested_steps returned 0")
    out = np.zeros((steps, n, 4), dtype=np.uint64)

    def call():
        batch.fill_blocks(out, steps, release_gil=False)

    return call, (lambda: None), steps, out.nbytes


def stream_batch_worker(target, kind, n, q):
    """
    One fill_blocks() call: each of the n streams advances `steps` rounds,
    n * steps blocks out (step-major). `steps` comes from the library.
    kind is "PRNG" or "XOF".
    """
    try:
        if target == "rust":
            res = rust_call("batch", MIN_TIME, MIN_ITERS, kind.lower(), n)
            steps, out_len = int(res["steps"]), int(res["bytes"])
            result = rust_summary(res, n, steps, out_len)
        else:
            call, free, steps, out_len = setup_stream_batch(target, kind, n)
            try:
                times = timed_loop(call)
            finally:
                free()
            result = summarize(times, n, steps, out_len)
        q.put({"result": result, "steps": steps, "size_bytes": out_len})
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


def run_multi_rust(kind, size, procs):
    """Native twin of run_multi: `procs` worker processes, each with its own stream and buffer.
    A worker prints `ready` after its warmup and starts the timed window on a line from stdin,
    so all windows open together."""
    cmd = rust_cmd("multi", kind.lower(), size, MULTI_WARMUP, MULTI_TIME)
    kids = [
        subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
        )
        for _ in range(procs)
    ]

    def stderr_of(kid):
        kid.kill()
        kid.wait()
        return kid.stderr.read().strip()

    def expect(kid, key):
        fields = kid.stdout.readline().split()
        if not fields or fields[0].split("=")[0] != key:
            raise RuntimeError(f"{EXE_NAME} multi: {stderr_of(kid) or 'no output'}")
        return dict(item.split("=", 1) for item in fields if "=" in item)

    try:
        for kid in kids:
            expect(kid, "ready")
        for kid in kids:
            kid.stdin.write("go\n")
            kid.stdin.flush()
        results = []
        for kid in kids:
            fields = expect(kid, "calls")
            results.append({"calls": int(fields["calls"]), "elapsed": float(fields["elapsed"])})
        return results
    finally:
        for kid in kids:
            if kid.poll() is None:
                kid.kill()
            kid.wait()
            for pipe in (kid.stdin, kid.stdout, kid.stderr):
                try:
                    pipe.close()
                except OSError:
                    pass


def run_multi(kind, size, procs):
    """Runs `procs` multi_fill_worker processes together, returns their results."""
    if TARGET == "rust":
        return run_multi_rust(kind, size, procs)
    barrier = multiprocessing.Barrier(procs)
    q = multiprocessing.Queue()
    workers = [
        multiprocessing.Process(target=multi_fill_worker, args=(TARGET, kind, size, barrier, q))
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
    res = run_worker(func, (TARGET, *args), MODE_ENV[mode])
    vs_ref = None
    if mode in COMPARE:
        ref = run_worker(func, (TARGET, *args), COMPARE[mode][1])
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
        table_start(f"{kind}_FILL  (single stream, N=1; includes call overhead)", mode)
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


def parse_target(raw):
    raw = raw.strip().lower()
    if not raw:
        return DEFAULT_TARGET
    if raw.isdigit() and int(raw) in range(1, len(TARGETS) + 1):
        return TARGETS[int(raw) - 1]
    if raw in TARGET_WORDS:
        return TARGET_WORDS[raw]
    print(f"ERROR: invalid target '{raw}', expected 1-{len(TARGETS)}, {', '.join(TARGETS)}")
    sys.exit(1)


def prompt_selection():
    words = [a for a in sys.argv[1:] if a.lower() in TARGET_WORDS]
    args = [a for a in sys.argv[1:] if a.lower() not in TARGET_WORDS]
    if len(words) > 1:
        print("ERROR: more than one target given")
        sys.exit(1)
    interactive = not sys.argv[1:]

    if words:
        target = TARGET_WORDS[words[0].lower()]
    elif interactive:
        print("Select target:")
        for i, name in enumerate(TARGETS, 1):
            default = " (default)" if name == DEFAULT_TARGET else ""
            print(f"  {i} - {name}: {TARGET_HELP[name]}{default}")
        target = parse_target(input(f"Target [1-{len(TARGETS)}, Enter = 1]: "))
    else:
        target = DEFAULT_TARGET

    print("Select which test to run:")
    print("  0 - RUN ALL (default)")
    for i in range(1, LAST_SECTION + 1):
        print(f"  {i} - {SECTIONS[i][0]}")

    interactive = not args
    if interactive:
        raw = input(f"Choice [0-{LAST_SECTION}, Enter = 0]: ").strip() or "0"
    else:
        raw = args[0] if args else "0"

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
            return target, choice, "auto"
        print("Select variant:")
        for i, name in enumerate(MODES, 1):
            default = " (default)" if name == "auto" else ""
            print(f"  {i} - {name}: {VARIANT_HELP[name]}{default}")
        mode = parse_variant(input(f"Variant [1-{len(MODES)}, Enter = 1]: "))
    else:
        mode = parse_variant(args[1]) if len(args) > 1 else "auto"

    if choice != 0 and mode not in SECTIONS[choice][2]:
        print(f"ERROR: section {choice} is not affected by variant '{mode}'")
        sys.exit(1)

    return target, choice, mode


def runtime_lines(mode):
    if mode in COMPARE:
        ref_name = "st" if mode == "compare" else "scalar"
        runs = [("auto", {}), (ref_name, COMPARE[mode][1])]
    else:
        runs = [(mode, MODE_ENV[mode])]
    lines = []
    details = []
    for name, env in runs:
        info = run_worker(info_worker, (TARGET,), env)
        lines.append(f"{name}: backend={info['backend']}, threads={info['threads']}")
        if not details:
            details = info["lines"]
    return details + lines


# =========================================================
# MAIN
# =========================================================
def main():
    global TARGET
    TARGET, choice, mode = prompt_selection()

    if TARGET == "rust":
        if not EXE_PATH:
            print(f"ERROR: no {EXE_NAME} found.")
            print("Build it with: cargo build --release --example speed")
            print("Searched:")
            for path in exe_candidates():
                print(f"  {path}")
            sys.exit(1)
    elif TARGET == "c":
        if not DLL_PATH:
            print(f"ERROR: no loadable {DLL_NAME} found.")
            if DLL_ERRORS:
                print("Found but not loadable:")
                for line in DLL_ERRORS:
                    print(f"  {line}")
            print("Searched:")
            for path in dll_candidates():
                print(f"  {path}")
            sys.exit(1)
        try:
            load_lib()
        except (OSError, AttributeError) as e:
            print(f"ERROR: cannot use {DLL_PATH}: {e}")
            sys.exit(1)
    else:
        try:
            load_py()
        except ImportError as e:
            print(f"ERROR: cannot import lustro or numpy: {e}")
            sys.exit(1)

    width = max(WIDTH_CMP, WIDTH_MULTI)
    title = {
        "rust": "LUSTRO (RUST, native API) -- SPEED TEST (normalized per round)",
        "python": "LUSTRO (PYTHON, GIL held) -- SPEED TEST (normalized per round)",
        "c": "LUSTRO.DLL (C) -- SPEED TEST (normalized per round)",
    }[TARGET]

    print("=" * width)
    print(f"{title:^{width}}")
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

    target_notes = {
        "rust": "Target rust: the native API, called from examples/speed.rs; this script only starts it and\n"
        "reads p50 / p95 back, the timing loop runs inside that process. Input and output are\n"
        "preallocated: HASH_* hash256_many_fixed_into on contiguous bytes, HASH_VAR\n"
        "hash256_many_var_into on a prebuilt slice table, *_BATCH fill_blocks into [[u8; 32]],\n"
        "*_FILL fill_bytes into a Vec<u8>. Message bytes come from a fixed-seed generator; HASH_VAR\n"
        "lengths are the ones the other targets use. The worker is a fresh process per row, as for\n"
        "the other targets, and the *_MULTI rows start one worker per stream.\n"
        "Small calls and single-stream rows (*_FILL) include only the call itself.\n",
        "python": "Target python: every call runs with the GIL held (release_gil=False); other Python threads wait.\n"
        "Input and output are preallocated and used in place: HASH_* hash256_many_fixed_into on an\n"
        "(N, msg_len) uint8 array, HASH_VAR hash256_many_var_into on a list of bytes, *_BATCH\n"
        "fill_blocks into a (steps, N, 4) uint64 array, *_FILL fill_into a bytearray.\n"
        "Small calls and single-stream rows (*_FILL) include Python call overhead.\n",
        "c": "Target c: ctypes calls into lustro.dll; input and output are preallocated ctypes buffers,\n"
        "HASH_VAR uses a prebuilt pointer/length table.\n"
        "Small calls and single-stream rows (*_FILL) include ctypes call overhead.\n",
    }
    target_note = target_notes[TARGET]
    print("\n" + "=" * width)
    print(
        "ns/round is per engine round, comparable across tables.\n"
        "Data rate: input bytes for HASH rows, output bytes for stream rows.\n"
        "Steps: value suggested by the library for that N.\n"
        + target_note +
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