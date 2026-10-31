**Please note the API is currently in its beta stage. I'm still working on a list of functionality updates.**

**Edit: 27.10.26. I will be uploading a breaking change patch in the coming week or two at most. It'll be the only one before going out of beta. Lustro 1.0.1 will be the initial post-beta release.**

# Lustro API

Questions? Please see the **[FAQ](https://github.com/Ligatum/Lustro/blob/MAIN/DOCS/FAQ.md)**.

## Get the Code

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

# rand_core traits for LustroPrng and LustroXof (Rust, also enables entropy)

cargo build --release --features rand

# Python 3.11-3.14 (requires NumPy)

pip install maturin numpy
maturin develop --release

# C/C++ FFI bindings

cargo build --release --features ffi

```

In the release profiles (`panic = "unwind"`) the C FFI boundary catches internal panics and returns `LustroError::InternalPanic`; the Python module raises them as `pyo3_runtime.PanicException` (a `BaseException`). Direct Rust calls panic as usual, for example on a length mismatch.

The Python module declares that it requires the GIL (`gil_used = true`).
On free-threaded CPython builds, importing the module enables the GIL
unless this behavior is explicitly overridden (`PYTHON_GIL=0` or `-X gil=0`).
Running Lustro with the GIL disabled is not supported.

## Speed Test

This repo is the full API implementation and also contains **Lustro_Speed_Tester.py**. It runs the equivalent sections on native Rust and FFI layers. The results can be compared.

`````bash
# rust target (built on request only, no other build includes it)
cargo build --release --example speed
`````

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

### PRNG and XOF

`LustroPrng` generates a stream from a 32-byte seed and a stream ID. `LustroXof` generates a stream from a message of any length.

**Rust**

```rust
// PRNG
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let mut rng = LustroPrng::new(&seed, StreamId(0));

let value = rng.next_u64();
```

```rust
// XOF
use lustro::LustroXof;

let mut xof = LustroXof::new(b"hello world");

let value = xof.next_u64();
```

**Python**

```python
# PRNG
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)  # stream_id defaults to 0
value = rng.next_u64()
```

```python
# XOF
from lustro import LustroXof

xof = LustroXof(b"hello world")
value = xof.next_u64()
```

**C / C++**

```c
/* PRNG */
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

```c
/* XOF */
#include "lustro.h"

LustroXof *xof = NULL;
lustro_xof_new((const uint8_t *)"hello world", 11, &xof);

uint64_t value;
lustro_xof_next_u64(xof, &value);

lustro_xof_free(xof);
```

Every constructor returns a `LustroError` and stores the new context in its last argument (`out_ctx`).
On failure `*out_ctx` is set to `NULL` and the status tells why (`lustro_strerror` returns a message). The `lustro_*_free` functions accept `NULL`.
The C examples below omit the status checks for brevity.

### Random Seed

**Rust** (`--features entropy`, also enabled by `rand`)

```rust
use lustro::{LustroPrng, Seed256, StreamId};

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

### Typed Reads

Read integers, a full block, or any number of bytes from a single stream.

**Rust**

```rust
use lustro::{LustroPrng, Seed256, StreamId};
use std::io::Read;

let seed = Seed256::from_bytes([0u8; 32]);
let mut rng = LustroPrng::new(&seed, StreamId(0));

let a = rng.next_u32();
let b = rng.next_u64();
let c = rng.next_u128();
let block = rng.next_block();   // [u8; 32]

let mut buf = [0u8; 100];
rng.fill_bytes(&mut buf);

let mut bytes = Vec::new();
rng.by_ref().take(100).read_to_end(&mut bytes).unwrap();   // std::io::Read
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

`LustroXof` reads the same way: use a `LustroXof` in place of `rng` (Rust and Python), or the `lustro_xof_*` functions in C/C++ (`lustro_xof_next_u32`, `lustro_xof_next_u64`, `lustro_xof_next_u128`, `lustro_xof_next_block`, `lustro_xof_fill`).

All values are little-endian and taken from the same byte stream: in the examples above `a` is bytes 0–3, `b` bytes 4–11 and `c` bytes 12–27. `next_block()` always advances to a full new 32-byte block and discards the unread rest of the current one.

In Rust, `LustroPrng` and `LustroXof` also implement `std::io::Read`. `read` fills the whole buffer and returns its length, exactly like `fill_bytes`. The stream has no end, so do not use an unbounded `read_to_end`; bound the read, for example with `take`.

---

### Clone

Copy a stream with its exact state. The copy is independent: both continue with the same bytes, and neither affects the other. The clone keeps the position inside the current block as well.

**Rust**

```rust
// PRNG
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let mut rng = LustroPrng::new(&seed, StreamId(0));
rng.next_u64();

let mut twin = rng.clone();
assert_eq!(rng.next_u64(), twin.next_u64());
```

```rust
// XOF
use lustro::LustroXof;

let mut xof = LustroXof::new(b"hello world");
xof.next_u64();

let mut twin = xof.clone();
assert_eq!(xof.next_u64(), twin.next_u64());
```

**Python**

```python
# PRNG
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)
rng.next_u64()

twin = rng.copy()  # copy.copy() and copy.deepcopy() work too
assert rng.next_u64() == twin.next_u64()
```

```python
# XOF
from lustro import LustroXof

xof = LustroXof(b"hello world")
xof.next_u64()

twin = xof.copy()
assert xof.next_u64() == twin.next_u64()
```

**C / C++**

```c
/* PRNG */
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrng *rng = NULL;
lustro_prng_new(seed, 0, 0, &rng);

LustroPrng *twin = NULL;
lustro_prng_clone(rng, &twin);

lustro_prng_free(twin);
lustro_prng_free(rng);
```

```c
/* XOF */
#include "lustro.h"

LustroXof *xof = NULL;
lustro_xof_new((const uint8_t *)"hello world", 11, &xof);

LustroXof *twin = NULL;
lustro_xof_clone(xof, &twin);

lustro_xof_free(twin);
lustro_xof_free(xof);
```

---

### Fork

Derive a child stream from the current state and a 128-bit identifier. The parent is not changed.

**Rust**

```rust
// PRNG
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let rng = LustroPrng::new(&seed, StreamId(0));

let mut child = rng.fork(StreamId(1));
let value = child.next_u64();
```

```rust
// XOF
use lustro::{LustroXof, StreamId};

let xof = LustroXof::new(b"hello world");

let mut child = xof.fork(StreamId(1));
let value = child.next_u64();
```

**Python**

```python
# PRNG
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)

child = rng.fork(1)
value = child.next_u64()
```

```python
# XOF
from lustro import LustroXof

xof = LustroXof(b"hello world")

child = xof.fork(1)
value = child.next_u64()
```

**C / C++**

```c
/* PRNG */
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrng *rng = NULL;
lustro_prng_new(seed, 0, 0, &rng);

LustroPrng *child = NULL;
lustro_prng_fork(rng, 0, 1, &child);  /* id as (hi, lo) */

uint64_t value;
lustro_prng_next_u64(child, &value);

lustro_prng_free(child);
lustro_prng_free(rng);
```

```c
/* XOF */
#include "lustro.h"

LustroXof *xof = NULL;
lustro_xof_new((const uint8_t *)"hello world", 11, &xof);

LustroXof *child = NULL;
lustro_xof_fork(xof, 0, 1, &child);  /* id as (hi, lo) */

uint64_t value;
lustro_xof_next_u64(child, &value);

lustro_xof_free(child);
lustro_xof_free(xof);
```

The same parent state and identifier always give the same child. Reading more bytes from the block the parent is already in does not change the child; moving the parent on to a later block changes the state it is derived from. The child starts at its own first block and does not inherit the parent's position.

The output block holds the two state words `fork()` derives from, so once a PRNG has generated a block, `fork(id)` equals `LustroPrng::new` seeded with that block and the same identifier. Whoever knows the block can derive the children; Lustro makes no cryptographic claim. The identifier is hashed together with the state.

---

### Derive Path

Derive a stream directly from a seed (or message) and a path of identifiers, without exposing the intermediate streams.

**Rust**

```rust
// PRNG
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let rng = LustroPrng::derive_path(&seed, &[StreamId(12), StreamId(7), StreamId(99)]);
```

```rust
// XOF
use lustro::{LustroXof, StreamId};

let xof = LustroXof::derive_path(b"hello world", &[StreamId(12), StreamId(7), StreamId(99)]);
```

**Python**

```python
# PRNG
from lustro import LustroPrng

seed = bytes(32)
rng = LustroPrng.derive_path(seed, [12, 7, 99])
```

```python
# XOF
from lustro import LustroXof

xof = LustroXof.derive_path(b"hello world", [12, 7, 99])
```

**C / C++**

```c
/* PRNG */
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};

uint64_t ids_hi[3] = {0, 0, 0};
uint64_t ids_lo[3] = {12, 7, 99};

LustroPrng *rng = NULL;
lustro_prng_derive_path(seed, ids_hi, ids_lo, 3, &rng);

lustro_prng_free(rng);
```

```c
/* XOF */
#include "lustro.h"

uint64_t ids_hi[3] = {0, 0, 0};
uint64_t ids_lo[3] = {12, 7, 99};

LustroXof *xof = NULL;
lustro_xof_derive_path((const uint8_t *)"hello world", 11, ids_hi, ids_lo, 3, &xof);

lustro_xof_free(xof);
```

For a PRNG, `derive_path(seed, path)` gives the same stream as `new` with the first
identifier followed by `fork` for each remaining one, in a single call. For an XOF,
every identifier in `path` is a `fork`, since the message already is the root.

---

## Batches

### Batch Hash

Hash many messages in one call. `*_many_fixed` takes messages of one length stored back to back in a single buffer; `*_many_var` takes messages of any lengths.

**Rust**

```rust
use lustro::{hash256_many_fixed_into, hash256_many_var, Hash256};

// Messages of one length, stored back to back.
let data = [0u8; 3 * 16];
let mut out = vec![Hash256::default(); 3];
hash256_many_fixed_into(&data, 16, &mut out);

// Messages of any length.
let messages = [b"a".as_ref(), b"bc".as_ref(), b"".as_ref()];
let digests = hash256_many_var(&messages);
```

`hash256_many_var_into(&messages, &mut out)` writes into a slice you provide. `hash256_many_fixed_into` panics if `data.len() != out.len() * message_len`.

**Python**

```python
import numpy as np
import lustro

# Equal-length messages: one message per row of a 2D uint8 array.
rows = np.zeros((3, 16), dtype=np.uint8)
digests = lustro.hash256_many_fixed(rows)  # uint8 array, shape (3, 32)

# Messages of any length: a sequence of bytes or byte buffers.
digests = lustro.hash256_many_var([b"a", b"bc", b""])  # shape (3, 32)

# Into an array you provide, see "GIL and Throughput (Python)".
out = np.empty((3, 32), dtype=np.uint8)
lustro.hash256_many_fixed_into(rows, out)
lustro.hash256_many_var_into([b"a", b"bc", b""], out)
```

**C / C++**

```c
#include "lustro.h"

/* Equal-length messages, stored back to back. */
uint8_t rows[3][16] = {{0}};
uint8_t digests[3][LUSTRO_HASH256_LEN];
lustro_hash256_many_fixed((const uint8_t *)rows, 3, 16, (uint8_t *)digests);

/* Messages of any length. */
const uint8_t *messages[] = {
    (const uint8_t *)"a",
    (const uint8_t *)"bc",
    (const uint8_t *)""
};
uintptr_t lengths[] = {1, 2, 0};

lustro_hash256_many_var(messages, 3, lengths, (uint8_t *)digests);
```

The names mean the same in Rust, Python and C/C++. `hash128_*` and `lustro_hash128_*` work the same way and return 16-byte digests.

Both variants can use AVX2 for batches of 4 or more messages and send large batches to the thread pool, as far as the CPU and the runtime settings (see Runtime Settings) allow. The digests do not depend on the backend or on the number of threads.

---

### Batch PRNG and XOF

Many independent streams in one object, for PRNG and XOF alike.

A PRNG or XOF batch produces output in steps: `fill_blocks(out, steps)` writes `steps` blocks for every stream. `steps` only sets how much one call produces. The same total split over several calls gives every stream the same blocks. `suggested_steps()` returns a `steps` chosen for speed at the current number of streams and backend (`LUSTRO_DISABLE_SIMD` can change it). It is a hint only: any `steps` gives correct output. The output holds `len() * steps` blocks, so with many streams check its size before allocating.

**Rust**

```rust
// PRNG
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let mut prng = LustroPrngBatch::new(&seed, &ids);

let steps = prng.suggested_steps(); // or a fixed number, e.g. 2
let mut out = vec![[0u8; 32]; prng.len() * steps];
prng.fill_blocks(&mut out, steps);

// Or let the library allocate the vector:
let more = prng.blocks(steps); // Vec<[u8; 32]>, prng.len() * steps blocks
```

```rust
// XOF
use lustro::LustroXofBatch;

let messages = [b"msg0".as_ref(), b"msg1".as_ref(), b"msg2".as_ref()];
let mut xof = LustroXofBatch::new(&messages);

let steps = xof.suggested_steps(); // or a fixed number, e.g. 2
let mut out = vec![[0u8; 32]; xof.len() * steps];
xof.fill_blocks(&mut out, steps);

let more = xof.blocks(steps);
```

**Python**

```python
# PRNG
import numpy as np
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])

steps = prng.suggested_steps()  # or a fixed number, e.g. 2
out = np.empty((steps, len(prng), 4), dtype=np.uint64)
prng.fill_blocks(out, steps)

# Or let the library allocate the array:
more = prng.blocks(steps)
```

```python
# XOF
import numpy as np
from lustro import LustroXofBatch

xof = LustroXofBatch([b"msg0", b"msg1", b"msg2"])

steps = xof.suggested_steps()  # or a fixed number, e.g. 2
out = np.empty((steps, len(xof), 4), dtype=np.uint64)
xof.fill_blocks(out, steps)

more = xof.blocks(steps)
```

`fill_blocks(out, steps)` writes one 32-byte block per stream per step, step-major: stream `i` at step `s` is block `s * n + i` (`n` = number of streams). In Python that is `out[s, i]`, with `out` shaped `(steps, n_streams, 4)`; each block is four `uint64` values. `blocks(steps)` (Rust and Python) returns the same blocks in a newly allocated result: a `Vec<[u8; 32]>` in Rust, a new, writable array of that shape in Python.

In Python, a batch keeps an internal buffer the size of its largest `fill_blocks()` call made with the default `release_gil=True`; `release_buffer()` frees it earlier. `fill_blocks(..., release_gil=False)` and `blocks()` do not use that buffer.

**C / C++**

```c
/* PRNG */
#include <stdlib.h>
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};

uint64_t ids_hi[4] = {0};
uint64_t ids_lo[4] = {0, 1, 2, 3};

LustroPrngBatch *prng = NULL;
lustro_prng_batch_new(seed, ids_hi, ids_lo, 4, &prng);

size_t steps = lustro_prng_batch_suggested_steps(prng);  /* or a fixed number, e.g. 2 */
size_t out_len = steps * lustro_prng_batch_len(prng) * LUSTRO_BLOCK_LEN;
uint8_t *out = (uint8_t *)malloc(out_len);

lustro_prng_batch_fill_blocks(prng, out, out_len, steps);

free(out);
lustro_prng_batch_free(prng);
```

```c
/* XOF */
#include <stdlib.h>
#include "lustro.h"

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};
uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof = NULL;
lustro_xof_batch_new(messages, lengths, 3, &xof);

size_t steps = lustro_xof_batch_suggested_steps(xof);  /* or a fixed number, e.g. 2 */
size_t out_len = steps * lustro_xof_batch_len(xof) * LUSTRO_BLOCK_LEN;
uint8_t *out = (uint8_t *)malloc(out_len);

lustro_xof_batch_fill_blocks(xof, out, out_len, steps);

free(out);
lustro_xof_batch_free(xof);
```

In C/C++, `lustro_batch_block_offset(step, batch_len, lane)` from `lustro.h` gives the byte offset of one block in the output: `(step * batch_len + lane) * LUSTRO_BLOCK_LEN`. It expects `step < steps` and `lane < batch_len` and does not check its arguments.

```c
/* lane 2 at step 0 of the PRNG output above */
const uint8_t *block = out + lustro_batch_block_offset(0, 4, 2);
```

---

### Batch Single Lane

Take one lane out of a batch as a single stream. The stream continues the lane: its next block is the block the batch would write for that lane next. The batch is not changed, and the two evolve independently afterwards.

**Rust**

```rust
// PRNG
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let mut prng = LustroPrngBatch::new(&seed, &ids);

prng.blocks(2);

// None if the index is out of range.
let mut lane = prng.lane(1).expect("lane in range");
let block = lane.next_block();
```

```rust
// XOF
use lustro::LustroXofBatch;

let messages = [b"msg0".as_ref(), b"msg1".as_ref(), b"msg2".as_ref()];
let mut xof = LustroXofBatch::new(&messages);

xof.blocks(2);

let mut lane = xof.lane(1).expect("lane in range");
let block = lane.next_block();
```

**Python**

```python
# PRNG
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])
prng.blocks(2)

lane = prng.lane(1)  # IndexError if the index is out of range
block = lane.next_block()
```

```python
# XOF
from lustro import LustroXofBatch

xof = LustroXofBatch([b"msg0", b"msg1", b"msg2"])
xof.blocks(2)

lane = xof.lane(1)
block = lane.next_block()
```

**C / C++**

```c
/* PRNG */
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

```c
/* XOF */
#include "lustro.h"

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};
uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof = NULL;
lustro_xof_batch_new(messages, lengths, 3, &xof);

LustroXof *lane = NULL;
LustroError err = lustro_xof_batch_lane(xof, 1, &lane);

uint8_t block[LUSTRO_BLOCK_LEN];
lustro_xof_next_block(lane, block);

lustro_xof_free(lane);  /* a single stream, not a batch */
lustro_xof_batch_free(xof);
```

The result is an ordinary single stream (`LustroPrng` or `LustroXof`), so it supports typed reads, `fork` and snapshots. `lane(i).fork(id)` equals child `i` of `fork` called with `id` at position `i`. An out-of-range index gives `None` (Rust), `IndexError` (Python) or `LUSTRO_ERROR_INVALID_ARGUMENT` (C/C++).

---

### Batch Clone

Copy a whole batch with the exact state of every lane. The copy is independent.

**Rust**

```rust
// PRNG
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let mut prng = LustroPrngBatch::new(&seed, &ids);
prng.blocks(2);

let twin = prng.clone();

// Returns Err(BatchError::Reserve) instead of aborting if the lanes cannot be allocated.
let twin = prng.try_clone()?;
```

```rust
// XOF
use lustro::LustroXofBatch;

let messages = [b"msg0".as_ref(), b"msg1".as_ref(), b"msg2".as_ref()];
let mut xof = LustroXofBatch::new(&messages);
xof.blocks(2);

let twin = xof.clone();
let twin = xof.try_clone()?;
```

**Python**

```python
# PRNG
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])
prng.blocks(2)

twin = prng.copy()  # MemoryError if the lanes cannot be allocated
```

```python
# XOF
from lustro import LustroXofBatch

xof = LustroXofBatch([b"msg0", b"msg1", b"msg2"])
xof.blocks(2)

twin = xof.copy()
```

**C / C++**

```c
/* PRNG */
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrngBatch *prng = NULL;
lustro_prng_batch_new_range(seed, 0, 0, 4, &prng);

LustroPrngBatch *twin = NULL;
LustroError err = lustro_prng_batch_clone(prng, &twin);  /* LUSTRO_ERROR_OUT_OF_MEMORY if the lanes cannot be allocated */

lustro_prng_batch_free(twin);
lustro_prng_batch_free(prng);
```

```c
/* XOF */
#include "lustro.h"

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};
uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof = NULL;
lustro_xof_batch_new(messages, lengths, 3, &xof);

LustroXofBatch *twin = NULL;
LustroError err = lustro_xof_batch_clone(xof, &twin);

lustro_xof_batch_free(twin);
lustro_xof_batch_free(xof);
```

In Rust, `clone()` aborts the process if the lanes cannot be allocated; `try_clone()` reports it as an error.

---

### Batch Fork

Create multiple child streams. The parent is not changed.

**Rust**

```rust
// PRNG
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];

let prng = LustroPrngBatch::new(&seed, &ids);
let one_each = prng.fork(&[StreamId(10), StreamId(11), StreamId(12), StreamId(13)]);
let children = prng.fork_range(StreamId(100));
let many_children = prng.fork_many(&[StreamId(100), StreamId(200)]);
```

```rust
// XOF
use lustro::{LustroXofBatch, StreamId};

let messages = [b"msg0".as_ref(), b"msg1".as_ref(), b"msg2".as_ref()];

let xof = LustroXofBatch::new(&messages);
let one_each = xof.fork(&[StreamId(10), StreamId(11), StreamId(12)]);
let children = xof.fork_range(StreamId(100));
let many_children = xof.fork_many(&[StreamId(100), StreamId(200)]);
```

**Python**

```python
# PRNG
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])

one_each = prng.fork([10, 11, 12, 13])
children = prng.fork_range(100)
many_children = prng.fork_many([100, 200])
```

```python
# XOF
from lustro import LustroXofBatch

xof = LustroXofBatch([b"msg0", b"msg1", b"msg2"])

one_each = xof.fork([10, 11, 12])
children = xof.fork_range(100)
many_children = xof.fork_many([100, 200])
```

**C / C++**

```c
/* PRNG */
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrngBatch *prng = NULL;
lustro_prng_batch_new_range(seed, 0, 0, 4, &prng);

uint64_t ids_hi[4] = {0, 0, 0, 0};
uint64_t ids_lo[4] = {10, 11, 12, 13};

LustroPrngBatch *one_each = NULL;
lustro_prng_batch_fork(prng, ids_hi, ids_lo, 4, &one_each);

LustroPrngBatch *children = NULL;
lustro_prng_batch_fork_range(prng, 0, 100, &children);

uint64_t k_hi[2] = {0, 0};
uint64_t k_lo[2] = {100, 200};

LustroPrngBatch *many_children = NULL;
lustro_prng_batch_fork_many(prng, k_hi, k_lo, 2, &many_children);

lustro_prng_batch_free(many_children);
lustro_prng_batch_free(children);
lustro_prng_batch_free(one_each);
lustro_prng_batch_free(prng);
```

```c
/* XOF */
#include "lustro.h"

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};
uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof = NULL;
lustro_xof_batch_new(messages, lengths, 3, &xof);

uint64_t ids_hi[3] = {0, 0, 0};
uint64_t ids_lo[3] = {10, 11, 12};

LustroXofBatch *one_each = NULL;
lustro_xof_batch_fork(xof, ids_hi, ids_lo, 3, &one_each);

LustroXofBatch *children = NULL;
lustro_xof_batch_fork_range(xof, 0, 100, &children);

uint64_t k_hi[2] = {0, 0};
uint64_t k_lo[2] = {100, 200};

LustroXofBatch *many_children = NULL;
lustro_xof_batch_fork_many(xof, k_hi, k_lo, 2, &many_children);

lustro_xof_batch_free(many_children);
lustro_xof_batch_free(children);
lustro_xof_batch_free(one_each);
lustro_xof_batch_free(xof);
```

`fork(ids)` derives one child per lane, using the identifier at the same position. `ids` must have exactly `batch.len()` elements (a panic in Rust, `ValueError` in Python, `LUSTRO_ERROR_INVALID_LENGTH` in C/C++).
`fork_range(first)` derives `batch.len()` children using sequential stream IDs.
`fork_many(ids)` derives `ids.len()` children per lane instead, producing a
batch of `batch.len() * ids.len()` lanes, ordered parent-major: child `j` of
lane `i` is at index `i * ids.len() + j`. Each child starts at step 0.

---

### Batch Derive Path

Derive a batch directly from a shared path, one lane per root (PRNG) or per message (XOF).

**Rust**

```rust
// PRNG
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let roots = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let path = [StreamId(12), StreamId(7)];

let batch = LustroPrngBatch::derive_path(&seed, &roots, &path);
```

```rust
// XOF
use lustro::{LustroXofBatch, StreamId};

let messages = [b"msg0".as_ref(), b"msg1".as_ref(), b"msg2".as_ref()];
let path = [StreamId(12), StreamId(7)];

let batch = LustroXofBatch::derive_path(&messages, &path);
```

**Python**

```python
# PRNG
from lustro import LustroPrngBatch

seed = bytes(32)
batch = LustroPrngBatch.derive_path(seed, [0, 1, 2, 3], [12, 7])
```

```python
# XOF
from lustro import LustroXofBatch

batch = LustroXofBatch.derive_path([b"msg0", b"msg1", b"msg2"], [12, 7])
```

**C / C++**

```c
/* PRNG */
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

```c
/* XOF */
#include "lustro.h"

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};
uintptr_t lengths[] = {4, 4, 4};

uint64_t path_hi[2] = {0, 0};
uint64_t path_lo[2] = {12, 7};

LustroXofBatch *batch = NULL;
lustro_xof_batch_derive_path(
    messages, lengths, 3, path_hi, path_lo, 2, &batch
);

lustro_xof_batch_free(batch);
```

For a PRNG batch, `derive_path(seed, roots, path)` derives a canonical batch: lane `i` equals
`LustroPrng::derive_path(seed, [roots[i]] + path)`, independent of any other
stream's state. For an XOF batch, lane `i` equals `LustroXof::derive_path(messages[i], path)`. In Python the messages can be any sequence of `bytes` or byte buffers.

---

## State and Runtime

### Snapshot

Save and restore the exact stream position.

**Rust**

```rust
// PRNG
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

```rust
// XOF
use lustro::types::LustroXofSnapshot;
use lustro::LustroXof;

let mut xof = LustroXof::new(b"hello world");

let snapshot = xof.export_snapshot();
let bytes = snapshot.to_le_bytes();

let snapshot =
    LustroXofSnapshot::from_le_bytes(&bytes).expect("valid snapshot");

let mut restored = LustroXof::import_snapshot(snapshot);
```

**Python**

```python
# PRNG
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)

snapshot = rng.export_snapshot()
restored = LustroPrng.import_snapshot(snapshot)
```

```python
# XOF
from lustro import LustroXof

xof = LustroXof(b"hello world")

snapshot = xof.export_snapshot()
restored = LustroXof.import_snapshot(snapshot)
```

**C / C++**

```c
/* PRNG */
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

```c
/* XOF */
#include <stdio.h>
#include "lustro.h"

LustroXof *xof = NULL;
lustro_xof_new((const uint8_t *)"hello world", 11, &xof);

uint8_t snapshot[LUSTRO_SNAPSHOT_LEN];
lustro_xof_export_snapshot(xof, snapshot);

LustroXof *restored = NULL;
LustroError err = lustro_xof_import_snapshot(snapshot, &restored);
if (err != LUSTRO_ERROR_OK) {
    fprintf(stderr, "%s\n", lustro_strerror(err));
}

lustro_xof_free(restored);
lustro_xof_free(xof);
```

---

### Batch Snapshot

Save and restore every lane of a batch at once. The snapshot has a dynamic size: a header plus one entry per lane.

**Rust**

```rust
// PRNG
use lustro::types::LustroPrngBatchSnapshot;
use lustro::{LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let mut prng = LustroPrngBatch::new(&seed, &ids);
prng.blocks(2);

let snapshot = prng.export_snapshot();
let bytes = snapshot.to_le_bytes();   // Vec<u8>

let snapshot =
    LustroPrngBatchSnapshot::from_le_bytes(&bytes).expect("valid snapshot");

let mut restored = LustroPrngBatch::import_snapshot(snapshot);
```

```rust
// XOF
use lustro::types::LustroXofBatchSnapshot;
use lustro::LustroXofBatch;

let messages = [b"msg0".as_ref(), b"msg1".as_ref(), b"msg2".as_ref()];
let mut xof = LustroXofBatch::new(&messages);
xof.blocks(2);

let snapshot = xof.export_snapshot();
let bytes = snapshot.to_le_bytes();   // Vec<u8>

let snapshot =
    LustroXofBatchSnapshot::from_le_bytes(&bytes).expect("valid snapshot");

let mut restored = LustroXofBatch::import_snapshot(snapshot);
```

**Python**

```python
# PRNG
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])
prng.blocks(2)

snapshot = prng.export_snapshot()   # bytes
restored = LustroPrngBatch.import_snapshot(snapshot)
```

```python
# XOF
from lustro import LustroXofBatch

xof = LustroXofBatch([b"msg0", b"msg1", b"msg2"])
xof.blocks(2)

snapshot = xof.export_snapshot()    # bytes
restored = LustroXofBatch.import_snapshot(snapshot)
```

**C / C++**

```c
/* PRNG */
#include <stdio.h>
#include <stdlib.h>
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrngBatch *prng = NULL;
lustro_prng_batch_new_range(seed, 0, 0, 4, &prng);

size_t size = lustro_prng_batch_snapshot_size(prng);
uint8_t *snapshot = (uint8_t *)malloc(size);
lustro_prng_batch_export_snapshot(prng, snapshot, size);

LustroPrngBatch *restored = NULL;
LustroError err = lustro_prng_batch_import_snapshot(snapshot, size, &restored);
if (err != LUSTRO_ERROR_OK) {
    fprintf(stderr, "%s\n", lustro_strerror(err));
}

lustro_prng_batch_free(restored);
free(snapshot);
lustro_prng_batch_free(prng);
```

```c
/* XOF */
#include <stdio.h>
#include <stdlib.h>
#include "lustro.h"

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};
uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof = NULL;
lustro_xof_batch_new(messages, lengths, 3, &xof);

size_t size = lustro_xof_batch_snapshot_size(xof);
uint8_t *snapshot = (uint8_t *)malloc(size);
lustro_xof_batch_export_snapshot(xof, snapshot, size);

LustroXofBatch *restored = NULL;
LustroError err = lustro_xof_batch_import_snapshot(snapshot, size, &restored);
if (err != LUSTRO_ERROR_OK) {
    fprintf(stderr, "%s\n", lustro_strerror(err));
}

lustro_xof_batch_free(restored);
free(snapshot);
lustro_xof_batch_free(xof);
```

The snapshot size is `LUSTRO_BATCH_SNAPSHOT_HEADER_LEN + n * LUSTRO_BATCH_SNAPSHOT_LANE_LEN` for `n` streams. In C/C++ call `*_batch_snapshot_size(ctx)` before export; `out_len` must equal it.

---

### Runtime Settings

Thread count and SIMD backend follow `LUSTRO_NUM_THREADS`, `LUSTRO_DISABLE_MT` and `LUSTRO_DISABLE_SIMD` by default. `configure` sets them from code and must be called before the first hash batch call or batch `fill_blocks`/`blocks`. They apply to hash batches as well as to PRNG and XOF batches.

`LUSTRO_DISABLE_MT` and `LUSTRO_DISABLE_SIMD` are set by `1`, `true`, `on` or `yes` and are not set by `0`, `false`, `off`, `no` or an empty value (case is ignored). Any other value sets them.

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

The settings can be set only once. The first hash batch call, batch `fill_blocks`/`blocks`, `suggested_steps()` or `runtime_info()` fixes them too, so a later `configure` fails, even with the same arguments: `ConfigError::AlreadyConfigured` (Rust), `RuntimeError` (Python), `LUSTRO_ERROR_ALREADY_CONFIGURED` (C/C++). Bad arguments fail without changing anything: `ValueError` for `threads < 1` (Python), `LUSTRO_ERROR_INVALID_ARGUMENT` for an unknown `simd` (C/C++). `runtime_info()` reports the selected backend and the configured pool size (0 = batches run on the calling thread). It does not report running workers: the pool starts later and may fail to start. A child forked after its parent first used the pool runs on the calling thread; one forked earlier may start its own pool.

---

## Language Notes

### Python Notes

Single streams (`LustroPrng`, `LustroXof`) can also return or fill larger amounts of data:

```python
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)

data = rng.fill(1000)         # bytes
buf = bytearray(1000)
rng.fill_into(buf)            # bytearray, writable memoryview or NumPy uint8 array
words = rng.fill_u64(1000)    # NumPy uint64 array of shape (1000,)

data = rng.read(1000)         # same as fill(1000)
n = rng.readinto(buf)         # n == 1000, any writable C-contiguous buffer
```

`fill_u64(n)` needs NumPy and returns a new, writable array with the values that `n` calls of `next_u64()` would give.

`read(size)` is the same as `fill(size)`; there is no `read()` without a size, because the stream has no end. `readinto(buf)` accepts any writable, C-contiguous buffer, whatever its item type (for example `array('I')` or a NumPy `uint32` array). It writes the stream as raw bytes, without conversion, and returns the number of bytes written. `fill_into(buf)` takes byte buffers only.

Seeds, messages, hash input and snapshots are given as `bytes`. `bytearray`, `memoryview`, `array('B')` and NumPy `uint8` arrays are accepted too; they must be C-contiguous with byte items (`TypeError` otherwise) and are copied. `hash256_many_var`, `hash128_many_var` and `LustroXofBatch` take any sequence of such objects, but not a single string or buffer.

An instance must not be used by several threads at the same time (an overlapping call raises `RuntimeError`). Use `copy()`, `fork()` or separate instances instead. The installed version is available as `lustro.__version__`, and the package ships type stubs (`lustro.pyi`, `py.typed`).

---

### GIL and Throughput (Python)

| Call | GIL |
|---|---|
| `hash256`, `hash128` | released while hashing |
| `hash256_many_fixed`, `hash128_many_fixed` | released; the input is copied first |
| `hash*_many_fixed_into` | released by default, held with `release_gil=False` |
| `hash*_many_var`, `hash*_many_var_into` | released by default, held with `release_gil=False` |
| `fill_blocks` (`LustroPrngBatch`, `LustroXofBatch`) | released by default, held with `release_gil=False` |
| `fill`, `fill_u64`, `blocks` | released while generating |
| `fill_into`, `readinto` | held |

By default (`release_gil=True`) other Python threads run during the call. Input is copied before the GIL is released and results are copied into `out` after it is taken back, which costs time on large batches.

`release_gil=False` is the maximum-throughput mode: the GIL stays held and, where the input allows, data is read in place and digests are written straight into `out`. Other Python threads wait. The thread pool and AVX2 are used in both modes.

```python
out = np.empty((n, 32), dtype=np.uint8)
lustro.hash256_many_fixed_into(rows, out, release_gil=False)
lustro.hash256_many_var_into(messages, out, release_gil=False)

steps = prng.suggested_steps()
blocks = np.empty((steps, len(prng), 4), dtype=np.uint64)
prng.fill_blocks(blocks, steps, release_gil=False)
```

In this mode:

- Not every input is read in place. In `hash*_many_var` and `hash*_many_var_into` items that are not `bytes` are copied. In `hash*_many_fixed_into` a `data` view that is not C-contiguous (for example `a[:, :16]`) is copied and hashed with the GIL released.
- An exact `list` or `tuple` of exact `bytes` (no subclasses) is read without taking a reference on each item, which is faster for many small messages.
- `out` must not share memory with the input. The same array, or views of one array, as `data` and `out` raise `TypeError` in both modes. Memory shared through separate wrappers (for example a `memoryview` of `out` used as a message) raises `ValueError` here; by default the input is copied first and the call is allowed.
- When a `list` or `tuple` of 16384 or more items is given to `hash*_many_var_into` and the batch runs on the thread pool, it is hashed while it is read, so `out` may be partly written if the call then fails. By default nothing is written before all items are read.
- `fill_blocks` writes in place on little-endian targets; elsewhere the flag has no effect.

---

### rand_core Integration (Rust)

With `--features rand`, `LustroPrng` and `LustroXof` implement the `rand_core` 0.10
traits `TryRng` and `Rng` (infallible, `Error = Infallible`). `LustroPrng` also
implements `SeedableRng`. For `LustroXof` its input is a message of any
length.

```toml
[dependencies]
lustro = { version = "<version>", features = ["rand"] }
rand_core = "0.10"
```

```rust
use lustro::{LustroPrng, LustroXof, Seed256, StreamId};
use rand_core::{Rng, SeedableRng};

fn draw(rng: &mut impl Rng) -> u64 {
    rng.next_u64()
}

// PRNG from a 32-byte seed (stream 0).
let mut prng = LustroPrng::from_seed([7u8; 32]);
let a = draw(&mut prng);

// Random seed (needs the system entropy source).
let mut random = LustroPrng::from(Seed256::from_entropy()?);
let mut buf = [0u8; 100];
random.fill_bytes(&mut buf);

// XOF over a message of any length.
let mut xof = LustroXof::new(b"message");
let b = draw(&mut xof);

// On a LustroPrng, fork takes a StreamId.
let mut child = prng.fork(StreamId(1));
let c = child.next_u64();
```

- **Same stream:** `Rng` returns the same bytes, from the same position, as the
  inherent `next_u32`, `next_u64` and `fill_bytes`.
- **Seeding:** `from_seed` starts at stream 0. `rand_core` 0.10 has no
  `from_entropy`, so use `LustroPrng::from(Seed256::from_entropy()?)`.
- **`fork`:** `LustroPrng::fork(StreamId)` shadows `SeedableRng::fork()` on a
  concrete `LustroPrng`. Only code that is generic over `SeedableRng` calls the
  trait method.
- **Quality:** implementing `Rng` is an interface, not a claim about cryptographic quality. `CryptoRng` is not implemented.

---

## Reference

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