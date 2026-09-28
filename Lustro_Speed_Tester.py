"""
lustro.dll speed test (ctypes), normalized per engine round, dll file must be present in the same directory.
"""

import ctypes
import os
import sys
import time
import multiprocessing
import numpy as np
import gc
import psutil

# =========================================================
# CONFIG
# =========================================================
MIN_TIME = 3.0
MIN_ITERS = 50

# Sizes bracket the scalar/parallel thresholds in dispatch.rs.
# Re-check them after retuning.
BATCH_SIZES = [128, 256, 512, 1024, 1536, 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144]

# Exact multiple of 32 B: no terminator round (absorb_with_domain, api.rs).
HASH_MSG_LEN_BASELINE = 32

# Lengths on both sides of a 32 B boundary (31/32/33, 64/65) to separate
# terminator cost from round count.
HASH_MSG_LENGTHS = [16, 31, 32, 33, 64, 65, 128, 256, 512, 768, 1024, 2048, 4096]

# N for the length sweep.
HASH_SWEEP_N = 131072

# N stays below the message-count threshold, so any parallel speedup here
# comes from the byte threshold (N * msg_len >= 65536).
SHOWCASE_N_LIST = [128, 256, 512, 768, 1024]
SHOWCASE_MSG_LENGTHS = [256, 512, 1024, 2048, 4096]

# Steps per fill_blocks() call; each value is a menu entry.
STEPS_OPTIONS = [1, 4, 16]


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


CPU_FREQ_GHZ = 4.5

CACHE_L1_MAX = 32 * 1024
CACHE_L2_MAX = 512 * 1024
CACHE_L3_MAX = 16 * 1024 * 1024

# Display only. The pool size comes from available_parallelism() in
# init_pool() (dispatch.rs) and can't be set from here.
HW_THREADS = psutil.cpu_count(logical=True) or 1

DLL_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "lustro.dll")

gc.disable()


# =========================================================
# UTILS
# =========================================================
def format_size(size_bytes):
    if size_bytes < 1024:
        s = f"{size_bytes} B"
    elif size_bytes < 1024 ** 2:
        s = f"{size_bytes // 1024} KB"
    else:
        s = f"{size_bytes // (1024 ** 2)} MB"

    if size_bytes <= CACHE_L1_MAX:
        label = "[L1]"
    elif size_bytes <= CACHE_L2_MAX:
        label = "[L2]"
    elif size_bytes <= CACHE_L3_MAX:
        label = "[L3]"
    else:
        label = "[RAM]"
    return f"{s}{label}"


def timed_loop(fn):
    for _ in range(20):
        fn()
    times = []
    total_start = time.perf_counter()
    iter_counter = 0
    while True:
        start = time.perf_counter_ns()
        fn()
        end = time.perf_counter_ns()
        times.append((end - start) * 1e-9)
        iter_counter += 1
        if (time.perf_counter() - total_start) > MIN_TIME and iter_counter >= MIN_ITERS:
            break
    return np.array(times)


def summarize(times, n_elements, rounds_per_elem, total_bytes):
    """
    Everything is per engine round (n_elements * rounds_per_elem), so the
    numbers compare across pipelines. cycles_per_byte and bytes_per_cycle
    use total_bytes, the usual unit when comparing primitives.
    """
    p50 = float(np.percentile(times, 50))
    p95 = float(np.percentile(times, 95))
    cycles_p50 = p50 * CPU_FREQ_GHZ * 1e9

    rounds_total = n_elements * rounds_per_elem

    return {
        "gbps": total_bytes / p50 / (1024 ** 3),
        "rounds_per_sec": rounds_total / p50,
        "ns_per_round": (p50 * 1e9) / rounds_total,
        "cycles_per_round": cycles_p50 / rounds_total,
        "cycles_per_byte": cycles_p50 / total_bytes,
        "bytes_per_cycle": total_bytes / cycles_p50,
        "p50_ms": p50 * 1000,
        "p95_ms": p95 * 1000,
        "rounds_total": rounds_total,
    }


COL = {
    "pipeline": 12,
    "size": 12,
    "thr": 3,
    "n": 8,
    "rounds": 10,
    "p50": 10,
    "p95": 10,
    "tp": 12,
    "rsec": 14,
    "ns": 10,
    "cy": 8,
    "cyb": 8,
    "bcy": 8,
}


def make_header():
    return (
        f"{'Pipeline':<{COL['pipeline']}} | {'Batch size':<{COL['size']}} | "
        f"{'Thr':<{COL['thr']}} | {'N':<{COL['n']}} | {'Rounds':>{COL['rounds']}} | "
        f"{'p50 (ms)':>{COL['p50']}} | {'p95 (ms)':>{COL['p95']}} | "
        f"{'Throughput':>{COL['tp']}} | {'Rounds/sec':>{COL['rsec']}} | "
        f"{'ns/round':>{COL['ns']}} | {'cy/round':>{COL['cy']}} | "
        f"{'cy/B':>{COL['cyb']}} | {'B/cy':>{COL['bcy']}}"
    )


def print_row(label, threads, n, size_bytes, data):
    tp_str = f"{data['gbps']:.2f} GB/s"
    rs_str = f"{data['rounds_per_sec'] / 1e6:.3f} M/s"
    print(
        f"{label:<{COL['pipeline']}} | {format_size(size_bytes):<{COL['size']}} | "
        f"{threads:<{COL['thr']}} | {n:<{COL['n']}} | {data['rounds_total']:>{COL['rounds']}} | "
        f"{data['p50_ms']:>{COL['p50']}.4f} | {data['p95_ms']:>{COL['p95']}.4f} | "
        f"{tp_str:>{COL['tp']}} | {rs_str:>{COL['rsec']}} | "
        f"{data['ns_per_round']:>{COL['ns']}.2f} | {data['cycles_per_round']:>{COL['cy']}.2f} | "
        f"{data['cycles_per_byte']:>{COL['cyb']}.3f} | {data['bytes_per_cycle']:>{COL['bcy']}.3f}"
    )


# =========================================================
# DLL BINDING: 1:1 with the public C API
# (SRC/FFI/mod.rs, hash.rs, prng.rs, xof.rs)
# =========================================================
U8P = ctypes.POINTER(ctypes.c_uint8)
SizeT = ctypes.c_size_t


def load_lib():
    if not os.path.exists(DLL_PATH):
        raise FileNotFoundError(f"not found: {DLL_PATH}")
    lib = ctypes.CDLL(DLL_PATH)

    lib.lustro_api_version.restype = ctypes.c_uint32
    lib.lustro_api_version.argtypes = []

    # No lustro_dispatcher_init: not part of the public API. The Rayon pool
    # starts on the first parallel call.

    lib.lustro_hash256_many.restype = ctypes.c_int32
    lib.lustro_hash256_many.argtypes = [U8P, SizeT, SizeT, U8P]

    # fill_blocks(ctx, out, out_len, steps), output is step-major.
    lib.lustro_prng_batch_new_range.restype = ctypes.c_void_p
    lib.lustro_prng_batch_new_range.argtypes = [U8P, ctypes.c_uint64, ctypes.c_uint64, SizeT]
    lib.lustro_prng_batch_free.restype = None
    lib.lustro_prng_batch_free.argtypes = [ctypes.c_void_p]
    lib.lustro_prng_batch_fill_blocks.restype = ctypes.c_int32
    lib.lustro_prng_batch_fill_blocks.argtypes = [ctypes.c_void_p, U8P, SizeT, SizeT]

    lib.lustro_xof_batch_new.restype = ctypes.c_void_p
    lib.lustro_xof_batch_new.argtypes = [ctypes.POINTER(U8P), ctypes.POINTER(SizeT), SizeT]
    lib.lustro_xof_batch_free.restype = None
    lib.lustro_xof_batch_free.argtypes = [ctypes.c_void_p]
    lib.lustro_xof_batch_fill_blocks.restype = ctypes.c_int32
    lib.lustro_xof_batch_fill_blocks.argtypes = [ctypes.c_void_p, U8P, SizeT, SizeT]

    return lib


def u8_buf(data: bytes):
    n = len(data)
    return (ctypes.c_uint8 * n).from_buffer_copy(data) if n else (ctypes.c_uint8 * 0)()


# =========================================================
# WORKERS
# =========================================================
def hash_batch_worker(n, msg_len, q):
    lib = load_lib()

    rng = np.random.default_rng(123)
    data = rng.integers(0, 256, size=(n, msg_len), dtype=np.uint8)
    data_flat = np.ascontiguousarray(data).reshape(-1)
    data_ptr = data_flat.ctypes.data_as(U8P)

    out = np.empty(n * 32, dtype=np.uint8)
    out_ptr = out.ctypes.data_as(U8P)

    def call():
        err = lib.lustro_hash256_many(data_ptr, n, msg_len, out_ptr)
        if err != 0:
            raise RuntimeError(f"lustro_hash256_many returned error {err}")

    times = timed_loop(call)
    total_bytes = n * msg_len
    result = summarize(times, n, hash_rounds_for_len(msg_len), total_bytes)
    q.put({"threads": HW_THREADS, "result": result, "size_bytes": total_bytes})


def _make_xof_batch(lib, n):
    messages = [f"m{i}".encode() for i in range(n)]
    bufs = [u8_buf(m) for m in messages]  # keep alive until lustro_xof_batch_new returns
    ptr_array = (U8P * n)(*[ctypes.cast(b, U8P) for b in bufs])
    len_array = (SizeT * n)(*[len(m) for m in messages])
    ctx = lib.lustro_xof_batch_new(ptr_array, len_array, n)
    if not ctx:
        raise RuntimeError("lustro_xof_batch_new returned NULL")
    return ctx


def stream_batch_worker(kind, n, steps, q):
    """
    One fill_blocks() call: each of the n streams advances `steps` rounds,
    n * steps blocks out (step-major). kind is "PRNG" or "XOF".
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
            fill_name = "lustro_prng_batch_fill_blocks"
        else:
            batch_ctx = _make_xof_batch(lib, n)
            fill = lib.lustro_xof_batch_fill_blocks
            free = lib.lustro_xof_batch_free
            fill_name = "lustro_xof_batch_fill_blocks"

        try:
            out_len = n * steps * 32
            out = np.empty(out_len, dtype=np.uint8)
            out_ptr = out.ctypes.data_as(U8P)

            def call():
                err = fill(batch_ctx, out_ptr, out_len, steps)
                if err != 0:
                    raise RuntimeError(f"{fill_name} returned error {err}")

            times = timed_loop(call)
            result = summarize(times, n, steps, out_len)
            q.put({"threads": HW_THREADS, "result": result, "size_bytes": out_len})
        finally:
            free(batch_ctx)
    except Exception as e:
        q.put({"error": repr(e)})


def run_worker(func, args):
    q = multiprocessing.Queue()
    p = multiprocessing.Process(target=func, args=(*args, q))
    p.start()
    res = q.get()
    p.join()
    if "error" in res:
        raise RuntimeError(f"worker {func.__name__} returned error: {res['error']}")
    return res


# =========================================================
# PIPELINE SECTIONS: one per menu entry, each prints its own table.
# =========================================================
def section_hash_baseline(width, hdr):
    print(
        f"\n{'HASH256_MANY  (' + str(hash_rounds_for_len(HASH_MSG_LEN_BASELINE)) + ' round / message, msg_len=' + str(HASH_MSG_LEN_BASELINE) + ')':^{width}}")
    print(hdr)
    print("-" * width)
    for n in BATCH_SIZES:
        res = run_worker(hash_batch_worker, (n, HASH_MSG_LEN_BASELINE))
        print_row("HASH", res["threads"], n, res["size_bytes"], res["result"])


def section_hash_showcase(width, hdr):
    print(
        f"\n{'HASH256_MANY -- SMALL BATCH, LARGER MESSAGES (byte-threshold showcase)':^{width}}")
    print(hdr)
    print("-" * width)
    for n in SHOWCASE_N_LIST:
        for msg_len in SHOWCASE_MSG_LENGTHS:
            res = run_worker(hash_batch_worker, (n, msg_len))
            print_row(f"HASH_L{msg_len}", res["threads"], n, res["size_bytes"], res["result"])


def section_hash_sweep(width, hdr):
    print(
        f"\n{'HASH256_MANY — MSG LENGTH SWEEP (N=' + str(HASH_SWEEP_N) + ' fixed; N column below = msg_len)':^{width}}")
    print(hdr)
    print("-" * width)
    for msg_len in HASH_MSG_LENGTHS:
        res = run_worker(hash_batch_worker, (HASH_SWEEP_N, msg_len))
        print_row(f"HASH_L{msg_len}", res["threads"], msg_len, res["size_bytes"], res["result"])


def make_stream_section(kind, steps):
    def section(width, hdr):
        print(f"\n{kind + '_BATCH_FILL_BLOCKS  (steps=' + str(steps) + ': ' + str(steps) + ' round(s) / stream / call)':^{width}}")
        print(hdr)
        print("-" * width)
        for n in BATCH_SIZES:
            res = run_worker(stream_batch_worker, (kind, n, steps))
            print_row(f"{kind}_S{steps}", res["threads"], n, res["size_bytes"], res["result"])
    return section


# Menu: 1-3 hash, then PRNG for each steps value, then XOF.
SECTIONS = {
    1: ("HASH256_MANY (baseline, msg_len=32)", section_hash_baseline),
    2: ("HASH256_MANY small-batch/large-message showcase", section_hash_showcase),
    3: ("HASH256_MANY message-length sweep", section_hash_sweep),
}
_next_id = 4
for _kind in ("PRNG", "XOF"):
    for _steps in STEPS_OPTIONS:
        SECTIONS[_next_id] = (
            f"{_kind}_BATCH_FILL_BLOCKS, steps={_steps}",
            make_stream_section(_kind, _steps),
        )
        _next_id += 1
LAST_SECTION = max(SECTIONS)


# =========================================================
# SELECTION MENU
# =========================================================
def prompt_selection():
    print("Select which test to run:")
    print("  0 - RUN ALL")
    for i in range(1, LAST_SECTION + 1):
        print(f"  {i} - {SECTIONS[i][0]}")

    # A number on the command line (python speed_test.py 4) skips the prompt.
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
    WIDTH = 171

    if not os.path.exists(DLL_PATH):
        print(f"ERROR: not found: {DLL_PATH}")
        sys.exit(1)

    choice = prompt_selection()
    hdr = make_header()

    print("=" * WIDTH)
    print(f"{'LUSTRO.DLL — BATCH SPEED TEST (normalized per round)':^{WIDTH}}")
    print(f"{'DLL: ' + DLL_PATH:^{WIDTH}}")
    print(f"{'HW logical threads: ' + str(HW_THREADS):^{WIDTH}}")
    if choice == 0:
        print(f"{'Running: ALL sections':^{WIDTH}}")
    else:
        print(f"{'Running: section ' + str(choice) + ' - ' + SECTIONS[choice][0]:^{WIDTH}}")
    print("=" * WIDTH)

    if choice == 0:
        sections_to_run = range(1, LAST_SECTION + 1)
    else:
        sections_to_run = [choice]

    for i in sections_to_run:
        _, fn = SECTIONS[i]
        fn(WIDTH, hdr)

    if choice == 0:
        print("\n" + "=" * WIDTH)
        print(
            "All ns/round and cy/round figures are comparable across every table\n"
            "above (normalized per engine round, not per element).\n"
            "The difference between the steps=" + ", ".join(str(s) for s in STEPS_OPTIONS) + " rows at the same N is the\n"
            "cost of a single DLL/Rayon entry (ctypes marshalling + pool.install()),\n"
            "amortized over more rounds per call."
        )
        print("=" * WIDTH)


if __name__ == "__main__":
    multiprocessing.set_start_method("spawn", force=True)
    main()