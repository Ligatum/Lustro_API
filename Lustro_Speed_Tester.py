"""
lustro.dll speed test (ctypes), normalized per engine round. lustro.dll must be in the same directory.
Batch calls use the `steps` suggested by the library for the given N.
"""

import ctypes
import multiprocessing
import os
import queue
import sys
import time

# =========================================================
# CONFIG
# =========================================================
MIN_TIME = 1.0
MIN_ITERS = 100

# Spread over single-thread, pool, cache-resident and memory-bound batches.
BATCH_SIZES = [64, 256, 1024, 4096, 16384, 65536, 131072, 196608]

# Exact multiple of 32 B: no terminator round (absorb_with_domain, api.rs).
HASH_MSG_LEN_BASELINE = 32

# Lengths on both sides of a 32 B boundary (31/32/33, 64/65) to separate
# terminator cost from round count.
HASH_MSG_LENGTHS = [16, 31, 32, 33, 64, 65, 128, 256, 512, 768, 1024, 2048, 4096]

# N for the length sweep.
HASH_SWEEP_N = 16384

# Single-instance calls: message lengths for lustro_hash256, buffer sizes for
# lustro_prng_fill / lustro_xof_fill. One ctypes call is timed, so small sizes
# are dominated by call overhead.
SINGLE_HASH_LENGTHS = [32, 1024, 65536, 1 << 20]
SINGLE_FILL_SIZES = [4096, 65536, 1 << 20, 16 << 20]


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


# =========================================================
# UTILS
# =========================================================
def format_size(size_bytes):
    if size_bytes < 1024:
        return f"{size_bytes} B"
    if size_bytes < 1024 ** 2:
        return f"{size_bytes // 1024} KiB"
    return f"{size_bytes // (1024 ** 2)} MiB"


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
}
WIDTH = sum(COL.values()) + 3 * (len(COL) - 1)


def make_header():
    return (
        f"{'Pipeline':<{COL['pipeline']}} | {'Batch size':<{COL['size']}} | "
        f"{'N':<{COL['n']}} | {'Steps':>{COL['steps']}} | {'Rounds':>{COL['rounds']}} | "
        f"{'p50 (ms)':>{COL['p50']}} | {'p95 (ms)':>{COL['p95']}} | "
        f"{'Throughput':>{COL['tp']}} | {'ns/round':>{COL['ns']}}"
    )


def print_row(label, n, steps, size_bytes, data):
    tp_str = f"{data['gib_s']:.2f} GiB/s"
    steps_str = "-" if steps is None else str(steps)
    print(
        f"{label:<{COL['pipeline']}} | {format_size(size_bytes):<{COL['size']}} | "
        f"{n:<{COL['n']}} | {steps_str:>{COL['steps']}} | {data['rounds_total']:>{COL['rounds']}} | "
        f"{data['p50_ms']:>{COL['p50']}.4f} | {data['p95_ms']:>{COL['p95']}.4f} | "
        f"{tp_str:>{COL['tp']}} | {data['ns_per_round']:>{COL['ns']}.2f}"
    )


# =========================================================
# DLL BINDING: 1:1 with the public C API
# =========================================================
U8P = ctypes.POINTER(ctypes.c_uint8)
SizeT = ctypes.c_size_t


def load_lib():
    if not os.path.exists(DLL_PATH):
        raise FileNotFoundError(f"not found: {DLL_PATH}")
    lib = ctypes.CDLL(DLL_PATH)

    lib.lustro_api_version.restype = ctypes.c_uint32
    lib.lustro_api_version.argtypes = []

    lib.lustro_hash256.restype = ctypes.c_int32
    lib.lustro_hash256.argtypes = [U8P, SizeT, U8P]

    lib.lustro_hash256_many.restype = ctypes.c_int32
    lib.lustro_hash256_many.argtypes = [U8P, SizeT, SizeT, U8P]

    # Single stream: new / fill(ctx, out, out_len) / free.
    lib.lustro_prng_new.restype = ctypes.c_void_p
    lib.lustro_prng_new.argtypes = [U8P, ctypes.c_uint64, ctypes.c_uint64]
    lib.lustro_prng_free.restype = None
    lib.lustro_prng_free.argtypes = [ctypes.c_void_p]
    lib.lustro_prng_fill.restype = ctypes.c_int32
    lib.lustro_prng_fill.argtypes = [ctypes.c_void_p, U8P, SizeT]

    lib.lustro_xof_new.restype = ctypes.c_void_p
    lib.lustro_xof_new.argtypes = [U8P, SizeT]
    lib.lustro_xof_free.restype = None
    lib.lustro_xof_free.argtypes = [ctypes.c_void_p]
    lib.lustro_xof_fill.restype = ctypes.c_int32
    lib.lustro_xof_fill.argtypes = [ctypes.c_void_p, U8P, SizeT]

    # fill_blocks(ctx, out, out_len, steps), output is step-major.
    # suggested_steps(ctx) is the `steps` the library recommends for len(ctx).
    lib.lustro_prng_batch_new_range.restype = ctypes.c_void_p
    lib.lustro_prng_batch_new_range.argtypes = [U8P, ctypes.c_uint64, ctypes.c_uint64, SizeT]
    lib.lustro_prng_batch_free.restype = None
    lib.lustro_prng_batch_free.argtypes = [ctypes.c_void_p]
    lib.lustro_prng_batch_fill_blocks.restype = ctypes.c_int32
    lib.lustro_prng_batch_fill_blocks.argtypes = [ctypes.c_void_p, U8P, SizeT, SizeT]
    lib.lustro_prng_batch_suggested_steps.restype = SizeT
    lib.lustro_prng_batch_suggested_steps.argtypes = [ctypes.c_void_p]

    lib.lustro_xof_batch_new.restype = ctypes.c_void_p
    lib.lustro_xof_batch_new.argtypes = [ctypes.POINTER(U8P), ctypes.POINTER(SizeT), SizeT]
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


# =========================================================
# WORKERS
# =========================================================
def hash_batch_worker(n, msg_len, q):
    try:
        lib = load_lib()

        data = u8_buf(os.urandom(n * msg_len))
        data_ptr = ctypes.cast(data, U8P)

        out = (ctypes.c_uint8 * (n * 32))()
        out_ptr = ctypes.cast(out, U8P)

        def call():
            err = lib.lustro_hash256_many(data_ptr, n, msg_len, out_ptr)
            if err != 0:
                raise RuntimeError(f"lustro_hash256_many returned error {err}")

        times = timed_loop(call)
        total_bytes = n * msg_len
        result = summarize(times, n, hash_rounds_for_len(msg_len), total_bytes)
        q.put({"result": result, "steps": None, "size_bytes": total_bytes})
    except Exception as e:
        q.put({"error": repr(e)})


def hash_single_worker(msg_len, q):
    try:
        lib = load_lib()

        data = u8_buf(os.urandom(msg_len))
        data_ptr = ctypes.cast(data, U8P)
        out = (ctypes.c_uint8 * 32)()
        out_ptr = ctypes.cast(out, U8P)

        def call():
            err = lib.lustro_hash256(data_ptr, msg_len, out_ptr)
            if err != 0:
                raise RuntimeError(f"lustro_hash256 returned error {err}")

        times = timed_loop(call)
        result = summarize(times, 1, hash_rounds_for_len(msg_len), msg_len)
        q.put({"result": result, "steps": None, "size_bytes": msg_len})
    except Exception as e:
        q.put({"error": repr(e)})


def fill_single_worker(kind, size, q):
    """
    One lustro_prng_fill / lustro_xof_fill call on a single stream.
    size is a multiple of 32, so rounds = size // 32 (one round per 32 B block).
    """
    try:
        lib = load_lib()

        if kind == "PRNG":
            seed = u8_buf(bytes(range(32)))
            ctx = lib.lustro_prng_new(seed, 0, 0)
            if not ctx:
                raise RuntimeError("lustro_prng_new returned NULL")
            fill = lib.lustro_prng_fill
            free = lib.lustro_prng_free
            fill_name = "lustro_prng_fill"
        else:
            msg = u8_buf(b"m0")
            ctx = lib.lustro_xof_new(msg, 2)
            if not ctx:
                raise RuntimeError("lustro_xof_new returned NULL")
            fill = lib.lustro_xof_fill
            free = lib.lustro_xof_free
            fill_name = "lustro_xof_fill"

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


def _make_xof_batch(lib, n):
    messages = [f"m{i}".encode() for i in range(n)]
    bufs = [u8_buf(m) for m in messages]  # keep alive until lustro_xof_batch_new returns
    ptr_array = (U8P * n)(*[ctypes.cast(b, U8P) for b in bufs])
    len_array = (SizeT * n)(*[len(m) for m in messages])
    ctx = lib.lustro_xof_batch_new(ptr_array, len_array, n)
    if not ctx:
        raise RuntimeError("lustro_xof_batch_new returned NULL")
    return ctx


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
            batch_ctx = lib.lustro_prng_batch_new_range(seed, 0, 0, n)
            if not batch_ctx:
                raise RuntimeError("lustro_prng_batch_new_range returned NULL")
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


def run_worker(func, args):
    q = multiprocessing.Queue()
    p = multiprocessing.Process(target=func, args=(*args, q))
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


# =========================================================
# PIPELINE SECTIONS: one per menu entry, each prints its own table.
# =========================================================
def section_hash_baseline(hdr):
    title = (
        f"HASH256_MANY  ({hash_rounds_for_len(HASH_MSG_LEN_BASELINE)} round / message, "
        f"msg_len={HASH_MSG_LEN_BASELINE})"
    )
    print(f"\n{title:^{WIDTH}}")
    print(hdr)
    print("-" * WIDTH)
    for n in BATCH_SIZES:
        res = run_worker(hash_batch_worker, (n, HASH_MSG_LEN_BASELINE))
        print_row("HASH", n, res["steps"], res["size_bytes"], res["result"])


def section_hash_sweep(hdr):
    title = f"HASH256_MANY -- MSG LENGTH SWEEP (N={HASH_SWEEP_N} fixed; N column below = msg_len)"
    print(f"\n{title:^{WIDTH}}")
    print(hdr)
    print("-" * WIDTH)
    for msg_len in HASH_MSG_LENGTHS:
        res = run_worker(hash_batch_worker, (HASH_SWEEP_N, msg_len))
        print_row(f"HASH_L{msg_len}", msg_len, res["steps"], res["size_bytes"], res["result"])


def section_hash_single(hdr):
    title = "HASH256  (single message per call; N column = msg_len; includes ctypes call overhead)"
    print(f"\n{title:^{WIDTH}}")
    print(hdr)
    print("-" * WIDTH)
    for msg_len in SINGLE_HASH_LENGTHS:
        res = run_worker(hash_single_worker, (msg_len,))
        print_row(f"HASH_L{msg_len}", msg_len, res["steps"], res["size_bytes"], res["result"])


def make_fill_single_section(kind):
    def section(hdr):
        title = f"{kind}_FILL  (single stream, N=1; includes ctypes call overhead)"
        print(f"\n{title:^{WIDTH}}")
        print(hdr)
        print("-" * WIDTH)
        for size in SINGLE_FILL_SIZES:
            res = run_worker(fill_single_worker, (kind, size))
            print_row(f"{kind}_FILL", 1, res["steps"], res["size_bytes"], res["result"])
    return section


def make_stream_section(kind):
    def section(hdr):
        title = f"{kind}_BATCH_FILL_BLOCKS  (steps chosen by the library)"
        print(f"\n{title:^{WIDTH}}")
        print(hdr)
        print("-" * WIDTH)
        for n in BATCH_SIZES:
            res = run_worker(stream_batch_worker, (kind, n))
            print_row(kind, n, res["steps"], res["size_bytes"], res["result"])
    return section


SECTIONS = {
    1: ("HASH256_MANY (baseline, msg_len=32)", section_hash_baseline),
    2: ("HASH256_MANY message-length sweep", section_hash_sweep),
    3: ("PRNG_BATCH_FILL_BLOCKS", make_stream_section("PRNG")),
    4: ("XOF_BATCH_FILL_BLOCKS", make_stream_section("XOF")),
    5: ("HASH256 (single message)", section_hash_single),
    6: ("PRNG_FILL (single stream)", make_fill_single_section("PRNG")),
    7: ("XOF_FILL (single stream)", make_fill_single_section("XOF")),
}
LAST_SECTION = max(SECTIONS)


# =========================================================
# SELECTION MENU
# =========================================================
def prompt_selection():
    print("Select which test to run:")
    print("  0 - RUN ALL")
    for i in range(1, LAST_SECTION + 1):
        print(f"  {i} - {SECTIONS[i][0]}")

    if len(sys.argv) > 1:
        raw = sys.argv[1]
    else:
        raw = input(f"Choice [0-{LAST_SECTION}]: ").strip()

    try:
        choice = int(raw)
    except ValueError:
        print(f"ERROR: invalid choice '{raw}', expected a number 0-{LAST_SECTION}")
        sys.exit(1)

    if choice not in range(0, LAST_SECTION + 1):
        print(f"ERROR: invalid choice {choice}, expected a number 0-{LAST_SECTION}")
        sys.exit(1)

    return choice


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

    choice = prompt_selection()
    hdr = make_header()

    print("=" * WIDTH)
    print(f"{'LUSTRO.DLL -- BATCH SPEED TEST (normalized per round)':^{WIDTH}}")
    print(f"{'DLL: ' + DLL_PATH:^{WIDTH}}")
    print(f"{'HW logical threads: ' + str(HW_THREADS):^{WIDTH}}")
    env = [f"{k}={os.environ[k]}" for k in ENV_VARS if k in os.environ]
    if env:
        print(f"{'Env: ' + ' '.join(env):^{WIDTH}}")
    if choice == 0:
        print(f"{'Running: ALL sections':^{WIDTH}}")
    else:
        print(f"{'Running: section ' + str(choice) + ' - ' + SECTIONS[choice][0]:^{WIDTH}}")
    print("=" * WIDTH)

    sections_to_run = range(1, LAST_SECTION + 1) if choice == 0 else [choice]
    for i in sections_to_run:
        _, fn = SECTIONS[i]
        fn(hdr)

    if choice == 0:
        print("\n" + "=" * WIDTH)
        print(
            "ns/round is normalized per engine round, so it compares across tables.\n"
            "Steps is the value suggested by the library for that N.\n"
            "Single-instance rows (HASH256, *_FILL) include ctypes call overhead; compare\n"
            "them only at large sizes."
        )
        print("=" * WIDTH)


if __name__ == "__main__":
    multiprocessing.set_start_method("spawn", force=True)
    main()