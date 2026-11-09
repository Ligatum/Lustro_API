**Please note the API is currently in its beta stage. I'm still working on a list of functionality updates.**

**Edit: 27.10.26. I will be uploading a breaking change patch in the coming week or two at most. It'll be the only one before going out of beta. Lustro 1.0.1 will be the initial post-beta release.**
**Edit: 04.11.26. I am aiming for the upgrades to be finished by 13.11.26. A health issue delayed me slightly.**

# Lustro API

Questions? Please see the **[FAQ](https://github.com/Ligatum/Lustro/blob/MAIN/DOCS/FAQ.md)**.

The core mechanism and the API implementation are currently considered non-cryptographic.

## Get the Code

```bash
git clone https://github.com/Ligatum/Lustro_API.git
cd Lustro_API
```

## Build

```bash
# Rust

cargo build --release

# rand_core traits for LustroPrng and LustroXof (Rust, also enables entropy)

cargo build --release --features rand

# Python 3.11-3.14

pip install maturin numpy
maturin develop --release

# C/C++ FFI bindings

cargo build --release --features ffi

```

In the release profiles (`panic = "unwind"`) the C FFI boundary catches internal panics and returns `LustroError::InternalPanic`; the Python module raises them as `pyo3_runtime.PanicException` (a `BaseException`). Direct Rust calls panic as usual, for example on a length mismatch.

NumPy is needed only by the functions that take or return arrays (`hash*_many*`, `fill_blocks`, `blocks`, `fill_u64`); without it they raise `ImportError`. The install command above includes NumPy; a package built from the repo also has it as the `numpy` extra.

The Python module declares that it requires the GIL (`gil_used = true`).
On free-threaded CPython builds, importing the module enables the GIL
unless this behavior is explicitly overridden (`PYTHON_GIL=0` or `-X gil=0`).
Running Lustro with the GIL disabled is not supported.

## Speed Test

The repo also contains **Lustro_Speed_Tester.py**. It runs equivalent sections on native Rust, Python and C layers. The results can be compared.

`````bash
# rust target (built on request only, no other build includes it)
cargo build --release --example speed
`````

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
use lustro::{hash256, Hash256};

let digest = hash256(b"hello world");

println!("{digest}");     // lowercase hex, 64 digits
println!("{digest:X}");   // uppercase
println!("{digest:#x}");  // with a 0x prefix

let parsed: Hash256 = format!("{digest:#x}").parse()?;
assert_eq!(parsed, digest);
```

Digests are `Hash` and `Ord`, so they work as keys in `HashMap` and `BTreeMap`. `Ord` compares the bytes lexicographically, not as a number. Conversion to and from `[u8; N]` is available through `From` / `Into`. In Python a digest is `bytes`, so use `digest.hex()`.

`parse()` reads a digest back from hex: 64 digits for `Hash256`, 32 for `Hash128`, either case, with an optional `0x` or `0X` prefix. Spaces are not trimmed. `ParseHashError` says whether the length or a digit is wrong. `Hash256::first_128()` returns the first 16 bytes as a `Hash128`, the same value `hash128` returns for that message. In Python use `bytes.fromhex(s)`.

### Incremental Hash

A message that is not in memory at once, such as a large file, can be hashed in pieces. The length of the whole message goes into the initial state, so it has to be known before the first byte. Pass it to the hasher; `update` then takes the bytes in pieces of any size, and the digest equals `hash256` of the whole message.

**Rust**

```rust
use lustro::{hash256, hash256_reader, LustroHasher};
use std::fs::File;

let mut h = LustroHasher::new(11);
h.update(b"hello ")?;
h.update(b"world")?;
assert_eq!(h.finalize()?, hash256(b"hello world"));

// A file, read in 8 KiB pieces.
let file = File::open("data.bin")?;
let len = file.metadata()?.len();
let digest = hash256_reader(len, file)?;
```

**Python**

```python
import os
import lustro

h = lustro.LustroHasher(11)
h.update(b"hello ")
h.update(b"world")
assert h.digest() == lustro.hash256(b"hello world")

h = lustro.LustroHasher(os.path.getsize("data.bin"))
with open("data.bin", "rb") as f:
    while chunk := f.read(1 << 16):
        h.update(chunk)
digest = h.digest()
```

**C / C++**

```c
#include "lustro.h"

LustroHasher *h = NULL;
lustro_hasher_new(11, &h);
lustro_hasher_update(h, (const uint8_t *)"hello ", 6);
lustro_hasher_update(h, (const uint8_t *)"world", 5);

uint8_t digest[LUSTRO_HASH256_LEN];
lustro_hasher_finalize256(h, digest);
lustro_hasher_free(h);
```

- **Length:** a piece that would go past the declared length is rejected whole (`HashError::TooMuchData` in Rust, `ValueError` in Python, `LUSTRO_ERROR_INVALID_LENGTH` in C/C++) and nothing of it is absorbed. Asking for the digest early fails the same way (`HashError::TooLittleData`), and more pieces can follow. `remaining()` (`lustro_hasher_remaining`) tells how many bytes are still expected.
- **Digest:** reading it does not end the hasher; every call gives the same value. Its first 16 bytes are `hash128` of the message.
- **Reader:** `hash256_reader(len, reader)` reads exactly `len` bytes and leaves the rest in the reader (pass `&mut reader` to keep it). A reader that ends early gives `ErrorKind::UnexpectedEof`.
- **Copy:** `clone()` in Rust, `copy()` in Python and `lustro_hasher_clone` in C/C++ continue independently, for example to hash several messages of the same length that share a prefix.

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

`StreamId` wraps a `u128`, so `StreamId(5)` works. `StreamId::from` takes `u8`, `u16`, `u32`, `u64` and `u128` values; `StreamId::from(5)` does not compile, because an untyped literal is an `i32`.

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

### Incremental XOF

`LustroXofAbsorber` is the XOF counterpart of the hasher from [Incremental Hash](#incremental-hash): give it the total length, feed the message with `update`, and `finalize` returns the `LustroXof` of the whole message. `LustroXof::from_reader` does the same from a reader, like `hash256_reader`.

**Rust**

```rust
use lustro::{LustroXof, LustroXofAbsorber};
use std::fs::File;

let mut a = LustroXofAbsorber::new(11);
a.update(b"hello ")?;
a.update(b"world")?;
let mut xof = a.finalize()?;
assert_eq!(xof.next_u64(), LustroXof::new(b"hello world").next_u64());

// A file, read in 8 KiB pieces.
let file = File::open("data.bin")?;
let len = file.metadata()?.len();
let mut xof = LustroXof::from_reader(len, file)?;
```

**Python**

```python
import lustro

a = lustro.LustroXofAbsorber(11)
a.update(b"hello ")
a.update(b"world")
xof = a.finalize()
assert xof.fill(64) == lustro.LustroXof(b"hello world").fill(64)
```

**C / C++**

```c
#include "lustro.h"

LustroXofAbsorber *a = NULL;
lustro_xof_absorber_new(11, &a);
lustro_xof_absorber_update(a, (const uint8_t *)"hello ", 6);
lustro_xof_absorber_update(a, (const uint8_t *)"world", 5);

LustroXof *xof = NULL;
lustro_xof_absorber_finalize(a, &xof);
lustro_xof_absorber_free(a);

uint64_t value;
lustro_xof_next_u64(xof, &value);
lustro_xof_free(xof);
```

- **Errors:** as for the hasher. `finalize` before the declared length is reached fails (`HashError::TooLittleData`, `ValueError`, `LUSTRO_ERROR_INVALID_LENGTH` with `*out_ctx` set to `NULL`), and more pieces can follow.
- **Stream:** `finalize` does not end the absorber; each call returns a new stream at position 0. In C it is an ordinary `LustroXof` context, freed with `lustro_xof_free`, also after the absorber is freed.

### Random Seed

**Rust** (`--features entropy`, also enabled by `rand`, `python` and `ffi`)

```rust
use lustro::{LustroPrng, StreamId};

// Err(EntropyError) if the operating system cannot provide entropy.
// Stream 0 of a fresh seed; the seed is not kept.
let mut rng = LustroPrng::from_entropy()?;

// Stream 5 of another fresh seed; unrelated to `rng`.
let mut other = LustroPrng::from_entropy_with_id(StreamId(5))?;
```

To keep the seed, take it first: `Seed256::from_entropy()?`, then `LustroPrng::new(&seed, StreamId(0))`.

**Python**

```python
from lustro import LustroPrng

rng = LustroPrng.from_entropy()  # OSError if the system source is unavailable
other = LustroPrng.from_entropy(stream_id=5)  # another fresh seed, unrelated to `rng`
```

The seed is not kept. To keep it, use `LustroPrng(os.urandom(32), 0)`.

**C / C++**

`lustro_seed_from_entropy` fills a 32-byte seed from the system's entropy source;
`lustro_prng_from_entropy` creates a PRNG directly (stream ID as `hi`, `lo`, the seed is not kept):

```c
LustroPrng *rng = NULL;
LustroError err = lustro_prng_from_entropy(0, 0, &rng);
if (err != LUSTRO_ERROR_OK) {
    /* LUSTRO_ERROR_ENTROPY_UNAVAILABLE: the system entropy source is unavailable */
}

/* To keep the seed, take it first */
uint8_t seed[LUSTRO_SEED_LEN];
LustroPrng *kept = NULL;
err = lustro_seed_from_entropy(seed);
if (err == LUSTRO_ERROR_OK) {
    err = lustro_prng_new(seed, 0, 0, &kept);
}

lustro_prng_free(kept);
lustro_prng_free(rng);
```

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

rng.advance_bytes(50);   // skip 50 bytes
rng.advance_blocks(2);   // skip 2 blocks

let position: u128 = rng.position();
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

rng.advance_bytes(50)           # skip 50 bytes
rng.advance_blocks(2)           # skip 2 blocks

position = rng.position()       # int
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

lustro_prng_advance_bytes(rng, 50);   /* skip 50 bytes */
lustro_prng_advance_blocks(rng, 2);   /* skip 2 blocks */

uint8_t position[16];                 /* u128, 16 bytes little-endian */
lustro_prng_position(rng, position);

lustro_prng_free(rng);
```

`LustroXof` reads the same way: use a `LustroXof` in place of `rng` (Rust and Python), or the `lustro_xof_*` functions in C/C++ (`lustro_xof_next_u32`, `lustro_xof_next_u64`, `lustro_xof_next_u128`, `lustro_xof_next_block`, `lustro_xof_fill`).

All values are little-endian and taken from the same byte stream: in the examples above `a` is bytes 0–3, `b` bytes 4–11 and `c` bytes 12–27. `next_block()` always advances to a full new 32-byte block and discards the unread rest of the current one.

`advance_bytes(n)` skips `n` bytes and `advance_blocks(n)` skips `n` blocks, leaving the stream where `fill` of `n` bytes or `n` calls of `next_block()` would. Both cost O(n); there is no jump-ahead. `position()` returns the bytes taken so far, and the discarded rest of a block counts as taken. They exist for `LustroPrng` and `LustroXof` in Rust, Python and C/C++ (`lustro_prng_advance_bytes`, `lustro_prng_advance_blocks`, `lustro_prng_position` and the same for `lustro_xof_*`). In C/C++ `n` is a `uint64_t`, and the position is written as 16 bytes, little-endian; `lustro_u128_to_parts` splits it into `(hi, lo)`.

In Rust, `LustroPrng` and `LustroXof` also implement `std::io::Read`. `read` fills the whole buffer and returns its length, exactly like `fill_bytes`. The stream has no end, so `read_to_end` and `read_to_string` return an `InvalidInput` error and read nothing; bound the read, for example with `take`. `io::copy` and `bytes()` without a limit are not stopped, so bound those too.

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

Derive a child stream from the current state and a 128-bit identifier. The parent is not changed. The child depends on the parent's current block, not on the position inside it: forking after 1 byte or after 31 bytes of the same block gives the same child.

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

In C/C++ a 128-bit ID goes in as two `uint64_t`, `hi` first, and `next_u128` returns 16 bytes, little-endian. `lustro_u128_from_parts(hi, lo, out)` and `lustro_u128_to_parts(in, &hi, &lo)` from `lustro.h` convert between the two forms.

The same parent state and identifier always give the same child. Reading more bytes from the block the parent is already in does not change the child; moving the parent on to a later block changes the state it is derived from. The child starts at its own first block and does not inherit the parent's position.

A 32-byte output block is the stream's state at that point, and `fork()` derives from that state. So once a PRNG has generated a block, `fork(id)` equals `LustroPrng::new` with that block as the seed and the same identifier.

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

`path` must not be empty: `derive_path` panics in Rust (`try_derive_path` returns an error), Python raises `ValueError`, and C/C++ returns `LUSTRO_ERROR_INVALID_LENGTH`. The same holds for the batch versions below.

---

### Fork into a Batch

Derive many children of one stream in a single call. `fork_batch(ids)` returns a batch whose lane `i` is `fork(ids[i])`, and `fork_batch_range(first, count)` uses the IDs `first`, `first + 1`, ... `fork_path(path)` returns `fork(path[0]).fork(path[1])...` without the intermediate streams. All three start from the current state, like `fork`, and do not change the parent.

**Rust**

```rust
use lustro::{LustroPrng, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let rng = LustroPrng::new(&seed, StreamId(0));

let children = rng.fork_batch(&[StreamId(10), StreamId(20)]); // LustroPrngBatch, 2 lanes
let numbered = rng.fork_batch_range(StreamId(100), 8);        // ids 100 to 107
let leaf = rng.fork_path(&[StreamId(12), StreamId(7)]);       // rng.fork(12).fork(7)
```

**Python**

```python
from lustro import LustroPrng

rng = LustroPrng(bytes(32), 0)

children = rng.fork_batch([10, 20])        # LustroPrngBatch, 2 lanes
numbered = rng.fork_batch_range(100, 8)    # ids 100 to 107
leaf = rng.fork_path([12, 7])              # rng.fork(12).fork(7)
```

**C / C++**

```c
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrng *rng = NULL;
lustro_prng_new(seed, 0, 0, &rng);

uint64_t ids_hi[2] = {0, 0};
uint64_t ids_lo[2] = {10, 20};

LustroPrngBatch *children = NULL;
lustro_prng_fork_batch(rng, ids_hi, ids_lo, 2, &children);

LustroPrngBatch *numbered = NULL;
lustro_prng_fork_batch_range(rng, 0, 100, 8, &numbered);  /* first id as (hi, lo), then count */

uint64_t path_hi[2] = {0, 0};
uint64_t path_lo[2] = {12, 7};

LustroPrng *leaf = NULL;
lustro_prng_fork_path(rng, path_hi, path_lo, 2, &leaf);

lustro_prng_free(leaf);
lustro_prng_batch_free(numbered);
lustro_prng_batch_free(children);
lustro_prng_free(rng);
```

`LustroXof` has the same methods (`lustro_xof_fork_batch`, `lustro_xof_fork_batch_range` and `lustro_xof_fork_path` in C/C++) and returns a `LustroXofBatch`.

For a stream that has not produced output yet, `fork_path` gives what `derive_path` gives: `LustroPrng::new(&seed, StreamId(5)).fork_path(path)` is `derive_path(&seed, [5] + path)`. It is for the cases `derive_path` does not cover: a stream from `from_entropy()`, a lane of a batch, a restored snapshot, or a stream that has already produced output. `path` must not be empty, with the same errors as `derive_path`. If the lanes cannot be allocated, `fork_batch` and `fork_batch_range` panic in Rust (`try_fork_batch` and `try_fork_batch_range` return `BatchError::Reserve`), Python raises `MemoryError`, and C/C++ returns `LUSTRO_ERROR_OUT_OF_MEMORY`.

---

## Batches

### Batch Hash

Hash many messages in one call. `*_many_fixed` takes messages of one length stored back to back in a single buffer; `*_many_var` takes messages of any length.

**Rust**

```rust
use lustro::{
    hash256_many_fixed, hash256_many_fixed_into, hash256_many_var, hash256_many_var_into, Hash256,
};

// Messages of one length, stored back to back.
let data = [0u8; 3 * 16];
let fixed = hash256_many_fixed(&data, 16);

// The same, written into a slice you provide.
let mut out = vec![Hash256::default(); 3];
hash256_many_fixed_into(&data, 16, &mut out);

// Messages of any length.
let messages = [b"a".as_ref(), b"bc".as_ref(), b"".as_ref()];
let digests = hash256_many_var(&messages);

// The same, written into a slice you provide.
hash256_many_var_into(&messages, &mut out);
```

`hash256_many_fixed` returns a `Vec` and panics if `message_len` is 0 or `data.len()` is not a multiple of `message_len`. `hash256_many_fixed_into` panics if `data.len() != out.len() * message_len`, and `hash256_many_var_into` if `messages.len() != out.len()`.

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

Both variants can use AVX2 for batches of 4 or more messages and send large batches to the thread pool, as far as the CPU and the runtime settings allow. The digests do not depend on the backend or on the number of threads.

---

### Batch PRNG and XOF

Many independent streams in one object, for PRNG and XOF alike.

A PRNG or XOF batch produces output in steps: `fill_blocks(out, steps)` writes `steps` blocks for every stream. `steps` only sets how much one call produces. The same total split over several calls gives every stream the same blocks. `suggested_steps()` returns a `steps` chosen for speed at the current number of streams; it is only a hint, any `steps` gives correct output. The output holds `len() * steps` blocks, so with many streams check its size before allocating.

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

Creating a batch allocates its lanes. `LustroPrngBatch::new` and `LustroXofBatch::new` panic if that fails; `try_new` returns `Err(BatchError::Reserve)` instead. Python raises `MemoryError`, and C/C++ returns `LUSTRO_ERROR_OUT_OF_MEMORY`.

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

`lustro_*_batch_len` returns 0 for an empty batch and also for `NULL`. `lustro_*_batch_get_len(ctx, &len)` tells the two apart: it returns `LUSTRO_ERROR_INVALID_POINTER` for `NULL` and writes nothing.

In C/C++, `lustro_batch_block_offset(step, batch_len, lane)` from `lustro.h` gives the byte offset of one block in the output: `(step * batch_len + lane) * LUSTRO_BLOCK_LEN`. It expects `step < steps` and `lane < batch_len` and does not check its arguments.

```c
/* lane 2 at step 0 of the PRNG output above */
const uint8_t *block = out + lustro_batch_block_offset(0, 4, 2);
```

---

### Batch into Single Lane

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
same = prng[1]       # the same lane; prng[-1] is the last one
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

The result is an ordinary single stream (`LustroPrng` or `LustroXof`), so it supports typed reads, `fork` and snapshots.

In Python, `batch[i]` does what `batch.lane(i)` does, and a negative `i` counts from the end (`batch[-1]` is the last lane). `for lane in batch` goes through all lanes in order. Each lane is a copy, and the loop uses the lane states as they were when it started, so later calls on the batch do not change it. Slices are not supported, and `lane(i)` itself still rejects a negative `i`.

---

### Single Streams into Batch

Build a batch from single streams, one lane per stream, in order. Each lane continues its stream at the next block, so `batch.lane(i)` gives back a copy of stream `i`. The streams are not changed.

A lane holds whole blocks only, so every stream must be at a block boundary: freshly created, or read in whole 32-byte blocks. A stream in the middle of a block is rejected rather than moved on, so no bytes are dropped. Read or skip the rest of the block first: a stream whose `position()` is not a multiple of 32 is inside a block, and `32 - position() % 32` more bytes bring it to the next boundary.

**Rust**

```rust
// PRNG
use lustro::{LustroPrng, LustroPrngBatch, Seed256, StreamId};

let seed = Seed256::from_bytes([0u8; 32]);
let a = LustroPrng::new(&seed, StreamId(0));
let mut b = LustroPrng::new(&seed, StreamId(1));
b.next_u64();   // b is now inside a block

// Err(BatchError::PartialBlock { index: 1 }); from_streams would panic.
assert!(LustroPrngBatch::try_from_streams(&[a.clone(), b.clone()]).is_err());

b.advance_bytes(24);   // the rest of the block
let batch = LustroPrngBatch::from_streams(&[a, b]);
```

```rust
// XOF
use lustro::{LustroXof, LustroXofBatch};

let a = LustroXof::new(b"msg0");
let mut b = LustroXof::new(b"msg1");
b.next_block();   // a whole block, b stays at a block boundary

let batch = LustroXofBatch::from_streams(&[a, b]);
```

**Python**

```python
# PRNG
from lustro import LustroPrng, LustroPrngBatch

a = LustroPrng(bytes(32), 0)
b = LustroPrng(bytes(32), 1)
b.next_u64()          # b is now inside a block; from_streams would raise ValueError

b.advance_bytes(24)   # the rest of the block
batch = LustroPrngBatch.from_streams([a, b])
```

```python
# XOF
from lustro import LustroXof, LustroXofBatch

a = LustroXof(b"msg0")
b = LustroXof(b"msg1")
b.next_block()

batch = LustroXofBatch.from_streams([a, b])
```

**C / C++**

```c
/* PRNG */
#include "lustro.h"

uint8_t seed[LUSTRO_SEED_LEN] = {0};
LustroPrng *a = NULL;
LustroPrng *b = NULL;
lustro_prng_new(seed, 0, 0, &a);
lustro_prng_new(seed, 0, 1, &b);

const LustroPrng *streams[] = {a, b};
LustroPrngBatch *batch = NULL;
LustroError err = lustro_prng_batch_from_streams(streams, 2, &batch);  /* LUSTRO_ERROR_INVALID_ARGUMENT if a stream is inside a block */

lustro_prng_batch_free(batch);
lustro_prng_free(b);
lustro_prng_free(a);
```

```c
/* XOF */
#include "lustro.h"

LustroXof *a = NULL;
LustroXof *b = NULL;
lustro_xof_new((const uint8_t *)"msg0", 4, &a);
lustro_xof_new((const uint8_t *)"msg1", 4, &b);

const LustroXof *streams[] = {a, b};
LustroXofBatch *batch = NULL;
LustroError err = lustro_xof_batch_from_streams(streams, 2, &batch);

lustro_xof_batch_free(batch);
lustro_xof_free(b);
lustro_xof_free(a);
```

The streams can come from different seeds or messages, and the same stream can appear more than once. A stream inside a block gives `BatchError::PartialBlock { index }` from `try_from_streams` in Rust (`index` is the first such stream; `from_streams` panics), `ValueError` in Python and `LUSTRO_ERROR_INVALID_ARGUMENT` in C/C++. If the lanes cannot be allocated, Rust returns `BatchError::Reserve` (or panics), Python raises `MemoryError` and C/C++ returns `LUSTRO_ERROR_OUT_OF_MEMORY`. In C/C++ the streams stay owned by the caller and are freed separately.

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

// Return an error instead of panicking if the lanes cannot be allocated
// (`try_fork` also for a wrong number of ids).
let one_each = prng.try_fork(&[StreamId(10), StreamId(11), StreamId(12), StreamId(13)])?;
let children = prng.try_fork_range(StreamId(100))?;
let many_children = prng.try_fork_many(&[StreamId(100), StreamId(200)])?;
```

```rust
// XOF
use lustro::{LustroXofBatch, StreamId};

let messages = [b"msg0".as_ref(), b"msg1".as_ref(), b"msg2".as_ref()];

let xof = LustroXofBatch::new(&messages);
let one_each = xof.fork(&[StreamId(10), StreamId(11), StreamId(12)]);
let children = xof.fork_range(StreamId(100));
let many_children = xof.fork_many(&[StreamId(100), StreamId(200)]);

// Return an error instead of panicking if the lanes cannot be allocated
// (`try_fork` also for a wrong number of ids).
let one_each = xof.try_fork(&[StreamId(10), StreamId(11), StreamId(12)])?;
let children = xof.try_fork_range(StreamId(100))?;
let many_children = xof.try_fork_many(&[StreamId(100), StreamId(200)])?;
```

**Python**

```python
# PRNG
from lustro import LustroPrngBatch

prng = LustroPrngBatch(bytes(32), [0, 1, 2, 3])

one_each = prng.fork([10, 11, 12, 13])  # MemoryError if the lanes cannot be allocated
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
LustroError err = lustro_prng_batch_fork(prng, ids_hi, ids_lo, 4, &one_each);  /* LUSTRO_ERROR_OUT_OF_MEMORY if the lanes cannot be allocated */

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

`fork(ids)` derives one child per lane, using the identifier at the same position. `ids` must have exactly `batch.len()` elements (`fork` panics and `try_fork` returns `BatchError::LengthMismatch` in Rust, `ValueError` in Python, `LUSTRO_ERROR_INVALID_LENGTH` in C/C++).
`fork_range(first)` derives `batch.len()` children using sequential stream IDs.
`fork_many(ids)` derives `ids.len()` children per lane instead, producing a
batch of `batch.len() * ids.len()` lanes, ordered parent-major: child `j` of
lane `i` is at index `i * ids.len() + j`. Each child starts at step 0.

If the lanes cannot be allocated, `fork`, `fork_range` and `fork_many` panic in Rust; the `try_` variants return an error instead. Python raises `MemoryError`, and C/C++ returns `LUSTRO_ERROR_OUT_OF_MEMORY`.

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

A snapshot holds the full stream state, so keep it as private as the seed. Importing malformed bytes fails: `SnapshotError` from `from_le_bytes` (Rust), `lustro.SnapshotError`, a `ValueError` (Python), or `LUSTRO_ERROR_UNSUPPORTED_VERSION`, `_INVALID_KIND`, `_INVALID_CURSOR` or `_INVALID_LENGTH` (C/C++). A PRNG snapshot cannot be imported as an XOF or the other way round, and a single-stream snapshot cannot be imported as a batch.

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

Export and import allocate buffers sized by the lane count. If that fails, `export_snapshot`, `to_le_bytes` and `from_le_bytes` panic in Rust. `try_export_snapshot` and `try_to_le_bytes` return `Err(BatchError::Reserve)`, and `try_from_le_bytes` returns `Err(SnapshotError::Reserve)`. Python raises `MemoryError`, and C/C++ returns `LUSTRO_ERROR_OUT_OF_MEMORY`.

---

### Runtime Settings

Thread count and SIMD backend follow `LUSTRO_NUM_THREADS`, `LUSTRO_DISABLE_MT` and `LUSTRO_DISABLE_SIMD` by default. `configure` sets them from code, best before first use (see below). They apply to hash batches as well as to PRNG and XOF batches.

`LUSTRO_DISABLE_MT` and `LUSTRO_DISABLE_SIMD` are set by `1`, `true`, `on` or `yes` and are not set by `0`, `false`, `off`, `no` or an empty value (case is ignored). Any other value sets them.

`LUSTRO_NUM_THREADS` takes a positive number, capped at the CPU count. A value that is not a positive number (`abc`, `0`, `-1`) is ignored and the pool gets the full CPU count.

**Rust**

```rust
use lustro::{configure, runtime_info, runtime_preview, RuntimeOptions};
use std::num::NonZeroUsize;

let options = RuntimeOptions::new()
    .threads(NonZeroUsize::new(4).unwrap())
    .allow_simd(true);

configure(options)?;

// Read the settings: runtime_info() fixes them, runtime_preview() does not.
let info = runtime_info();
let preview = runtime_preview();
```

**Python**

```python
import lustro

lustro.configure(threads=4, allow_simd=True)

info = lustro.runtime_info()        # fixes the settings
preview = lustro.runtime_preview()  # does not
print(info["backend"], info["threads"], info["fixed"])
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

The settings are fixed once, by whichever comes first: `configure`, the first hash batch call, batch `fill_blocks`/`blocks`, or `runtime_info()`. `suggested_steps()` and `runtime_preview()` do not fix them.

A later `configure` succeeds if it asks for the same effect as the fixed settings, and fails otherwise: `ConfigError::AlreadyConfigured` (Rust), `RuntimeError` (Python), `LUSTRO_ERROR_ALREADY_CONFIGURED` (C/C++). Same effect means the same SIMD choice and, with the `parallel` feature, the same pool size. For the pool size, `threads` above the CPU count counts as the CPU count, and `threads=1` equals `LUSTRO_DISABLE_MT`. The SIMD choice is compared as given: on a CPU without AVX2, `allow_simd=True` and `False` still differ.

When calls race, one wins and the rest are compared with it. Bad arguments change nothing: `ValueError` for `threads < 1` (Python), `LUSTRO_ERROR_INVALID_ARGUMENT` for an unknown `simd` (C/C++).

`runtime_info()` and `runtime_preview()` (Rust, Python, and `lustro_runtime_info()` / `lustro_runtime_preview()` in C/C++) return the same report. The preview fixes nothing and starts no pool; before the settings are fixed it reads the environment as it is now (`fixed` is false). It may be out of date when it returns.

| Field | Meaning |
|---|---|
| `backend` | `"scalar"` or `"avx2"`, as used for the largest batches |
| `threads` | configured pool size, not running workers; 0 = batches run on the calling thread (also without `parallel`) |
| `fixed` | the settings are fixed; false only in a preview taken before that |
| `parallel_built` | built with the `parallel` feature |
| `simd_available` | the CPU has AVX2; `backend` shows whether it is used |
| `pool_usable` | the pool exists and this process can use it. False before the first parallel batch, after a failed pool build, and in a child forked after its parent started the pool |
| `logical_cpus` | logical CPUs at the call; the cap for `threads` |

In C/C++, `lustro_runtime_info(&info, sizeof info)` fills a `LustroRuntimeInfo` and fixes the settings like `runtime_info()`; `lustro_runtime_preview(&info, sizeof info)` fills it and fixes nothing. Both return `LUSTRO_ERROR_OK`, `LUSTRO_ERROR_INVALID_POINTER` for a NULL `info`, or `LUSTRO_ERROR_INVALID_LENGTH` if the size is too small. The struct has the fields above; the flags are 0 or 1 and `backend` is a static string (do not free it). `lustro_runtime_threads()` and `lustro_runtime_backend()` fix the settings like `runtime_info()`.

```c
LustroRuntimeInfo info;
if (lustro_runtime_preview(&info, sizeof info) == LUSTRO_ERROR_OK) {
    printf("%s, %zu threads, fixed=%d\n", info.backend, (size_t)info.threads, info.fixed);
}
```

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

Stream IDs and path elements are Python ints from 0 to `2**128 - 1`; a negative or larger value raises `OverflowError`. A list of them can be any sequence, for example a `tuple`, a `range` or a NumPy integer array.

All four classes can be pickled, for example to pass them to a worker process. A pickle holds the same data as `export_snapshot()`, so the restored object continues from the same position. It is a copy, like `copy()`, not a new stream: workers that need independent streams should use `fork()` or different stream IDs. Pickles contain generator state and, as always, must not be loaded from untrusted sources. They refer to the class as `lustro.LustroPrng` and so on; to store state long term, use `export_snapshot()`.

An instance must not be used by several threads at the same time (an overlapping call raises `RuntimeError`). Use `copy()`, `fork()` or separate instances instead. The installed version is available as `lustro.__version__`, and the package ships type stubs (`lustro.pyi`, `py.typed`).

---

### GIL and Throughput (Python)

| Call | GIL |
|---|---|
| `hash256`, `hash128` | released for inputs of 4096 bytes or more, held below |
| `hash256_many_fixed`, `hash128_many_fixed` | released; the input is copied first (`hash*_many_fixed_into` with `release_gil=False` reads in place) |
| `hash*_many_fixed_into` | released by default, held with `release_gil=False` |
| `hash*_many_var`, `hash*_many_var_into` | released by default, held with `release_gil=False` |
| `fill_blocks` (`LustroPrngBatch`, `LustroXofBatch`) | released by default, held with `release_gil=False` |
| `fill`, `fill_u64`, `blocks` | released while generating |
| `advance_bytes`, `advance_blocks` | released while skipping |
| `fill_into`, `readinto` | released for buffers of 4096 bytes or more, held below |
| `LustroHasher.update`, `LustroXofAbsorber.update` | released for inputs of 4096 bytes or more, held below |

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

- Not every input is read in place. In `hash*_many_var` and `hash*_many_var_into` items that are not `bytes` are copied; an exact `list` or `tuple` of exact `bytes` (no subclasses) is the fastest input, especially for many small messages. In `hash*_many_fixed_into` a `data` view that is not C-contiguous (for example `a[:, :16]`) is copied and hashed with the GIL released.
- `out` must not share memory with the input. The same array, or views of one array, as `data` and `out` raise `TypeError` in both modes. Memory shared through separate wrappers (for example a `memoryview` of `out` used as a message) raises `ValueError` here; by default the input is copied first and the call is allowed.
- When a `list` or `tuple` of 16384 or more items is given to `hash*_many_var_into` and the batch runs on the thread pool, it is hashed while it is read, so `out` may be partly written if the call then fails. By default nothing is written before all items are read.
- `fill_blocks` writes in place on little-endian targets; elsewhere the flag has no effect.

---

### rand_core Integration (Rust)

With `--features rand`, `LustroPrng` and `LustroXof` implement the `rand_core` 0.10
traits `TryRng` and `Rng` (infallible, `Error = Infallible`). `LustroPrng` also
implements `SeedableRng`. `LustroXof` does not: it is built from a message of any
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
let mut random = LustroPrng::from_entropy()?;
let mut buf = [0u8; 100];
random.fill_bytes(&mut buf);

// XOF over a message of any length.
let mut xof = LustroXof::new(b"message");
let b = draw(&mut xof);

// On a LustroPrng, fork takes a StreamId.
let mut child = prng.fork(StreamId(1));
let c = child.next_u64();

// Clone keeps the exact state: both continue with the same bytes.
let mut twin = prng.clone();
assert_eq!(draw(&mut prng), draw(&mut twin));

// Derive a stream from a seed (or message) and a path of identifiers.
let seed = Seed256::from_bytes([7u8; 32]);
let mut leaf = LustroPrng::derive_path(&seed, &[StreamId(12), StreamId(7)]);
let mut leaf_xof = LustroXof::derive_path(b"message", &[StreamId(12), StreamId(7)]);
```

- **Same stream:** `Rng` returns the same bytes, from the same position, as the
  inherent `next_u32`, `next_u64` and `fill_bytes`.
- **Seeding:** `from_seed` starts at stream 0. `rand_core` 0.10 has no
  `from_entropy`; use `LustroPrng::from_entropy()?`.
- **`fork`:** on a concrete `LustroPrng`, `prng.fork(id)` is the inherent
  `LustroPrng::fork(StreamId)`. The trait method `SeedableRng::fork()` runs in code
  generic over `SeedableRng` or when called as `SeedableRng::fork(&mut prng)`; it
  seeds a new generator from the next 32 bytes of `prng`.
- **Clone and `derive_path`:** both exist on `LustroPrng` and `LustroXof` and work
  without the `rand` feature. A clone repeats the same bytes, so do not use it to get
  a second independent stream; use `fork` or `derive_path` for that. `derive_path` is
  not a trait method, so generic code over `Rng` cannot call it; `clone` needs a
  `Clone` bound.
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