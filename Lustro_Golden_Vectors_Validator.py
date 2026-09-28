"""
================================================================
LUSTRO V1 API — GOLDEN VECTOR VALIDATOR (C FFI)
================================================================
Validates lustro.dll in the same directory as this script,
through the C ABI only (ctypes).
================================================================
"""

import ctypes
import hashlib
import os
import sys

EXPECTED_API_VERSION = 1

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
DLL_PATH = os.path.join(SCRIPT_DIR, "lustro.dll")

# ================================================================
# GOLDEN VECTORS
# ================================================================

# API version: 1

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

def _xof_lane_message(i: int) -> bytes:
    return bytes((i + j) & 0xFF for j in range(i % 70))

def _hash_row(i: int, length: int) -> bytes:
    return bytes((i * 31 + j * 7) & 0xFF for j in range(length))

# (label, seed, first_id, n, steps, sha256)
PRNG_BATCH_RANGE_VECTORS = [
    ('n1_s1', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f', 0, 1, 1, "7df70cb157cf81dd5783b4b050837b6c4533fdf024841e5722810554db3d06ff"),
    ('n7_s3', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 10, 7, 3, "8e2b2249c043fd1bc61a83d67a522361dcb930999b4dcfe92828bd6f57e8d71d"),
    ('n17_s5', b'\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff', 18446744073709551613, 17, 5, "d5368ce8c4a627571e4527ebcfd97fc325a5a8635bc9683d479afc08a91febf5"),
    ('n300_s2', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f', 1000, 300, 2, "35b1914913a488f53eba5f35e01349689ceb5d89c6bea647f0369e01abf0b671"),
    ('n300_s6', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f', 1000, 300, 6, "eb798396edf538283d7cacaeb5bc1f48157fcf015d23da1c4bed45c31c7a1867"),
    ('n1600_s1', b'\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00', 0, 1600, 1, "794c49bddbeda758f4e9e39d88cfd21997e08b760f70377cf991ae189e52952d"),
    ('n4099_s3', b'\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5\xa5', 1267650600228229401496703205376, 4099, 3, "37bc0a296117d3d6ae7e1b20b462b52180183aa69e720bf47f43d0da3c4aa8c5"),
]

# (label, seed, ids, steps, sha256)
PRNG_BATCH_IDS_VECTORS = [
    ('ids_sparse', b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f', [0, 5, 18446744073709551616, 1267650600228229401496703205383, 42, 170141183460469231731687303715884105728, 340282366920938463463374607431768211455], 4, "f8968d314ec3d0f7b76b1b2ec309e8184bcebd7cecb3a1f125723c921b74bf5b"),
]

# (label, batch snapshot hex, n, steps, sha256)
PRNG_BATCH_MIXED_VECTORS = [
    ('mixed_n19_s4', "01030000000000001300000000000000016441f1decb4bdf8e5240e2afc977f4e562d37e2251a0dc47f4f518bb6112c8000000000000000000000000000000000c544dbd9310eea323fb49c00e74dd7d887deb2f64bfb125b50e9c535b48edff05000000000000000000000000000000bc5b8d4a5b81ab83ddc1b6d59c8201df015c0ee3eb8fb38c8f7e2fea74ea73690a0000000000000000000000000000006d831393898f291fd7f5a34f6b1412ba6d737a16d2e7bfaa8b514a61394c826104000000000000000000000000000000747dc978cc000a673330afa49e4a09302776120d3cef4cbc25a62fe388cf01a209000000000000000000000000000000663d720a5212c308e1f540391cd99b5fdf42adb47e4ebcafd79dcd57dd2634040300000000000000000000000000000095015cd34ad9e8bb98c5f43aad3e6c9aafe89efe428cab823adeea6878d8e6d10800000000000000000000000000000014f99e99e5d616a60f4283f7383475330efebbd75146d7f2b88e484a9d6d7f2402000000000000000000000000000000dbf855cabbb7bbdbcfa54bc41636e257832033c0067e7abf8ad5715106a0205907000000000000000000000000000000e0b7d64312abcd6d202a8945f28fe653ef2dd6ddd87446e045716e6c33804c8c0100000000000000000000000000000068bee35fb5b67b50e8d37975ee21d224f59f2e4a9940cf0b7c995c1e0491565706000000000000000000000000000000384a3bc5b168561a8c3109108e2b2c4f7fd41cfa85ddb4e402df7ab4d6dd907f00000000000000000000000000000000edc99138b8a01f76df9a5ca84234ff341508e319ca76d2ee4bb99f164d0914310500000000000000000000000000000086e86a1b0fd61ca34c71061744892987afa7992ad967b24d944179ed7bcd5a970a000000000000000000000000000000e2b40f04c227a9ff0e4a1b27046055c6c185d1797319feb2a75ad96b717e0f8f04000000000000000000000000000000b4dd585eb7b65264d1ff907d756279b65bc60b301c4bcdc12a3efc148d1439aa090000000000000000000000000000001039c9185d87156f25c523028b4ba5a0d70473a0cf0f0ea3ed606df67af742de03000000000000000000000000000000ed37b2643d0dd606d0ee0fb6b65779ae67cacf0d809df59180cf50057a40e22708000000000000000000000000000000a9617804f5d1015d5b4f8b31f554442cc1cf236486556cf5f6c4b7d6176d5da902000000000000000000000000000000", 19, 4, "4a3d96c10a0344971b1259fd2a31f902103487d1182e2ec561cbaded7aca3ad4"),
]

# (label, n, steps, sha256)
XOF_BATCH_VECTORS = [
    ('n1_s1', 1, 1, "1c614355a825adb7593dee3ec20ce8b69aebe303b5ebe5027127df4a0e7cb4ae"),
    ('n7_s3', 7, 3, "5e143bb0c0a48510a9affe79cb82e7ea7641b7e9505270581c7195844aed8662"),
    ('n17_s5', 17, 5, "62e66b4c3a6aecba6098fd70625f9080fec85e980c14ce3d04fa6634259f42f0"),
    ('n300_s2', 300, 2, "9b259e81d9633d9c140c6c65fd1f2e7727b0f3f167e47a031ff8f77f9dc2dcfd"),
    ('n300_s6', 300, 6, "6c3b2e7dc83697b4939770c111cfd3ecd234187ee5f6a46db65c3252af924f5d"),
    ('n1600_s1', 1600, 1, "6d522d4a0f1f7d22889d165b59fbcf5636fe777bf9fa1dd8ce6e1ae1b50ca6eb"),
    ('n4099_s3', 4099, 3, "fa1401a0b809fd8a594c404695e0023ba33e1c1123699d5dfa55c24fc74ab873"),
]

# (label, batch snapshot hex, n, steps, sha256)
XOF_BATCH_MIXED_VECTORS = [
    ('mixed_n19_s4', "010400000000000013000000000000002cab4492aa44ba00b96a63026b338af433b0fb04f1f91bbc10a202a85ebef7e10000000000000000000000000000000086a1dc883fdfbe73604e85c20fcbe77eb335556a0671e6596c6ae378d320476705000000000000000000000000000000922c77c5d3d8fb1180e9ffdc2502eef8a1f84c3b29c78917449560fc24b974fe0a000000000000000000000000000000e198d6ff9bd5b846a2c65f44e502e079a4ded2ed2954c881611ebf0a90d056b70400000000000000000000000000000019488ca65620b47d1767f45fe6edc3ab2febbf3a2b8ea85c4479999741c8f50209000000000000000000000000000000bb97f690303100dc1d4b3f3ad39ac5a9571d302be2f6def1dc069f98ebb43ec803000000000000000000000000000000ea1423735fc0585522edd3cf80c64d1273a42b07751e5481e3a6566bd0d3e29f0800000000000000000000000000000010f3d7391fb837dfca0dab9a1cf7f4935051c50772d2636d68b80aff9ad860a102000000000000000000000000000000832360d0529d48db09c4eb2ca7bfc241e9708e997f9431b6eb816f94178d0d78070000000000000000000000000000001014497cadeec43626670be3572c6016ebb7b250a59f693249022b5d2da30e4c010000000000000000000000000000006c921abb77ebfe796bd5ff059d4459af2e85ed0ea0a0c7511b032eb7bcd0f4fb06000000000000000000000000000000fe2924c3ff9093bf75a4f67c6e10748a0811e896ad47fe34251e5bdfc2fc5dd1000000000000000000000000000000008bd56d0998eb980a089c1d6a950a9656e201ab378725ab404ee0cdeedcd603610500000000000000000000000000000038d3e4a5a5dee5baea7f7a8ef84cb91e16ef2ef1027d6c30bc9054080f99768f0a000000000000000000000000000000a53ca8668257efea1992448aab459f0b8d2a06a3ad8bb754a9cd27c6b5547a1104000000000000000000000000000000ce24b7647df00ed1c74da1726deeccea703b37a60574b572c23c92e59c5c87c5090000000000000000000000000000009e84ade46064f6c33286d9ec7b0b21cd420f9c46d3ffa26a0d46182e4de28f2203000000000000000000000000000000a3a43e3aefa4631cd73410b504ac6ce9bc611d2e37419ebce90f0a46c022891b08000000000000000000000000000000b788cf2fdfc7fbc829bbeb820b0c4831530da7ef36c2f5fa4ab194ee2ab4170602000000000000000000000000000000", 19, 4, "60731f8e67a1ba299d9c2df3d0fb9cfa9f9da38d7e58d05a40647e104d895d34"),
]

# (label, n, msg_len, sha256 of hash256 outputs, sha256 of hash128 outputs)
HASH_BATCH_VECTORS = [
    ('n1_len1', 1, 1, "d2aaffc4b717b2ed9acce78de3d7e9089daeb37d99220dd6922da0a867a30c35", "f680704ead790141b6e87c2ebdef241d3abe5a46fef4d39ccadb14a5d2f94532"),
    ('n9_len31', 9, 31, "0063f968adc2b7c9f5347dc8689a15680bf4941b87098eced7275f3fbb285688", "b744f11addacbd91f8e8347f7b1ec3ec057c8f8cf8e5c3e6ba305d165d2494e8"),
    ('n9_len32', 9, 32, "5a4e079e9bed671651d5f3d9f0b4e73326ebc6eba558d1ee7a6890a02885032a", "018280f364acf1185fc658d8f5ce95eba3e072d7382d013276e2d6f6bf0bbe19"),
    ('n9_len33', 9, 33, "d30eaa0590dc208f358badf7fb1e6900264ff9d8ec011cfbff0a29f611a0d841", "5d94f613e600ad4091b2c67dc192b7417d00d534a397e9fec273d2ec1062827c"),
    ('n1700_len40', 1700, 40, "ee9642dacc7c6095e11f9604f1e5a4ce895617b29eb4981d6ee5c82b4901dd7d", "e35c635b45f5a085b17c0bb7960de816cf4890497162c85872a32e31e01e6b4f"),
    ('n5_len70000', 5, 70000, "bf76a716f78d44402f1881cea5f158786a1d5ecbde3c4c9cff8e60b593760734", "dc394e0cb132344a7cc89bd413db21be6749e1e71112dd5ae6eaed943c5b0461"),
]

BATCH_FINGERPRINT = "64c6d698647250c40894320914a8dc8941ec7c27b15862b6a4084cd4788cf87b"

# ================================================================
# TEST HARNESS
# ================================================================
FAILURES = []


def check(label, condition, detail=""):
    status = "PASS" if condition else "FAIL"
    print(f"  [{status}] {label}" + (f" — {detail}" if detail and not condition else ""))
    if not condition:
        FAILURES.append(label)
    return condition


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


# ================================================================
# CTYPES HELPERS
# ================================================================
U8P = ctypes.POINTER(ctypes.c_uint8)
U64P = ctypes.POINTER(ctypes.c_uint64)
SizeT = ctypes.c_size_t


def u8_buf(data: bytes):
    n = len(data)
    return (ctypes.c_uint8 * n).from_buffer_copy(data) if n else (ctypes.c_uint8 * 0)()


def out_buf(n: int):
    return (ctypes.c_uint8 * n)()


def message_arrays(messages):
    # `bufs` must stay alive until the FFI call returns.
    bufs = [u8_buf(m) for m in messages]
    ptrs = (U8P * len(messages))(
        *[ctypes.cast(b, U8P) if len(m) else None for b, m in zip(bufs, messages)]
    )
    lens = (SizeT * len(messages))(*[len(m) for m in messages])
    return ptrs, lens, bufs


def split_id(x: int):
    return (x >> 64) & 0xFFFFFFFFFFFFFFFF, x & 0xFFFFFFFFFFFFFFFF


def _bind(lib, name, restype, *argtypes):
    fn = getattr(lib, name)
    fn.restype = restype
    fn.argtypes = list(argtypes)


def bind_signatures(lib):
    i32, u32, u64, vp = ctypes.c_int32, ctypes.c_uint32, ctypes.c_uint64, ctypes.c_void_p
    cp = ctypes.c_char_p

    _bind(lib, "lustro_api_version", u32)

    _bind(lib, "lustro_hash256", i32, cp, SizeT, U8P)
    _bind(lib, "lustro_hash128", i32, cp, SizeT, U8P)
    _bind(lib, "lustro_hash256_many", i32, U8P, SizeT, SizeT, U8P)
    _bind(lib, "lustro_hash128_many", i32, U8P, SizeT, SizeT, U8P)
    _bind(lib, "lustro_hash256_many_var", i32, ctypes.POINTER(U8P), SizeT, ctypes.POINTER(SizeT), U8P)
    _bind(lib, "lustro_hash128_many_var", i32, ctypes.POINTER(U8P), SizeT, ctypes.POINTER(SizeT), U8P)

    _bind(lib, "lustro_prng_new", vp, cp, u64, u64)
    _bind(lib, "lustro_prng_free", None, vp)
    _bind(lib, "lustro_prng_fill", i32, vp, U8P, SizeT)

    _bind(lib, "lustro_xof_new", vp, cp, SizeT)
    _bind(lib, "lustro_xof_free", None, vp)
    _bind(lib, "lustro_xof_fill", i32, vp, U8P, SizeT)

    _bind(lib, "lustro_prng_batch_new", vp, cp, U64P, U64P, SizeT)
    _bind(lib, "lustro_prng_batch_new_range", vp, cp, u64, u64, SizeT)
    _bind(lib, "lustro_prng_batch_free", None, vp)
    _bind(lib, "lustro_prng_batch_len", SizeT, vp)
    _bind(lib, "lustro_prng_batch_fill_blocks", i32, vp, U8P, SizeT, SizeT)
    _bind(lib, "lustro_prng_batch_import_snapshot", vp, U8P, SizeT)

    _bind(lib, "lustro_xof_batch_new", vp, ctypes.POINTER(U8P), ctypes.POINTER(SizeT), SizeT)
    _bind(lib, "lustro_xof_batch_free", None, vp)
    _bind(lib, "lustro_xof_batch_len", SizeT, vp)
    _bind(lib, "lustro_xof_batch_fill_blocks", i32, vp, U8P, SizeT, SizeT)
    _bind(lib, "lustro_xof_batch_import_snapshot", vp, U8P, SizeT)


def load_ffi():
    if not os.path.exists(DLL_PATH):
        sys.exit(f"ERROR: {DLL_PATH} not found")
    try:
        lib = ctypes.CDLL(DLL_PATH)
    except OSError as e:
        sys.exit(f"ERROR: cannot load {DLL_PATH}: {e}")
    try:
        bind_signatures(lib)
    except AttributeError as e:
        sys.exit(f"ERROR: {DLL_PATH} is missing an expected symbol ({e})")
    version = lib.lustro_api_version()
    if version != EXPECTED_API_VERSION:
        sys.exit(f"ERROR: unexpected API version {version} (expected {EXPECTED_API_VERSION})")
    return lib


# ================================================================
# SINGLE-CALL WRAPPERS
# ================================================================
def hash256(lib, msg: bytes) -> bytes:
    out = out_buf(32)
    err = lib.lustro_hash256(msg, len(msg), out)
    if err != 0:
        raise RuntimeError(f"lustro_hash256 returned error {err}")
    return bytes(out)


def hash128(lib, msg: bytes) -> bytes:
    out = out_buf(16)
    err = lib.lustro_hash128(msg, len(msg), out)
    if err != 0:
        raise RuntimeError(f"lustro_hash128 returned error {err}")
    return bytes(out)


def prng_fill(lib, seed: bytes, stream_id: int, fill_len: int) -> bytes:
    hi, lo = split_id(stream_id)
    ctx = lib.lustro_prng_new(seed, hi, lo)
    if not ctx:
        raise RuntimeError("lustro_prng_new returned NULL")
    try:
        out = out_buf(fill_len)
        err = lib.lustro_prng_fill(ctx, out, fill_len)
        if err != 0:
            raise RuntimeError(f"lustro_prng_fill returned error {err}")
        return bytes(out)
    finally:
        lib.lustro_prng_free(ctx)


def xof_fill(lib, message: bytes, fill_len: int) -> bytes:
    ctx = lib.lustro_xof_new(message, len(message))
    if not ctx:
        raise RuntimeError("lustro_xof_new returned NULL")
    try:
        out = out_buf(fill_len)
        err = lib.lustro_xof_fill(ctx, out, fill_len)
        if err != 0:
            raise RuntimeError(f"lustro_xof_fill returned error {err}")
        return bytes(out)
    finally:
        lib.lustro_xof_free(ctx)


# ================================================================
# BATCH WRAPPERS
# ================================================================
def prng_batch_fill(lib, ctx, n: int, steps: int) -> bytes:
    size = n * steps * 32
    out = out_buf(size)
    err = lib.lustro_prng_batch_fill_blocks(ctx, out, size, steps)
    if err != 0:
        raise RuntimeError(f"lustro_prng_batch_fill_blocks returned error {err}")
    return bytes(out)


def xof_batch_fill(lib, ctx, n: int, steps: int) -> bytes:
    size = n * steps * 32
    out = out_buf(size)
    err = lib.lustro_xof_batch_fill_blocks(ctx, out, size, steps)
    if err != 0:
        raise RuntimeError(f"lustro_xof_batch_fill_blocks returned error {err}")
    return bytes(out)


def prng_batch_from_snapshot(lib, snap_hex: str):
    data = bytes.fromhex(snap_hex)
    return lib.lustro_prng_batch_import_snapshot(u8_buf(data), len(data))


def xof_batch_from_snapshot(lib, snap_hex: str):
    data = bytes.fromhex(snap_hex)
    return lib.lustro_xof_batch_import_snapshot(u8_buf(data), len(data))


def xof_batch_new(lib, messages):
    ptrs, lens, bufs = message_arrays(messages)
    ctx = lib.lustro_xof_batch_new(ptrs, lens, len(messages))
    return ctx


# ================================================================
# SINGLE-STREAM VECTORS
# ================================================================
def validate_single(lib):
    digest = hashlib.sha256()

    print("\n== C FFI: HASH VECTORS ==")
    for label, msg, exp256, exp128 in HASH_VECTORS:
        got256 = hash256(lib, msg)
        got128 = hash128(lib, msg)
        digest.update(got256)
        digest.update(got128)
        check(f"hash256 [{label}]", got256.hex() == exp256,
              f"expected={exp256} got={got256.hex()}")
        check(f"hash128 [{label}]", got128.hex() == exp128,
              f"expected={exp128} got={got128.hex()}")

    print("\n== C FFI: PRNG VECTORS ==")
    for label, seed, stream_id, fill_len, exp in PRNG_VECTORS:
        got = prng_fill(lib, seed, stream_id, fill_len)
        digest.update(got)
        check(f"prng [{label}]", got.hex() == exp, f"expected={exp} got={got.hex()}")

    print("\n== C FFI: XOF VECTORS ==")
    for label, message, fill_len, exp in XOF_VECTORS:
        got = xof_fill(lib, message, fill_len)
        digest.update(got)
        check(f"xof [{label}]", got.hex() == exp, f"expected={exp} got={got.hex()}")

    fingerprint = digest.hexdigest()
    check("fingerprint", fingerprint == GOLDEN_FINGERPRINT,
          f"expected={GOLDEN_FINGERPRINT} got={fingerprint}")


# ================================================================
# BATCH VECTORS
# ================================================================
def validate_prng_batch(lib):
    print("\n== C FFI: PRNG BATCH ==")
    digests = []

    for label, seed, first_id, n, steps, exp in PRNG_BATCH_RANGE_VECTORS:
        hi, lo = split_id(first_id)

        ctx = lib.lustro_prng_batch_new_range(seed, hi, lo, n)
        if not check(f"prng batch [{label}] new_range", bool(ctx)):
            digests.append("")
            continue
        try:
            check(f"prng batch [{label}] len", lib.lustro_prng_batch_len(ctx) == n)
            got = sha(prng_batch_fill(lib, ctx, n, steps))
        finally:
            lib.lustro_prng_batch_free(ctx)
        digests.append(got)
        check(f"prng batch [{label}]", got == exp, f"expected={exp} got={got}")

        # Consecutive calls must concatenate: 1 step, then the rest.
        if steps >= 2:
            ctx = lib.lustro_prng_batch_new_range(seed, hi, lo, n)
            try:
                data = prng_batch_fill(lib, ctx, n, 1) + prng_batch_fill(lib, ctx, n, steps - 1)
            finally:
                lib.lustro_prng_batch_free(ctx)
            check(f"prng batch [{label}] split 1+{steps - 1}", sha(data) == exp)

    for label, seed, ids, steps, exp in PRNG_BATCH_IDS_VECTORS:
        n = len(ids)
        his = (ctypes.c_uint64 * n)(*[split_id(i)[0] for i in ids])
        los = (ctypes.c_uint64 * n)(*[split_id(i)[1] for i in ids])
        ctx = lib.lustro_prng_batch_new(seed, his, los, n)
        if not check(f"prng batch [{label}] new", bool(ctx)):
            digests.append("")
            continue
        try:
            got = sha(prng_batch_fill(lib, ctx, n, steps))
        finally:
            lib.lustro_prng_batch_free(ctx)
        digests.append(got)
        check(f"prng batch [{label}]", got == exp, f"expected={exp} got={got}")

    for label, snap_hex, n, steps, exp in PRNG_BATCH_MIXED_VECTORS:
        ctx = prng_batch_from_snapshot(lib, snap_hex)
        if not check(f"prng batch [{label}] import_snapshot", bool(ctx)):
            digests.append("")
            continue
        try:
            check(f"prng batch [{label}] len", lib.lustro_prng_batch_len(ctx) == n)
            got = sha(prng_batch_fill(lib, ctx, n, steps))
        finally:
            lib.lustro_prng_batch_free(ctx)
        digests.append(got)
        check(f"prng batch [{label}]", got == exp, f"expected={exp} got={got}")

    return digests


def validate_xof_batch(lib):
    print("\n== C FFI: XOF BATCH ==")
    digests = []

    for label, n, steps, exp in XOF_BATCH_VECTORS:
        messages = [_xof_lane_message(i) for i in range(n)]
        ctx = xof_batch_new(lib, messages)
        if not check(f"xof batch [{label}] new", bool(ctx)):
            digests.append("")
            continue
        try:
            check(f"xof batch [{label}] len", lib.lustro_xof_batch_len(ctx) == n)
            got = sha(xof_batch_fill(lib, ctx, n, steps))
        finally:
            lib.lustro_xof_batch_free(ctx)
        digests.append(got)
        check(f"xof batch [{label}]", got == exp, f"expected={exp} got={got}")

        if steps >= 2:
            ctx = xof_batch_new(lib, messages)
            try:
                data = xof_batch_fill(lib, ctx, n, 1) + xof_batch_fill(lib, ctx, n, steps - 1)
            finally:
                lib.lustro_xof_batch_free(ctx)
            check(f"xof batch [{label}] split 1+{steps - 1}", sha(data) == exp)

    for label, snap_hex, n, steps, exp in XOF_BATCH_MIXED_VECTORS:
        ctx = xof_batch_from_snapshot(lib, snap_hex)
        if not check(f"xof batch [{label}] import_snapshot", bool(ctx)):
            digests.append("")
            continue
        try:
            check(f"xof batch [{label}] len", lib.lustro_xof_batch_len(ctx) == n)
            got = sha(xof_batch_fill(lib, ctx, n, steps))
        finally:
            lib.lustro_xof_batch_free(ctx)
        digests.append(got)
        check(f"xof batch [{label}]", got == exp, f"expected={exp} got={got}")

    return digests


def validate_hash_batch(lib):
    print("\n== C FFI: HASH BATCH ==")
    digests = []

    for label, n, length, exp256, exp128 in HASH_BATCH_VECTORS:
        rows = [_hash_row(i, length) for i in range(n)]
        in_buf = u8_buf(b"".join(rows))

        out256 = out_buf(n * 32)
        out128 = out_buf(n * 16)
        err256 = lib.lustro_hash256_many(in_buf, n, length, out256)
        err128 = lib.lustro_hash128_many(in_buf, n, length, out128)
        if not (check(f"hash batch [{label}] hash256_many returns Ok", err256 == 0, f"error {err256}")
                and check(f"hash batch [{label}] hash128_many returns Ok", err128 == 0, f"error {err128}")):
            digests += ["", ""]
            continue

        got256, got128 = sha(bytes(out256)), sha(bytes(out128))
        digests += [got256, got128]
        check(f"hash batch [{label}] hash256", got256 == exp256,
              f"expected={exp256} got={got256}")
        check(f"hash batch [{label}] hash128", got128 == exp128,
              f"expected={exp128} got={got128}")

        # The pointer-array variant must agree with the flat-buffer one.
        ptrs, lens, _bufs = message_arrays(rows)
        var256 = out_buf(n * 32)
        var128 = out_buf(n * 16)
        e1 = lib.lustro_hash256_many_var(ptrs, n, lens, var256)
        e2 = lib.lustro_hash128_many_var(ptrs, n, lens, var128)
        check(f"hash batch [{label}] many_var == many",
              e1 == 0 and e2 == 0 and bytes(var256) == bytes(out256) and bytes(var128) == bytes(out128))

    return digests


def validate_hash_var_lengths(lib):
    # Variable lengths (including empty) through *_many_var, against single calls.
    print("\n== C FFI: HASH BATCH, VARIABLE LENGTHS ==")
    n = 300
    messages = [_xof_lane_message(i) for i in range(n)]
    ptrs, lens, _bufs = message_arrays(messages)

    out256 = out_buf(n * 32)
    out128 = out_buf(n * 16)
    e1 = lib.lustro_hash256_many_var(ptrs, n, lens, out256)
    e2 = lib.lustro_hash128_many_var(ptrs, n, lens, out128)
    check("hash256_many_var returns Ok", e1 == 0, f"error {e1}")
    check("hash128_many_var returns Ok", e2 == 0, f"error {e2}")

    bad256 = [i for i, m in enumerate(messages)
              if bytes(out256[i * 32:(i + 1) * 32]) != hash256(lib, m)]
    bad128 = [i for i, m in enumerate(messages)
              if bytes(out128[i * 16:(i + 1) * 16]) != hash128(lib, m)]
    check(f"hash256_many_var == hash256 for all {n} lengths", not bad256,
          f"first mismatch at {bad256[:1]}")
    check(f"hash128_many_var == hash128 for all {n} lengths", not bad128,
          f"first mismatch at {bad128[:1]}")


def main():
    if "BATCH_FINGERPRINT" not in globals():
        sys.exit("ERROR: paste the generator output into the GOLDEN VECTORS section first.")

    width = 72
    print("=" * width)
    print(f"{'LUSTRO V1 — GOLDEN VECTOR VALIDATOR (C FFI)':^{width}}")
    print(f"{'directory: ' + SCRIPT_DIR:^{width}}")
    print("=" * width)

    lib = load_ffi()
    print(f"\nDLL: {DLL_PATH}")
    print(f"API version: {lib.lustro_api_version()}")

    validate_single(lib)

    batch_digests = []
    batch_digests += validate_prng_batch(lib)
    batch_digests += validate_xof_batch(lib)
    batch_digests += validate_hash_batch(lib)
    validate_hash_var_lengths(lib)

    print("\n== C FFI: BATCH FINGERPRINT ==")
    batch_fingerprint = sha(b"".join(bytes.fromhex(d) for d in batch_digests))
    check("batch fingerprint", batch_fingerprint == BATCH_FINGERPRINT,
          f"expected={BATCH_FINGERPRINT} got={batch_fingerprint}")

    print("\n" + "=" * width)
    if FAILURES:
        print(f"RESULT: {len(FAILURES)} test(s) FAILED:")
        for f in FAILURES:
            print(f"  - {f}")
        sys.exit(1)
    print("RESULT: All tests passed. Golden vectors match (C FFI).")
    print("=" * width)


if __name__ == "__main__":
    main()