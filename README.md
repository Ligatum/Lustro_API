**Please note the API is currently in its beta stage. I'm still working on a list of functionality updates.**

# Lustro API

Questions? Please see the **[FAQ](https://github.com/Ligatum/Lustro/blob/MAIN/DOCS/FAQ.md)**.

## Clone

```bash
git clone https://github.com/Ligatum/Lustro_API.git
cd Lustro_API
```

## Build

```bash
# Rust

cargo build --release

# System entropy for Seed256::from_entropy() (Rust)

cargo build --release --features entropy

# Python (requires NumPy)

pip install maturin numpy
maturin develop --release

# C/C++ FFI bindings

cargo build --release --features ffi

```

All release builds catch internal panics and convert them to `LustroError::InternalPanic` (or the corresponding Python exception) at the boundary instead of just aborting the process. See ARCHITECTURE.md §13.

## Speed Test

This repo is the full API implementation and also contains **Lustro_Speed_Tester.py**. This script requires the C FFI library (`lustro.dll`) in the same folder.

Please note that the core mechanism and the API implementation are considered non-cryptographic at this moment.

---

## Quick Start

### Hash

**Rust**

```rust
use lustro::hash256;

let digest = hash256(b"hello world");
```

**Python**

```python
import lustro

digest = lustro.hash256(b"hello world")
```

**C / C++**

```c
#include "lustro.h"

uint8_t digest[LUSTRO_HASH256_LEN];
lustro_hash256((const uint8_t *)"hello world", 11, digest);
```

`hash128` is available in the same way and returns `LUSTRO_HASH128_LEN` (16) bytes.

**Printing and comparing digests (Rust)**

`Hash128` and `Hash256` print as hex of the digest bytes, in the order returned by `as_bytes()`: 32 and 64 digits. Width, fill and alignment work as for strings (`{digest:>70}`, `{digest:*^70}`), and a precision shortens the output (`{digest:.8}`).

```rust
use lustro::hash256;

let digest = hash256(b"hello world");

println!("{digest}");     // lowercase hex, 64 digits
println!("{digest:X}");   // uppercase
println!("{digest:#x}");  // with a 0x prefix
```

Digests are `Hash` and `Ord`, so they work as keys in `HashMap` and `BTreeMap`. `Ord` compares the bytes lexicographically, not as a number. Conversion to and from `[u8; N]` is available through `From` / `Into`. In Python a digest is `bytes`, so use `digest.hex()`.

---

### Batch Hash

Hash many messages in one call.

**Rust**

```rust
use lustro::hash256_many;

let messages = [b"a".as_ref(), b"bc".as_ref(), b"".as_ref()];
let digests = hash256_many(&messages);
```

**Python**

```python
import numpy as np
import lustro

# Equal-length messages: one message per row of a 2D uint8 array.
rows = np.zeros((3, 16), dtype=np.uint8)
digests = lustro.hash256_many(rows)  # uint8 array, shape (3, 32)

# Messages of any length: a sequence of bytes or byte buffers.
digests = lustro.hash256_many_var([b"a", b"bc", b""])  # shape (3, 32)
```

**C / C++**

```c
#include "lustro.h"

/* Equal-length messages, stored back to back. */
uint8_t rows[3][16] = {{0}};
uint8_t digests[3][LUSTRO_HASH256_LEN];
lustro_hash256_many((const uint8_t *)rows, 3, 16, (uint8_t *)digests);

/* Messages of any length. */
const uint8_t *messages[] = {
    (const uint8_t *)"a",
    (const uint8_t *)"bc",
    (const uint8_t *)""
};
uintptr_t lengths[] = {1, 2, 0};

lustro_hash256_many_var(messages, 3, lengths, (uint8_t *)digests);
```

Naming differs between the Rust API and the C/Python APIs. In Rust, `hash256_many` takes messages of any length. In C and Python, `*_many` expects messages of equal length, and the any-length variant is `*_many_var`. `hash128_many` and `hash128_many_var` work the same way and return 16-byte digests.

---

### PRNG

**Rust**

```rust
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let mut rng = LustroPrng::new(&seed, StreamId(0));

let value = rng.next_u64();
```

**Python**

```python
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)  # stream_id defaults to 0
value = rng.next_u64()
```

**C / C++**

```c
#include <stdio.h>
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};

LustroPrng *rng = NULL;
LustroError err = lustro_prng_new(seed, 0, 0, &rng);
if (err != LUSTRO_ERROR_OK) {
    fprintf(stderr, "%s\n", lustro_strerror(err));
    return 1;
}

uint64_t value;
lustro_prng_next_u64(rng, &value);

lustro_prng_free(rng);
```

Every constructor returns a `LustroError` and stores the new context in its last argument (`out_ctx`).
On failure `*out_ctx` is set to `NULL` and the status tells why (`lustro_strerror` returns a message). The `lustro_*_free` functions accept `NULL`.
The C examples below omit the status checks for brevity.

### Random Seed

**Rust** (`--features entropy`, also enabled by `rand`)

```rust
use lustro::prng::LustroPrng;
use lustro::types::{Seed256, StreamId};

// Err(EntropyError) if the operating system cannot provide entropy.
let seed = Seed256::from_entropy()?;
let mut rng = LustroPrng::new(&seed, StreamId(0));
```

**Python**

```python
import os
from lustro import LustroPrng

rng = LustroPrng(os.urandom(32), 0)
```

**C / C++** — fill the 32-byte seed from the system's entropy source
(for example `getrandom(2)`, `arc4random_buf` or `BCryptGenRandom`).

A seed from the operating system does not make the generator cryptographically
secure: `LustroPrng` does not implement `CryptoRng`.

---

### XOF

**Rust**

```rust
use lustro::LustroXof;

let mut xof = LustroXof::new(b"hello world");
let block = xof.next_block();
```

**Python**

```python
from lustro import LustroXof

xof = LustroXof(b"hello world")
block = xof.next_block()
```

**C / C++**

```c
#include "lustro.h"

LustroXof *xof = NULL;
lustro_xof_new((const uint8_t *)"hello world", 11, &xof);

uint8_t block[LUSTRO_BLOCK_LEN];
lustro_xof_next_block(xof, block);

lustro_xof_free(xof);
```

---

### Typed Reads

Read integers, a full block, or any number of bytes from a single stream. `LustroXof` has the same methods.

**Rust**

```rust
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let mut rng = LustroPrng::new(&seed, StreamId(0));

let a = rng.next_u32();
let b = rng.next_u64();
let c = rng.next_u128();
let block = rng.next_block();   // [u8; 32]

let mut buf = [0u8; 100];
rng.fill_bytes(&mut buf);
```

**Python**

```python
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)

a = rng.next_u32()
b = rng.next_u64()
c = rng.next_u128()
block = rng.next_block()        # bytes, 32
data = rng.fill(100)            # bytes, 100
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrng *rng = NULL;
lustro_prng_new(seed, 0, 0, &rng);

uint32_t a;
uint64_t b;
lustro_prng_next_u32(rng, &a);
lustro_prng_next_u64(rng, &b);

uint8_t c[16];                      /* u128, 16 bytes little-endian */
lustro_prng_next_u128(rng, c);

uint8_t block[LUSTRO_BLOCK_LEN];
lustro_prng_next_block(rng, block);

uint8_t data[100];
lustro_prng_fill(rng, data, sizeof(data));

lustro_prng_free(rng);
```

All values are little-endian and taken from the same byte stream: in the examples above `a` is bytes 0–3, `b` bytes 4–11 and `c` bytes 12–27. `next_block()` always advances to a full new 32-byte block and discards the unread rest of the current one. The XOF equivalents are `lustro_xof_next_u32` and so on (C/C++).

---

### Python Notes

Single streams (`LustroPrng`, `LustroXof`) can also return or fill larger amounts of data, and can be copied:

```python
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)

data = rng.fill(1000)         # bytes
buf = bytearray(1000)
rng.fill_into(buf)            # bytearray, writable memoryview or NumPy uint8 array
words = rng.fill_u64(1000)    # NumPy uint64 array of shape (1000,)

twin = rng.copy()             # same state; copy.copy() and copy.deepcopy() work too
```

`fill_u64(n)` needs NumPy and returns a new, writable array with the values that `n` calls of `next_u64()` would give.

Seeds, messages, hash input and snapshots are given as `bytes`. `bytearray`, `memoryview`, `array('B')` and NumPy `uint8` arrays are accepted too; they must be C-contiguous with byte items (`TypeError` otherwise) and are copied. `hash256_many_var`, `hash128_many_var` and `LustroXofBatch` take any sequence of such objects, but not a single string or buffer.

An instance must not be used by several threads at the same time (an overlapping call raises `RuntimeError`). Use `copy()`, `fork()` or separate instances instead. The installed version is available as `lustro.__version__`, and the package ships type stubs (`lustro.pyi`, `py.typed`).

---

### Derive Path

Derive a stream directly from seed and path of identifiers, without
exposing the intermediate streams.

**Rust**

```rust
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let rng = LustroPrng::derive_path(&seed, &[StreamId(12), StreamId(7), StreamId(99)]);
```

**Python**

```python
from lustro import LustroPrng

seed = bytes(32)
rng = LustroPrng.derive_path(seed, [12, 7, 99])
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};

uint64_t ids_hi[3] = {0, 0, 0};
uint64_t ids_lo[3] = {12, 7, 99};

LustroPrng *rng = NULL;
lustro_prng_derive_path(seed, ids_hi, ids_lo, 3, &rng);

lustro_prng_free(rng);
```

`derive_path(seed, path)` gives the same stream as `new` with the first
identifier followed by `fork` for each remaining one, in a single call. The same
API is available for XOF, via `LustroXof::derive_path(message, path)` (Rust),
`LustroXof.derive_path(message, path)` (Python), and `lustro_xof_derive_path(message, message_len, ids_hi, ids_lo, n, &out_ctx)`
(C/C++); there every identifier in `path` is a `fork`, since `XOF::new` already
takes the message as its root.

---

### Batch — PRNG and XOF

Lustro can process multiple independent streams.

**Rust**

```rust
use lustro::{LustroPrngBatch, LustroXofBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);

let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let mut prng = LustroPrngBatch::new(&seed, &ids);

let steps = 2;
let mut prng_out = vec![[0u8; 32]; prng.len() * steps];
prng.fill_blocks(&mut prng_out, steps);

let messages = [
    b"msg0".as_ref(),
    b"msg1".as_ref(),
    b"msg2".as_ref(),
];

let mut xof = LustroXofBatch::new(&messages);
let mut xof_out = vec![[0u8; 32]; xof.len() * steps];
xof.fill_blocks(&mut xof_out, steps);

// Or let the library allocate the vector:
let more = prng.blocks(steps); // Vec<[u8; 32]>, prng.len() * steps blocks
```

**Python**

```python
import numpy as np
from lustro import LustroPrngBatch, LustroXofBatch

seed = bytes(32)

steps = 2

prng = LustroPrngBatch(seed, [0, 1, 2, 3])
prng_out = np.empty((steps, len(prng), 4), dtype=np.uint64)
prng.fill_blocks(prng_out, steps)

xof = LustroXofBatch([b"msg0", b"msg1", b"msg2"])
xof_out = np.empty((steps, len(xof), 4), dtype=np.uint64)
xof.fill_blocks(xof_out, steps)

# Or let the library allocate the array:
prng_out = prng.blocks(steps)
```

`fill_blocks(out, steps)` writes one 32-byte block per stream per step, step-major: stream `i` at step `s` is block `s * n + i` (`n` = number of streams). In Python that is `out[s, i]`, with `out` shaped `(steps, n_streams, 4)`; each block is four `uint64` values. `blocks(steps)` (Rust and Python) returns the same blocks in a newly allocated result: a `Vec<[u8; 32]>` in Rust, a new, writable array of that shape in Python.

**C / C++**

```c
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};

uint64_t ids_hi[4] = {0};
uint64_t ids_lo[4] = {0, 1, 2, 3};

LustroPrngBatch *prng = NULL;
lustro_prng_batch_new(seed, ids_hi, ids_lo, 4, &prng);

#define STEPS 2

uint8_t prng_out[STEPS][4][LUSTRO_BLOCK_LEN];
lustro_prng_batch_fill_blocks(
    prng, (uint8_t *)prng_out, sizeof(prng_out), STEPS
);

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};

uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof = NULL;
lustro_xof_batch_new(messages, lengths, 3, &xof);

uint8_t xof_out[STEPS][3][LUSTRO_BLOCK_LEN];
lustro_xof_batch_fill_blocks(
    xof, (uint8_t *)xof_out, sizeof(xof_out), STEPS
);

lustro_xof_batch_free(xof);
lustro_prng_batch_free(prng);
```

---

### Batch — Suggested Steps

`suggested_steps()` returns a recommended `steps` for the current number of streams. It is a speed hint only: it does not change the output or the state of the batch, and the bytes of each stream do not depend on how the steps are split across calls.

**Rust**

```rust
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let mut prng = LustroPrngBatch::new(&seed, &ids);

let steps = prng.suggested_steps();
let mut out = vec![[0u8; 32]; prng.len() * steps];
prng.fill_blocks(&mut out, steps);
```

**Python**

```python
import numpy as np
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])

steps = prng.suggested_steps()
out = np.empty((steps, len(prng), 4), dtype=np.uint64)
prng.fill_blocks(out, steps)
```

**C / C++**

```c
#include <stdlib.h>
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrngBatch *prng = NULL;
lustro_prng_batch_new_range(seed, 0, 0, 4, &prng);

size_t steps = lustro_prng_batch_suggested_steps(prng);
size_t out_len = steps * lustro_prng_batch_len(prng) * LUSTRO_BLOCK_LEN;
uint8_t *out = malloc(out_len);

lustro_prng_batch_fill_blocks(prng, out, out_len, steps);

free(out);
lustro_prng_batch_free(prng);
```

The same API is available for XOF batches. In Python, a batch keeps an internal buffer the size of its largest `fill_blocks()` call; `release_buffer()` frees it earlier. `blocks()` does not use that buffer.

---

### Batch — Single Lane

Take one lane out of a batch as a single stream. The stream continues the lane: its next block is the block the batch would write for that lane next. The batch is not changed, and the two evolve independently afterwards.

**Rust**

```rust
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let mut prng = LustroPrngBatch::new(&seed, &ids);

prng.blocks(2);

// None if the index is out of range.
let mut lane = prng.lane(1).expect("lane in range");
let block = lane.next_block();
```

**Python**

```python
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])
prng.blocks(2)

lane = prng.lane(1)  # IndexError if the index is out of range
block = lane.next_block()
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrngBatch *prng = NULL;
lustro_prng_batch_new_range(seed, 0, 0, 4, &prng);

LustroPrng *lane = NULL;
LustroError err = lustro_prng_batch_lane(prng, 1, &lane);  /* LUSTRO_ERROR_INVALID_ARGUMENT if index >= len */

uint8_t block[LUSTRO_BLOCK_LEN];
lustro_prng_next_block(lane, block);

lustro_prng_free(lane);  /* a single stream, not a batch */
lustro_prng_batch_free(prng);
```

`LustroXofBatch` has the same `lane` and returns a `LustroXof`. The result is an ordinary single stream, so it supports typed reads, `fork` and snapshots. `lane(i).fork(id)` equals child `i` of `fork` called with `id` at position `i`. An out-of-range index gives `None` (Rust), `IndexError` (Python) or `LUSTRO_ERROR_INVALID_ARGUMENT` (C/C++).

---

### Batch Fork

Create multiple child streams.

**Rust**

```rust
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];

let prng = LustroPrngBatch::new(&seed, &ids);
let children = prng.fork_range(StreamId(100));
let many_children = prng.fork_many(&[StreamId(100), StreamId(200)]);
```

**Python**

```python
from lustro import LustroPrngBatch

seed = bytes(32)
prng = LustroPrngBatch(seed, [0, 1, 2, 3])

children = prng.fork_range(100)
many_children = prng.fork_many([100, 200])
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrngBatch *prng = NULL;
lustro_prng_batch_new_range(seed, 0, 0, 4, &prng);

LustroPrngBatch *children = NULL;
lustro_prng_batch_fork_range(prng, 0, 100, &children);

uint64_t k_hi[2] = {0, 0};
uint64_t k_lo[2] = {100, 200};

LustroPrngBatch *many_children = NULL;
lustro_prng_batch_fork_many(prng, k_hi, k_lo, 2, &many_children);

lustro_prng_batch_free(many_children);
lustro_prng_batch_free(children);
lustro_prng_batch_free(prng);
```

`fork_range(first)` derives `batch.len()` children using sequential stream IDs.
`fork_many(ids)` derives `ids.len()` children per lane instead, producing a
batch of `batch.len() * ids.len()` lanes, ordered parent-major: child `j` of
lane `i` is at index `i * ids.len() + j`. Each child starts at step 0.

The same batch fork API is available for XOF.

---

### Batch Derive Path

Derive a batch directly from a seed and a shared path, one lane
per root.

**Rust**

```rust
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let roots = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let path = [StreamId(12), StreamId(7)];

let batch = LustroPrngBatch::derive_path(&seed, &roots, &path);
```

**Python**

```python
from lustro import LustroPrngBatch

seed = bytes(32)
batch = LustroPrngBatch.derive_path(seed, [0, 1, 2, 3], [12, 7])
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};

uint64_t roots_hi[4] = {0, 0, 0, 0};
uint64_t roots_lo[4] = {0, 1, 2, 3};

uint64_t path_hi[2] = {0, 0};
uint64_t path_lo[2] = {12, 7};

LustroPrngBatch *batch = NULL;
lustro_prng_batch_derive_path(
    seed, roots_hi, roots_lo, 4, path_hi, path_lo, 2, &batch
);

lustro_prng_batch_free(batch);
```

`derive_path(seed, roots, path)` derives a canonical batch: lane `i` equals
`LustroPrng::derive_path(seed, [roots[i]] + path)`, independent of any other
stream's state. The same API is available for XOF, via
`LustroXofBatch::derive_path(messages, path)` (Rust),
`LustroXofBatch.derive_path(messages, path)` (Python), and
`lustro_xof_batch_derive_path(message_ptrs, message_lens, n_messages, path_hi, path_lo, n_path, &out_ctx)`
(C/C++), using a list of messages instead of a seed and root identifiers. In Python the messages can be any sequence of `bytes` or byte buffers.

---

### Snapshot

Save and restore the exact stream position.

**Rust**

```rust
use lustro::types::LustroPrngSnapshot;
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let mut rng = LustroPrng::new(&seed, StreamId(0));

let snapshot = rng.export_snapshot();
let bytes = snapshot.to_le_bytes();

let snapshot =
    LustroPrngSnapshot::from_le_bytes(&bytes).expect("valid snapshot");

let mut restored = LustroPrng::import_snapshot(snapshot);
```

**Python**

```python
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)

snapshot = rng.export_snapshot()
restored = LustroPrng.import_snapshot(snapshot)
```

**C / C++**

```c
#include <stdio.h>
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrng *rng = NULL;
lustro_prng_new(seed, 0, 0, &rng);

uint8_t snapshot[LUSTRO_SNAPSHOT_LEN];
lustro_prng_export_snapshot(rng, snapshot);

LustroPrng *restored = NULL;
LustroError err = lustro_prng_import_snapshot(snapshot, &restored);
if (err != LUSTRO_ERROR_OK) {
    fprintf(stderr, "%s\n", lustro_strerror(err));
}

lustro_prng_free(restored);
lustro_prng_free(rng);
```

The same snapshot API is available for XOF and batch contexts. Please note that for batch contexts in C/C++ the snapshot size is dynamic;
call the corresponding `*_batch_snapshot_size(ctx)` before export. It equals `LUSTRO_BATCH_SNAPSHOT_HEADER_LEN + n * LUSTRO_BATCH_SNAPSHOT_LANE_LEN`
for `n` streams.

---

### Runtime Settings

Thread count and SIMD backend follow `LUSTRO_NUM_THREADS`, `LUSTRO_DISABLE_MT` and `LUSTRO_DISABLE_SIMD` by default. `configure` sets them from code and must be called before the first batch call.

**Rust**

```rust
use lustro::{configure, RuntimeOptions};
use std::num::NonZeroUsize;

let options = RuntimeOptions::new()
    .threads(NonZeroUsize::new(4).unwrap())
    .allow_simd(true);

configure(options)?;
```

**Python**

```python
import lustro

lustro.configure(threads=4, allow_simd=True)
```

**C / C++**

```c
#include <stdio.h>
#include "lustro.h"

LustroError err = lustro_configure(4, LUSTRO_SIMD_AUTO);
if (err != LUSTRO_ERROR_OK) {
    fprintf(stderr, "%s\n", lustro_strerror(err));
}
```

Arguments you omit (`None` in Python, `0` and `LUSTRO_SIMD_KEEP` in C/C++) still follow the environment; the ones you pass override it. `threads` is capped at the number of logical CPUs, and `1` disables the thread pool. `allow_simd=False` (`LUSTRO_SIMD_SCALAR` in C/C++) selects the scalar backend.

The settings can be set only once. The first batch call, `suggested_steps()` or `runtime_info()` fixes them too, so a later `configure` fails, even with the same arguments: `ConfigError::AlreadyConfigured` (Rust), `RuntimeError` (Python), `LUSTRO_ERROR_ALREADY_CONFIGURED` (C/C++). Bad arguments fail without changing anything: `ValueError` for `threads < 1` (Python), `LUSTRO_ERROR_INVALID_ARGUMENT` for an unknown `simd` (C/C++). `runtime_info()` shows what ended up in effect.

---

### Size Constants

| Meaning | Bytes | C / C++ (`lustro.h`) | Rust (`lustro::types`) | Python (`lustro`) |
|---|---|---|---|---|
| Seed | 32 | `LUSTRO_SEED_LEN` | `LUSTRO_SEED_LEN` | `SEED_LEN` |
| Output block | 32 | `LUSTRO_BLOCK_LEN` | `LUSTRO_BLOCK_LEN` | `BLOCK_LEN` |
| `hash128` digest | 16 | `LUSTRO_HASH128_LEN` | `LUSTRO_HASH128_LEN` | `HASH128_LEN` |
| `hash256` digest | 32 | `LUSTRO_HASH256_LEN` | `LUSTRO_HASH256_LEN` | `HASH256_LEN` |
| Single snapshot | 56 | `LUSTRO_SNAPSHOT_LEN` | `LUSTRO_SNAPSHOT_LEN` | `SNAPSHOT_LEN` |
| Batch snapshot header | 16 | `LUSTRO_BATCH_SNAPSHOT_HEADER_LEN` | `LUSTRO_BATCH_SNAPSHOT_HEADER_LEN` | `BATCH_SNAPSHOT_HEADER_LEN` |
| Batch snapshot, per stream | 48 | `LUSTRO_BATCH_SNAPSHOT_LANE_LEN` | `LUSTRO_BATCH_SNAPSHOT_LANE_LEN` | `BATCH_SNAPSHOT_LANE_LEN` |

For full API details please see **[ARCHITECTURE.md](ARCHITECTURE.md)**.