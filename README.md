# Lustro API

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

## Speed Test and Validation

The repo contains **Lustro_API_ffi_python_validator.py** and **Lustro_API_ffi_base_speed_test.py**. Both scripts require lustro.dll in the same folder. The speed tester works only with C/C++.

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

let mut prng_out = vec![[0u8; 32]; prng.len()];
prng.fill_blocks(&mut prng_out);

let messages = [
    b"msg0".as_ref(),
    b"msg1".as_ref(),
    b"msg2".as_ref(),
];

let mut xof = LustroXofBatch::new(&messages);
let mut xof_out = vec![[0u8; 32]; xof.len()];
xof.fill_blocks(&mut xof_out);
```

**Python**

```python
import numpy as np
from lustro import LustroPrngBatchPy, LustroXofBatchPy

seed = bytes(32)

prng = LustroPrngBatchPy.new(seed, [0, 1, 2, 3])
prng_out = np.empty((prng.len(), 4), dtype=np.uint64)
prng.fill_blocks(prng_out)

xof = LustroXofBatchPy.new([b"msg0", b"msg1", b"msg2"])
xof_out = np.empty((xof.len(), 4), dtype=np.uint64)
xof.fill_blocks(xof_out)
```

`fill_blocks()` writes one 32-byte block per stream. In Python, each block is represented as four `uint64` values.

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};

uint64_t ids_hi[4] = {0};
uint64_t ids_lo[4] = {0, 1, 2, 3};

LustroPrngBatch *prng =
    lustro_prng_batch_new(seed, ids_hi, ids_lo, 4);

uint8_t prng_out[4][32];
lustro_prng_batch_fill_blocks(
    prng, (uint8_t *)prng_out, sizeof(prng_out)
);

const uint8_t *messages[] = {
    (const uint8_t *)"msg0",
    (const uint8_t *)"msg1",
    (const uint8_t *)"msg2"
};

uintptr_t lengths[] = {4, 4, 4};

LustroXofBatch *xof =
    lustro_xof_batch_new(messages, lengths, 3);

uint8_t xof_out[3][32];
lustro_xof_batch_fill_blocks(
    xof, (uint8_t *)xof_out, sizeof(xof_out)
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
```

**Python**

```python
from lustro import LustroPrngBatchPy

seed = bytes(32)
prng = LustroPrngBatchPy.new(seed, [0, 1, 2, 3])

children = prng.fork_range(100)
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[32] = {0};
LustroPrngBatch *prng =
    lustro_prng_batch_new_range(seed, 0, 0, 4);

LustroPrngBatch *children =
    lustro_prng_batch_fork_range(prng, 0, 100);

lustro_prng_batch_free(children);
lustro_prng_batch_free(prng);
```

`fork_range(first)` derives `batch.len()` children using sequential stream IDs. Each child starts at step 0.

The same batch fork API is available for XOF.

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
