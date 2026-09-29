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

# Python

maturin develop --release

# C/C++ FFI bindings

cargo build --release --features ffi

```

All release builds catch internal panics and convert them to `LustroError::InternalPanic` (or the corresponding Python exception) at the boundary instead of just aborting the process. See ARCHITECTURE.md §13.

## Speed Test and Validation

This repo is the full API implementation and also contains **Lustro_Golden_Vectors_Validator.py** along with **Lustro_Speed_Tester.py**. These scripts requires C FFI lustro.dll in the same folder.

Please note that the core mechanism and the API implementation are considered non-cryptographic at this moment.

---

## Quick Start

### Hash

**Rust**

```rust
use lustro::hash::hash256;

let digest = hash256(b"hello world");
```

**Python**

```python
from lustro import LustroHashPy

digest = LustroHashPy().hash256(b"hello world")
```

**C / C++**

```c
#include "lustro.h"

uint8_t digest[32];
lustro_hash256((const uint8_t *)"hello world", 11, digest);
```

---

### PRNG

**Rust**

```rust
use lustro::prng::LustroPrng;
use lustro::types::{Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let mut rng = LustroPrng::new(&seed, StreamId(0));

let value = rng.next_u64();
```

**Python**

```python
from lustro import LustroPrngPy

rng = LustroPrngPy(bytes(32), 0)
value = rng.next_u64()
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};
LustroPrng *rng = lustro_prng_new(seed, 0, 0);

uint64_t value;
lustro_prng_next_u64(rng, &value);

lustro_prng_free(rng);
```

---

### XOF

**Rust**

```rust
use lustro::xof::LustroXof;

let mut xof = LustroXof::new(b"hello world");
let block = xof.next_block();
```

**Python**

```python
from lustro import LustroXofPy

xof = LustroXofPy(b"hello world")
block = xof.next_block()
```

**C / C++**

```c
#include "lustro.h"

LustroXof *xof =
    lustro_xof_new((const uint8_t *)"hello world", 11);

uint8_t block[32];
lustro_xof_next_block(xof, block);

lustro_xof_free(xof);
```

---

### Derive Path

Derive a stream directly from seed and path of identifiers, without
exposing the intermediate streams.

**Rust**

```rust
use lustro::prng::LustroPrng;
use lustro::types::{Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let rng = LustroPrng::derive_path(&seed, &[StreamId(12), StreamId(7), StreamId(99)]);
```

**Python**

```python
from lustro import LustroPrngPy

seed = bytes(32)
rng = LustroPrngPy.derive_path(seed, [12, 7, 99])
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};

uint64_t ids_hi[3] = {0, 0, 0};
uint64_t ids_lo[3] = {12, 7, 99};

LustroPrng *rng =
    lustro_prng_derive_path(seed, ids_hi, ids_lo, 3);

lustro_prng_free(rng);
```

`derive_path(seed, path)` derives the same stream as calling `new` with the
first identifier, then `fork` for each remaining one — in one call, without
creating the intermediate streams. The same API is available for XOF, via
`LustroXof::derive_path(message, path)` (Rust), `LustroXofPy.derive_path(message, path)`
(Python), and `lustro_xof_derive_path(message, message_len, ids_hi, ids_lo, n)`
(C/C++); there every identifier in `path` is a `fork`, since `XOF::new` already
takes the message as its root.

---

### Batch — PRNG and XOF

Lustro can process multiple independent streams.

**Rust**

```rust
use lustro::prng::LustroPrngBatch;
use lustro::types::{Seed256, StreamId};
use lustro::xof::LustroXofBatch;

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
```

**Python**

```python
import numpy as np
from lustro import LustroPrngBatchPy, LustroXofBatchPy

seed = bytes(32)

steps = 2

prng = LustroPrngBatchPy.new(seed, [0, 1, 2, 3])
prng_out = np.empty((steps, prng.len(), 4), dtype=np.uint64)
prng.fill_blocks(prng_out, steps)

xof = LustroXofBatchPy.new([b"msg0", b"msg1", b"msg2"])
xof_out = np.empty((steps, xof.len(), 4), dtype=np.uint64)
xof.fill_blocks(xof_out, steps)
```

`fill_blocks(out, steps)` writes one 32-byte block per stream per step, step-major: stream `i` at step `s` is block `s * n + i` (`n` = number of streams). In Python that is `out[s, i]`, with `out` shaped `(steps, n_streams, 4)`; each block is four `uint64` values.

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};

uint64_t ids_hi[4] = {0};
uint64_t ids_lo[4] = {0, 1, 2, 3};

LustroPrngBatch *prng =
    lustro_prng_batch_new(seed, ids_hi, ids_lo, 4);

#define STEPS 2

uint8_t prng_out[STEPS][4][32];
lustro_prng_batch_fill_blocks(
    prng, (uint8_t *)prng_out, sizeof(prng_out), STEPS
);

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};

uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof =
    lustro_xof_batch_new(messages, lengths, 3);

uint8_t xof_out[STEPS][3][32];
lustro_xof_batch_fill_blocks(
    xof, (uint8_t *)xof_out, sizeof(xof_out), STEPS
);

lustro_xof_batch_free(xof);
lustro_prng_batch_free(prng);
```

---

### Batch Fork

Create multiple child streams.

**Rust**

```rust
use lustro::prng::LustroPrngBatch;
use lustro::types::{Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let ids = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];

let prng = LustroPrngBatch::new(&seed, &ids);
let children = prng.fork_range(StreamId(100));
let many_children = prng.fork_many(&[StreamId(100), StreamId(200)]);
```

**Python**

```python
from lustro import LustroPrngBatchPy

seed = bytes(32)
prng = LustroPrngBatchPy.new(seed, [0, 1, 2, 3])

children = prng.fork_range(100)
many_children = prng.fork_many([100, 200])
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};
LustroPrngBatch *prng =
    lustro_prng_batch_new_range(seed, 0, 0, 4);

LustroPrngBatch *children =
    lustro_prng_batch_fork_range(prng, 0, 100);

uint64_t k_hi[2] = {0, 0};
uint64_t k_lo[2] = {100, 200};

LustroPrngBatch *many_children =
    lustro_prng_batch_fork_many(prng, k_hi, k_lo, 2);

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
use lustro::prng::LustroPrngBatch;
use lustro::types::{Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let roots = [StreamId(0), StreamId(1), StreamId(2), StreamId(3)];
let path = [StreamId(12), StreamId(7)];

let batch = LustroPrngBatch::derive_path(&seed, &roots, &path);
```

**Python**

```python
from lustro import LustroPrngBatchPy

seed = bytes(32)
batch = LustroPrngBatchPy.derive_path(seed, [0, 1, 2, 3], [12, 7])
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};

uint64_t roots_hi[4] = {0, 0, 0, 0};
uint64_t roots_lo[4] = {0, 1, 2, 3};

uint64_t path_hi[2] = {0, 0};
uint64_t path_lo[2] = {12, 7};

LustroPrngBatch *batch =
    lustro_prng_batch_derive_path(
        seed, roots_hi, roots_lo, 4, path_hi, path_lo, 2
    );

lustro_prng_batch_free(batch);
```

`derive_path(seed, roots, path)` derives a canonical batch: lane `i` equals
`LustroPrng::derive_path(seed, [roots[i]] + path)`, independent of any other
stream's state. The same API is available for XOF, via
`LustroXofBatch::derive_path(messages, path)` (Rust),
`LustroXofBatchPy.derive_path(messages, path)` (Python), and
`lustro_xof_batch_derive_path(message_ptrs, message_lens, n_messages, path_hi, path_lo, n_path)`
(C/C++), using a list of messages instead of a seed and root identifiers.

---

### Snapshot

Save and restore the exact stream position.

**Rust**

```rust
use lustro::prng::LustroPrng;
use lustro::types::{LustroPrngSnapshot, Seed256, StreamId};

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
from lustro import LustroPrngPy

rng = LustroPrngPy(bytes(32), 0)

snapshot = rng.export_snapshot()
restored = LustroPrngPy.import_snapshot(snapshot)
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};
LustroPrng *rng = lustro_prng_new(seed, 0, 0);

uint8_t snapshot[56];
lustro_prng_export_snapshot(rng, snapshot);

LustroPrng *restored =
    lustro_prng_import_snapshot(snapshot);

lustro_prng_free(restored);
lustro_prng_free(rng);
```

The same snapshot API is available for XOF and batch contexts. Please note that for batch contexts in C/C++ the snapshot size is dynamic; call the corresponding `*_batch_snapshot_size(ctx)` before export.

For full API details please see **[ARCHITECTURE.md](ARCHITECTURE.md)**.