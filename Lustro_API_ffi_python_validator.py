"""
================================================================
LUSTRO V1 API — GOLDEN VECTOR VALIDATOR (FFI + Python)
================================================================
Validates lustro.dll in the same directory as this script.
================================================================
"""

import ctypes
import hashlib
import importlib.machinery
import importlib.util
import os
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
DLL_PATH = os.path.join(SCRIPT_DIR, "lustro.dll")

# ================================================================
# GOLDEN VECTORS
# ================================================================

HASH_VECTORS = [
    ('empty', b'', "a16b8377b406d0c43821de103a8054f8df152d9e0ea6cbccdbfd64620007fe8a", "a16b8377b406d0c43821de103a8054f8"),
    ('len1_zero', b'\x00', "5e8a34d1d5ccfa6eb12652fa1dea82bb0faae3feb0c49009975b48ceadd71e21", "5e8a34d1d5ccfa6eb12652fa1dea82bb"),
    ('len31_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', "33d8ba32c0a7b297dde8c4f4b95f3806171e3becb0d28309baef425b1540fc86", "33d8ba32c0a7b297dde8c4f4b95f3806"),
    ('len32_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', "90504df35dbf3e6238e150628ec81952df6afa70454e4bc4199bfed747c55fc5", "90504df35dbf3e6238e150628ec81952"),
    ('len33_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', "95f07e3d019ecc2b6a273b9bdffe6c4b8e53011d44cff4c22ddbc9bc7ed785d7", "95f07e3d019ecc2b6a273b9bdffe6c4b"),
    ('len63_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', "56fb9a710715aea351e914806ef7296134e99aaad9dcb381f9cd69124da9255f", "56fb9a710715aea351e914806ef72961"),
    ('len64_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', "7c201c02a6fe3bd1b88d0ff177493b6f2dec37a5c37dc94fb0e328c38a47f899", "7c201c02a6fe3bd1b88d0ff177493b6f"),
    ('len65_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', "0a3dfa070d7a4ecbd090e73f25ea179891e83f8bd8d921826d689a0712dd9578", "0a3dfa070d7a4ecbd090e73f25ea1798"),
    ('len32_ff', b'\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff', "9df7c2c5ec10fbd9e7f0bf1e33f9b952f8fd965510d9f93b76395cc7938d4895", "9df7c2c5ec10fbd9e7f0bf1e33f9b952"),
    ('len32_ascending', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f', "b5a3f963a9b41fef3fe4a12e9beb58cbe3174c7190b7b51822cb850b2754ec15", "b5a3f963a9b41fef3fe4a12e9beb58cb"),
    ('len64_ascending', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f !"#$%&\'()*+,-./0123456789:;<=>?', "dae7ec4d18e105e5ed35e247f3f4e6a74938509d3ae7925954f82c463075a301", "dae7ec4d18e105e5ed35e247f3f4e6a7"),
]

PRNG_VECTORS = [
    ('seed_zero_stream0_32', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 0, 32, "6ee8ec19b51f03056a5705675d962c17f39602cbe60099ff69bacc7e1a997869"),
    ('seed_zero_stream0_64', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 0, 64, "6ee8ec19b51f03056a5705675d962c17f39602cbe60099ff69bacc7e1a997869b08750b478760d28c813362b7fe77e0971e2451154decda83360c93b61d30b86"),
    ('seed_ff_stream1_32', b'\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff', 1, 32, "d94a37c25905fa92182b72c091dfd927e9ba28d4b5388043017067689b44f4c6"),
    ('seed_ascending_bigid', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f', 1267650600228229401496703205376, 32, "2e3a743027f5579c7af3d17e7b482c203ecb1b0f85c00dfc32b55c331e0eedf3"),
]

XOF_VECTORS = [
    ('empty', b'', 32, "d7d3ab6d1bd937326071b96af1513a40669c2131ac37e295a794eb02ec3fc306"),
    ('len1_zero', b'\x00', 32, "539acd0fa45bcb6c447be87a785ae02c5fd0e4a9636b3a43fd6207802ebc6fa8"),
    ('len31_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 32, "4c532d066b3c80bae0d45c21f05bb6e64150e1889c3fb35c4d0c620a8f28a53e"),
    ('len32_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 32, "70c6cb2cb6faeec1eda55ac9400b09a3ad0328bb499acc10eef1af0ae4bb3a48"),
    ('len32_zero_fill64', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 64, "70c6cb2cb6faeec1eda55ac9400b09a3ad0328bb499acc10eef1af0ae4bb3a48dc3b526a628ea5ad2c2de81a5074a4f3d953bea1f5d961187bd4ebcf3a9ec31e"),
    ('len33_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 32, "37b671c7725c4ac77c082200408a59c2a4ad7c5e3901e9221d81938e47ebb5ea"),
    ('len63_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 32, "1f21e09b02a99115487c6bc74e6c4e74d9dbd9cb375fc1c344134d12663487c9"),
    ('len64_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 32, "55e7ac4c8681f9163ff2536733af21dc791bfe5694f17ff7b7e01bd2326915e7"),
    ('len65_zero', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 32, "5d0b291d9150090a2b80f9cf02c593db1aee94c3a75cfbd31d1b9fd9a250bd35"),
    ('len32_ff', b'\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff', 32, "6ace491af13bcf020198e9e44c9a0d3f0de975800e3ff458007656d5250653b1"),
    ('len32_ascending', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f', 32, "e6782c97c70f82530f65c045e7b5797716ae13cd3171dd950ae3c9d76ee7d403"),
    ('len64_ascending', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f !"#$%&\'()*+,-./0123456789:;<=>?', 32, "5861572fd65f53b1b49977cf1c12abaa6935558d4166a22c4516e0dbdcf2f475"),
]

GOLDEN_FINGERPRINT = "d1f2b73d9da371903cc128dbd83b56471908b95a0025b0f809e2fc742c6e044e"

# ================================================================
# TEST HARNESS
# ================================================================
FAILURES = []


def check(label, condition, detail=""):
    status = "PASS" if condition else "FAIL"
    print(f"  [{status}] {label}" + (f" — {detail}" if detail and not condition else ""))
    if not condition:
        FAILURES.append(label)


def validate_layer(layer_name, hash256_fn, hash128_fn, prng_fill_fn, xof_fill_fn):
    sha = hashlib.sha256()

    print(f"\n== {layer_name}: HASH VECTORS ==")
    for label, msg, exp256, exp128 in HASH_VECTORS:
        got256 = hash256_fn(msg)
        got128 = hash128_fn(msg)
        sha.update(got256)
        sha.update(got128)
        check(f"{layer_name} hash256 [{label}]", got256.hex() == exp256,
              f"expected={exp256} got={got256.hex()}")
        check(f"{layer_name} hash128 [{label}]", got128.hex() == exp128,
              f"expected={exp128} got={got128.hex()}")

    print(f"\n== {layer_name}: PRNG VECTORS ==")
    for label, seed, stream_id, fill_len, exp in PRNG_VECTORS:
        got = prng_fill_fn(seed, stream_id, fill_len)
        sha.update(got)
        check(f"{layer_name} prng [{label}]", got.hex() == exp,
              f"expected={exp} got={got.hex()}")

    print(f"\n== {layer_name}: XOF VECTORS ==")
    for label, message, fill_len, exp in XOF_VECTORS:
        got = xof_fill_fn(message, fill_len)
        sha.update(got)
        check(f"{layer_name} xof [{label}]", got.hex() == exp,
              f"expected={exp} got={got.hex()}")

    fingerprint = sha.hexdigest()
    check(f"{layer_name} fingerprint", fingerprint == GOLDEN_FINGERPRINT,
          f"expected={GOLDEN_FINGERPRINT} got={fingerprint}")


# ================================================================
# FFI LAYER
# ================================================================
def try_load_ffi():
    if not os.path.exists(DLL_PATH):
        print(f"\n[FFI] skipped — {DLL_PATH} not found")
        return None
    try:
        lib = ctypes.CDLL(DLL_PATH)

        lib.lustro_hash256.restype = ctypes.c_int32
        lib.lustro_hash256.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_uint8)]

        lib.lustro_hash128.restype = ctypes.c_int32
        lib.lustro_hash128.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_uint8)]

        lib.lustro_prng_new.restype = ctypes.c_void_p
        lib.lustro_prng_new.argtypes = [ctypes.c_char_p, ctypes.c_uint64, ctypes.c_uint64]

        lib.lustro_prng_free.restype = None
        lib.lustro_prng_free.argtypes = [ctypes.c_void_p]

        lib.lustro_prng_fill.restype = ctypes.c_int32
        lib.lustro_prng_fill.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t]

        lib.lustro_xof_new.restype = ctypes.c_void_p
        lib.lustro_xof_new.argtypes = [ctypes.c_char_p, ctypes.c_size_t]

        lib.lustro_xof_free.restype = None
        lib.lustro_xof_free.argtypes = [ctypes.c_void_p]

        lib.lustro_xof_fill.restype = ctypes.c_int32
        lib.lustro_xof_fill.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t]

    except (OSError, AttributeError) as e:
        print(f"\n[FFI] skipped — {DLL_PATH} does not export the lustro_* FFI symbols ({e})")
        return None

    def hash256_fn(msg: bytes) -> bytes:
        out = (ctypes.c_uint8 * 32)()
        err = lib.lustro_hash256(msg, len(msg), out)
        if err != 0:
            raise RuntimeError(f"lustro_hash256 returned error {err}")
        return bytes(out)

    def hash128_fn(msg: bytes) -> bytes:
        out = (ctypes.c_uint8 * 16)()
        err = lib.lustro_hash128(msg, len(msg), out)
        if err != 0:
            raise RuntimeError(f"lustro_hash128 returned error {err}")
        return bytes(out)

    def prng_fill_fn(seed: bytes, stream_id: int, fill_len: int) -> bytes:
        stream_hi = (stream_id >> 64) & 0xFFFFFFFFFFFFFFFF
        stream_lo = stream_id & 0xFFFFFFFFFFFFFFFF
        ctx = lib.lustro_prng_new(seed, stream_hi, stream_lo)
        if not ctx:
            raise RuntimeError("lustro_prng_new returned NULL")
        out = (ctypes.c_uint8 * fill_len)()
        err = lib.lustro_prng_fill(ctx, out, fill_len)
        lib.lustro_prng_free(ctx)
        if err != 0:
            raise RuntimeError(f"lustro_prng_fill returned error {err}")
        return bytes(out)

    def xof_fill_fn(message: bytes, fill_len: int) -> bytes:
        ctx = lib.lustro_xof_new(message, len(message))
        if not ctx:
            raise RuntimeError("lustro_xof_new returned NULL")
        out = (ctypes.c_uint8 * fill_len)()
        err = lib.lustro_xof_fill(ctx, out, fill_len)
        lib.lustro_xof_free(ctx)
        if err != 0:
            raise RuntimeError(f"lustro_xof_fill returned error {err}")
        return bytes(out)

    return hash256_fn, hash128_fn, prng_fill_fn, xof_fill_fn


# ================================================================
# PYTHON LAYER
# ================================================================
def try_load_python_dll():
    if not os.path.exists(DLL_PATH):
        print(f"\n[Python] skipped — {DLL_PATH} not found")
        return None
    try:
        loader = importlib.machinery.ExtensionFileLoader("lustro", DLL_PATH)
        spec = importlib.util.spec_from_file_location("lustro", DLL_PATH, loader=loader)
        module = importlib.util.module_from_spec(spec)
        loader.exec_module(module)
    except Exception as e:
        print(f"\n[Python] skipped — {DLL_PATH} does not export a Python module init function ({e})")
        return None

    h = module.LustroHashPy()

    def hash256_fn(msg: bytes) -> bytes:
        return h.hash256(msg)

    def hash128_fn(msg: bytes) -> bytes:
        return h.hash128(msg)

    def prng_fill_fn(seed: bytes, stream_id: int, fill_len: int) -> bytes:
        p = module.LustroPrngPy(seed, stream_id)
        return p.fill(fill_len)

    def xof_fill_fn(message: bytes, fill_len: int) -> bytes:
        x = module.LustroXofPy(message)
        return x.fill(fill_len)

    return hash256_fn, hash128_fn, prng_fill_fn, xof_fill_fn


# ================================================================
# MAIN
# ================================================================
def main():
    width = 72
    print("=" * width)
    print(f"{'LUSTRO V1 — GOLDEN VECTOR VALIDATOR':^{width}}")
    print(f"{'directory: ' + SCRIPT_DIR:^{width}}")
    print("=" * width)

    layers = {
        "FFI": try_load_ffi(),
        "Python": try_load_python_dll(),
    }

    active = {name: fns for name, fns in layers.items() if fns is not None}

    if not active:
        sys.exit("\nERROR: no layer is available for validation.")

    for name, (hash256_fn, hash128_fn, prng_fill_fn, xof_fill_fn) in active.items():
        validate_layer(name, hash256_fn, hash128_fn, prng_fill_fn, xof_fill_fn)

    print("\n" + "=" * width)
    if FAILURES:
        print(f"RESULT: {len(FAILURES)} test(s) FAILED:")
        for f in FAILURES:
            print(f"  - {f}")
        sys.exit(1)
    else:
        print(f"RESULT: All tests passed. Golden vectors match ({', '.join(active.keys())}).")
    print("=" * width)


if __name__ == "__main__":
    main()