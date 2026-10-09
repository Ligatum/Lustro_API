# Lustro API V1 — Architecture

This document describes the architecture, execution semantics, and
binding references for Lustro. For a quick start, see [README](README.md).

For binding details please see [Rust API](#121-rust-api-pure-rust-no-bindings),
[Python Bindings](#122-python-bindings), and [C FFI Reference](#123-c-ffi-reference).

---

## 1. Architecture at a Glance

### Design Principles

- Single internal transformation engine shared by all public modules.
- Minimalism, modularity and a straightforward pipeline. Simple things work, when organized.
- Stateless (Hash) and stream-oriented (PRNG, XOF) execution modes.
- Explicit domain separation for every public primitive via domain tags.

### Diagram

```text
                    Core Transformation
                 evaluate_scalar() / stream_step()
                              │
              ┌───────────────┴───────────────┐
              │                                │
        Absorption Layer                Branch Derivation
      absorb_with_domain()          prepare_base + derive_branch_stream
              │                                │
   ┌──────────┴──────────┐          ┌──────────┴──────────┐
   │                     │          │                     │
Domain::Hash          Domain::Xof   Seed + StreamId    Current state + id
   │                     │          (Domain::Prng)         (fork)
   ▼                     ▼             │                     │
 Hash128/             StreamState      ▼                     ▼
 Hash256                  │        StreamState           StreamState
   │                      ▼                                (child)
   ▼                    XOF             │
  Hash              (stream_step()      ▼
                      per refill)      PRNG
                                   (stream_step()
                                    per refill)


                    ── Batch execution (parallel path) ──

absorb_hash256_batch_into        StreamLane           StreamLane
absorb_hash128_batch_into             │                    │
        │                             │                    │
dispatch_hash256_batch_into      dispatch_streams()  dispatch_streams()
dispatch_hash128_batch_into           │                    │
        │                             │                    │
    Hash Batch                   PRNG Batch           XOF Batch
                                                    (independent absorption,
                                                     lockstep dispatch)
```

All public primitives use the same transformation semantics through
`evaluate_scalar()` and `stream_step()`. Batch stream execution may use
an AVX2 implementation of the stream-step computation on supported
`x86_64` CPUs; the scalar path remains the reference execution path.
`absorb_with_domain()` and `StreamState` are built on top of these semantics.

---

## 2. Core Model

- The state is 256-bit, represented as `(s0: u128, s1: u128)`.
- The state is evaluated in two modes:
  - **Stateless** — `evaluate_scalar()`, a single IDM + ERD with `step = 0`.
  - **Stateful** — `stream_step()`, the same IDM + ERD driven by a
    step counter, called once per output block.
- Hash and XOF both *initialize* their state via absorption
  (`absorb_with_domain`); PRNG initializes via seed + identifier derivation.
- PRNG and XOF *consumption* (byte/block output) both go through the same
  `StreamState`.

See [§5 Engine](#5-engine) for engine details.
Absorption padding is described in [§4 Absorption / Hash & XOF-Init Model](#4-absorption--hash--xof-init-model).

---

## 3. Domain Separation

Each construction uses a distinct domain tag, XORed into the initial state:

```text
Domain::Hash = 0x01
Domain::Prng = 0x02
Domain::Xof  = 0x03
```

Hash, PRNG, and XOF each use a distinct domain. `fork()` reuses the parent's
domain (`Domain::Prng` for PRNG and `Domain::Xof` for XOF).

---

## 4. Absorption / Hash & XOF-Init Model

Hash and XOF share the same absorption mechanism, `absorb_with_domain()`;
they differ in what happens after absorption.

```text
Hash: message → absorption → digest (stateless, one-shot)
XOF:  message → absorption → StreamState → stream_step() → output (stateful)
```

**Absorption mechanics** (`absorb_with_domain`, `finalize_terminator`):

- The domain tag is XORed into `s0`, and the encoded bit-length is XORed into `s1`.
- Input is consumed in 32-byte blocks. Each block is XORed into `(s0, s1)`,
  evaluated with `evaluate_scalar`, and combined with the previous state using
  feed-forward.
- If the input is a non-zero multiple of 32 bytes, no terminator round is performed.
- Otherwise, the final block is padded with `0x80` followed by zeros and processed
  with the same feed-forward rule. If the remaining input is shorter than 16 bytes,
  `b0` is also XORed into `s1`.

**Hash finalization**: `Hash256` is raw `(s0 || s1)` state as 32
little-endian bytes; `Hash128` serializes only `s0` — see
[§10 Behavioral Guarantees](#10-behavioral-guarantees).

**XOF initialization**: the absorbed `(s0, s1)` becomes the initial
`StreamState`. All further output (`next_block`, `fill_bytes`, ...) uses the
same stream mechanics as PRNG — see [§6](#6-stream-model-streamstate).

---

## 5. Engine

Internal engine functions — all `pub(crate)`, not part of the public API.
Split across `core.rs`, `api.rs`, and `dispatch.rs`.

### Core Transformation

| Function | Description |
|---|---|
| `idm_scalar_ref` | IDM — Initial Diffusion Module; initial state mixing layer. |
| `erd_round_scalar_ref` | ERD — Evolving Representation Dynamics round; state permutation with Weyl counter perturbation. |
| `evaluate_scalar` | Single stateless IDM + ERD evaluation (`step = 0`). Used for message absorption (Hash/XOF) and branch derivation. |
| `stream_step` | Counter-driven IDM + ERD evaluation. Called by `StreamState::refill()` on every PRNG/XOF block advance. |

### SIMD Backend

On `x86_64` the batch stream execution may use AVX2 backend when the CPU
supports it. `core_avx2.rs` processes four independent `StreamLane` at a time.

### Absorption Layer

| Function | Description |
|---|---|
| `absorb_with_domain` | Block-by-block message absorption with domain separation. See [§4](#4-absorption--hash--xof-init-model). |
| `absorb_hash256_batch_into` | Delegates N-message 256-bit batch hashing to `dispatch_hash256_batch_into`; writes directly into caller-provided `&mut [[u8;32]]`. |
| `absorb_hash128_batch_into` | Delegates N-message 128-bit batch hashing to `dispatch_hash128_batch_into`; writes directly into caller-provided `&mut [[u8;16]]`. |

### Branch Derivation Pipeline

This is the shared mechanism behind stream initialization (PRNG) and forking (PRNG/XOF, single and batch):

| Function | Description                                                                                                                                                             |
|---|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `prepare_base` | Stage 1: applies the domain tag and `STREAM_INIT_MASK` to `(s0, s1)`. Accepts either seed-derived state or live stream state and can be reused for sibling derivations. |
| `derive_branch_stream` | Stage 2: XORs the identifier into `base_s1`, then calls `evaluate_scalar`. Returns `(s0, s1)` ready for `StreamState::new()`.                                           |
| `fork_lane` | Per-lane fork helper: `prepare_base` + `derive_branch_stream` on raw `(s0, s1)` pairs. Used by `LustroPrngBatch`/`LustroXofBatch` `fork`/`fork_range`.                  |
| `derive_path_lane` | Applies `fork_lane` once per identifier, in path order. Backs single-stream and batch `derive_path` (PRNG and XOF).                                                     |

### Dispatch Layer

| Function | Description |
|---|---|
| `dispatch_hash256_batch_into` | 256-bit batch evaluation using either the scalar or Rayon path, selected by workload. Writes directly into caller-provided `&mut [[u8;32]]`. |
| `dispatch_hash128_batch_into` | Same scalar/parallel split as the 256-bit variant. It simply omits `s1` serialization. |
| `dispatch_streams` | Advances N independent `StreamLane`s by `steps` rounds each. Output is step-major: `out[step * n + lane]`. Scalar or Rayon path depends on workload size; parallel workers own disjoint lane ranges and run them through all steps. |

Threshold values between scalar and parallel execution are listed in [§9 Execution Policy](#9-execution-policy).

---

## 6. Stream Model (`StreamState`)

Shared component used by both `LustroPrng` and `LustroXof`.

| Method | Description |
|---|---|
| `refill` | Advances the stream by one `stream_step()`; resets cursor to 0. |
| `fill_bytes` | Buffered byte output — continuous cursor; sequential split calls are equivalent to a single call of the concatenated length (see §10). |
| `read_bytes<N>` | Typed read of exactly N bytes (≤ 32); delegates to `fill_bytes`. |
| `read_full_block` | Returns one full 32-byte block; always triggers one engine step and resets the cursor to 32. |
| `fork` | Derives a new `StreamState` via `prepare_base` + `derive_branch_stream`. Cursor is not part of this branch derivation, but it *is* part of single-stream consumption state and is preserved by snapshots (see §8). It does not mutate `self`. |
| `to_parts` | Returns the complete internal state as `(s0: u128, s1: u128, step: u64, cursor: u8)`. Used by `export_snapshot()` on `LustroPrng` / `LustroXof`. |
| `from_parts` | Reconstructs `StreamState` from raw parts. Panics unless `cursor` is in `1..=32`. Used by `import_snapshot()` on `LustroPrng` / `LustroXof`. |

A new `StreamState` starts with `cursor = 32`; the first read triggers `refill()`.

---

## 7. Batch Model (`StreamLane`)

`StreamLane` is the block-oriented batch equivalent of `StreamState`, with minimal 
per-lane state `(s0, s1, step)` and no cursor. Batch output is always block-aligned. 
Each lane advances in full 32-byte blocks, with no byte-level consumption state. 
As a result, batch has no `next_u64()`-style API and its snapshot format has no cursor.

### Single Stream vs Batch

| | Single Stream (`StreamState`) | Batch (`StreamLane`) |
|---|---|---|
| Cursor | yes (1–32 in snapshots) | none (always block-aligned) |
| Output granularity | arbitrary byte count (`fill_bytes`) | fixed 32-byte blocks (`fill_blocks`) |
| State count | 1 | N independent lanes |
| Parallel execution | no | optional, size-dependent (see §9) |
| Snapshot | fixed 56 bytes, includes cursor | variable `16 + N×48` bytes, block-aligned |
| Derivation source | one seed (PRNG) / one message (XOF) | N independent lanes, same seed or N messages |

### Components

| Component | Purpose / API |
|---|---|
| `StreamLane` | Minimal per-lane state layout `(s0, s1, step)`. |
| `LustroPrngBatch` | Multi-stream execution context over a shared seed. `new()` — explicit stream IDs; `new_range()` — sequential stream IDs; `fill_blocks(out, steps)` — advances every lane by `steps` rounds, writing directly into caller-provided output via the dispatcher; `len()` / `is_empty()` — batch geometry; `suggested_steps()` — `steps` hint for `fill_blocks` (see §9). |
| `LustroXofBatch` | Multi-stream execution context derived from independently absorbed messages. Same `fill_blocks()` / `len()` / `is_empty()` / `suggested_steps()` surface as `LustroPrngBatch`; `new(messages)` absorbs each message independently before dispatch. |

`fill_blocks()` writes directly into caller-provided output. Python batch bindings
keep an internal block buffer between calls; `release_buffer()` frees it.
The Rayon thread pool is initialized lazily on the first parallel dispatch
and reused afterwards. If pool creation fails then dispatch silently falls back
to single-threaded execution. A forked child process (PID differs from the
pool owner) also uses the single-threaded path.

### Output Layout

Batch output is step-major. With `n` lanes and `steps` rounds, the block of
lane `i` at step `s` is `out[s * n + i]`:

```text
out[0 .. n]       step 0: lane 0 … lane n-1
out[n .. 2n]      step 1: lane 0 … lane n-1
…
```

---

## 8. Snapshot Model

Snapshots serialize generator state to a stable byte format for later
restoration.

- **Validation order**: snapshot `version` is checked first, then `kind`
  (rejecting e.g. a PRNG snapshot imported as XOF), then — for single-stream
  snapshots — `cursor` bounds (`1..=32`), before any state field is decoded.
  Batch snapshots check the minimum header length first, then `version`,
  `kind`, and finally `lane_count` against the total length.
- **Single-stream** (`LustroPrngSnapshot`, `LustroXofSnapshot`): fixed
  56-byte layout — `version(1) | kind(1) | reserved(6) | s0(16) | s1(16) |
  step(8) | cursor(1) | reserved(7)`.
- **Batch** (`LustroPrngBatchSnapshot`, `LustroXofBatchSnapshot`):
  variable-length — 16-byte header (`version(1) | kind(1) | reserved(6) |
  lane_count(8)`) followed by `lane_count × 48` bytes, each lane encoding
  `s0(16) | s1(16) | step(8) | reserved(8)`. No cursor field — batch lanes are
  always block-aligned.

`SnapshotKind` values: `Prng` = 1, `Xof` = 2, `PrngBatch` = 3, `XofBatch` = 4.
`SnapshotError` values: `UnsupportedVersion`, `InvalidKind`, `InvalidCursor`,
`InvalidLength`.
Format lengths are exposed as constants: `LUSTRO_SNAPSHOT_LEN` (56),
`LUSTRO_BATCH_SNAPSHOT_HEADER_LEN` (16) and `LUSTRO_BATCH_SNAPSHOT_LANE_LEN` (48).
`Debug` output on all four snapshot types is redacted — same convention
as `Seed256` — as snapshots contain internal stream state.
This affects only formatting; `to_le_bytes()` / `from_le_bytes()` are unaffected.

---

## 9. Execution Policy

Thresholds used by the dispatch layer to select the scalar or Rayon path.

```text
Hash parallelization
  Rayon path if (count >= 1664 messages OR total input >= 64 KiB)
  AND count >= 2

Stream parallelization (`fill_blocks`)

Scalar backend:
  parallel work-size threshold: 1536 (lanes × steps)
  parallel chunk size:          32 lanes

AVX2 backend (`x86_64`, AVX2 available):
  minimum lanes for AVX2:       4
  parallel work-size threshold: 2048 (lanes × steps)
  parallel chunk size:          64 lanes
  AVX2 kernel width:            4 lanes

Parallel execution requires at least 2 Rayon work chunks.
```

### Runtime Execution Controls

The dispatcher reads the following environment variables once, on first use:

- `LUSTRO_DISABLE_MT` — disables Rayon parallel execution. Any value counts as set, `0` included.
- `LUSTRO_DISABLE_SIMD` — disables AVX2 stream execution and uses the scalar backend. Any value counts as set, `0` included.
- `LUSTRO_NUM_THREADS=N` — sets the Rayon pool size, capped at the number of logical CPUs. Invalid or zero values are ignored.

These settings affect execution strategy only.

### Suggested steps

`suggested_steps()` on `LustroPrngBatch` / `LustroXofBatch` returns a `steps`
hint for `fill_blocks` based on the lane count. It is a hint only; the same
policy applies to PRNG and XOF.

---

## 10. Behavioral Guarantees

These guarantees hold across Rust, Python, and C FFI layers.

### Stream semantics

- Typed reads (`next_u32`, `next_u64`, `next_u128`) are little-endian, drawn from the same
  underlying byte stream as `fill_bytes`/`fill`/`*_fill`.
- `next_block()` always advances the stream by exactly one engine round and
  returns one 32-byte block, regardless of the cursor position before
  the call — the cursor is simply ignored and reset.
- `fill_bytes()` / `fill()` / `fill_into()` / `*_fill()` have a concatenation
  guarantee: splitting one large request into several sequential calls yields
  the same bytes as a single call, as long as no other stream-consuming call
  (`next_u32`/`next_u64`/`next_u128`/`next_block`) is interleaved.
- Single-stream `export_snapshot()` / `import_snapshot()` (PRNG/XOF) resume
  exactly: the restored context continues from the exported `step` and
  `cursor`. Batch snapshots restore each lane's `(s0, s1, step)` state;
  batch lanes have no cursor (see §7 and §8).

### Fork semantics

- `fork()` (single-stream and batch, PRNG and XOF) always starts the child at
  `step = 0` — the child does not inherit the parent's step counter or cursor.
- The parent stream is unaffected by any number of `fork()` calls.
- Fork is deterministic: the same parent state and identifier produce the same
  child. The identifier is part of the branch derivation; different identifiers
  are intended to produce different derived streams.
- Two forks with the same identifier will be identical regardless of how many
  bytes were consumed from the parent's current output block.
- `fork_many(ids)` (batch only, PRNG and XOF) derives `ids.len()` children per
  lane, producing a batch of `len() * ids.len()` lanes in parent-major order:
  child `j` of lane `i` is at index `i * ids.len() + j`. Same `step = 0` and
  parent-unaffected guarantees as `fork()`. Empty `ids` produces an empty batch.

### Derive-path semantics

- `derive_path` depends only on the seed/message and the path, not on prior
  reads from any stream.
- Equivalent to `new(seed, path[0])` (PRNG) or `new(message)` (XOF) followed
  by `fork()` for each remaining identifier, provided no output is read from
  an intermediate stream.
- Batch `derive_path(seed, roots, path)` (PRNG) / `derive_path(messages, path)`
  (XOF) derives a canonical batch: lane `i` equals the single-stream
  `derive_path` result for root `roots[i]` (PRNG) or message `messages[i]`
  (XOF) followed by `path`. `path` must not be empty; empty `roots`/`messages`
  produces an empty batch.

### Hash semantics

- `hash128()` always equals the first 16 bytes of `hash256()`. Both use the
  same absorption path; `hash128()` only omits serialization of `s1`.

### Batch semantics

- Batch operations preserve lane/stream order end to end.
- `fill_blocks(out, steps)` output is step-major: all lanes for step 0, then
  all lanes for step 1, and so on (`out[step * n + lane]`).
- Consecutive calls concatenate: `steps = a` followed by `steps = b` yields the
  same blocks and the same final state as one call with `steps = a + b`.
- `steps = 0` is a no-op; state is unchanged.
- Execution fallback is transparent: if Rayon pool cannot be initialized,
  execution falls back to the single-threaded path.

### API robustness semantics

- **Misuse is fail-fast** — caller-supplied length/shape mismatches are treated
  as programming errors and may panic in the Rust API; see §12.1 Error Model. 
  This is distinct from malformed *runtime data* (e.g. corrupt snapshot bytes): 
  for APIs that explicitly validate external input, malformed input is reported through 
  `Result`, `LustroError`, or a null-pointer return, rather than being an intentionally 
  exposed panic path. All release builds use `panic = "unwind"`; see §13.
- For an empty `derive_path` path, a `len() * ids.len()` overflow in `fork_many`
  and lane allocation failure, the Rust API also provides `try_*` variants that
  return `Err` instead of panicking (see §12.1).
- **Snapshot kind is validated before decoding** — a PRNG snapshot cannot be
  imported as XOF (or vice versa), and a single-stream snapshot cannot be
  imported as a batch (or vice versa). A mismatch returns an error rather than
  silently producing a corrupt stream. See §8 for validation order.

---

## 11. Public API Surface

At the architecture level, Lustro exposes three primary constructions:

```text
Hash → message → fixed-size digest (stateless)
PRNG → seed + stream ID → deterministic byte stream (stateful)
XOF  → message → deterministic byte stream (stateful)
```

Batch APIs provide grouped execution over multiple independent inputs.
See [§7 Batch Model](#7-batch-model-streamlane).

Specific functions, arguments, and types for each are listed per binding
below.

### Operation Names Across Layers

| Operation | Rust | Python | C |
|---|---|---|---|
| Fill bytes | `fill_bytes(out)` | `fill(size)` → `bytes`, `fill_into(buf)` | `lustro_*_fill(ctx, out, out_len)` |
| Typed reads | `next_u32()`, `next_u64()`, `next_u128()`, `next_block()` | same | `lustro_*_next_u32`, `_next_u64`, `_next_u128`, `_next_block` (write to `out`) |
| Copy | `clone()` | `copy()`, `copy.copy()` | `lustro_prng_clone`, `lustro_xof_clone` (no batch variant) |
| Release | `Drop` | garbage collection | `lustro_*_free` |
| Batch constructor | `new(seed, stream_ids)`, `new_range(...)` | `LustroPrngBatch(seed, stream_ids)`, `new_range(...)` | `lustro_prng_batch_new`, `_new_range` |
| Batch length | `len()`, `is_empty()` | `len()`, `is_empty()`, `len(batch)` | `lustro_*_batch_len` |
| Batch output | `fill_blocks(out, steps)` | `fill_blocks(out, steps)` | `lustro_*_batch_fill_blocks(ctx, out, out_len, steps)` |
| Snapshot | `export_snapshot()` → snapshot type, `to_le_bytes()` | `export_snapshot()` → `bytes` | `lustro_*_export_snapshot`; batch: `_batch_snapshot_size` first |
| Restore | `import_snapshot(snapshot)` | `import_snapshot(bytes)` | `lustro_*_import_snapshot` |
| Hash, batch | `hash256_many(&[&[u8]])` (variable length) | `hash256_many` (2D array, fixed length), `hash256_many_var` | `lustro_hash256_many` (fixed length), `_many_var` |

`fork`, `fork_many`, `fork_range`, `derive_path` and `suggested_steps` have the
same name in all three layers (C functions carry the `lustro_<type>_` prefix).

---

## 12. Language Bindings

### 12.1 Rust API (pure Rust, no bindings)

The main types (`LustroPrng`, `LustroXof`, `LustroPrngBatch`, `LustroXofBatch`,
`Seed256`, `StreamId`, `Hash128`, `Hash256`) and the hash functions are
re-exported at the crate root, e.g. `lustro::LustroPrng`. The module paths
(`lustro::prng::LustroPrng`, `lustro::types::Seed256`) remain valid and refer
to the same items. Snapshot types, `SnapshotError` and `LustroError` are only
available under `lustro::types` and `lustro::errors`.

**Functions**

| Module | Functions                                                                                                                                                                                                                                                                                                                                                                                               |
|---|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| API Version | `lustro_api_version()`                                                                                                                                                                                                                                                                                                                                                                                  |
| Hash | `hash256(message)`, `hash128(message)`, `hash256_many(messages)`, `hash128_many(messages)`, `hash256_many_into(messages, out)`, `hash128_many_into(messages, out)`                                                                                                                                                                                                                                      |
| `LustroPrng` | `new(seed, stream_id)`, `next_u32()`,`next_u64()`, `next_u128()`, `next_block()`, `fill_bytes(out)`, `fork(id)`, `derive_path(seed, path)`, `try_derive_path(seed, path)`, `clone()`, `export_snapshot()`, `import_snapshot(snapshot)`                                                                                                                                                                  |
| `LustroPrngBatch` | `new(seed, stream_ids)`, `new_range(seed, first_stream_id, count)`, `try_new_range(seed, first_stream_id, count)`, `len()`, `is_empty()`, `suggested_steps()`, `fill_blocks(out, steps)`, `fork(ids)`, `fork_many(ids)`, `try_fork_many(ids)`, `fork_range(first)`, `derive_path(seed, roots, path)`, `try_derive_path(seed, roots, path)`, `clone()`, `export_snapshot()`, `import_snapshot(snapshot)` |
| `LustroXof` | `new(message)`, `next_u32()`,`next_u64()`, `next_u128()`, `next_block()`, `fill_bytes(out)`, `fork(id)`, `derive_path(message, path)`, `clone()`, `try_derive_path(message, path)`, `export_snapshot()`, `import_snapshot(snapshot)`                                                                                                                                                                    |
| `LustroXofBatch` | `new(messages)`, `len()`, `is_empty()`, `suggested_steps()`, `fill_blocks(out, steps)`, `fork(ids)`, `fork_many(ids)`, `try_fork_many(ids)`, `fork_range(first)`, `derive_path(messages, path)`, `try_derive_path(messages, path)`, `clone()`, `export_snapshot()`, `import_snapshot(snapshot)`                                                                                                         |

**Types**

| Type | Methods / Traits |
|---|---|
| `Hash128`, `Hash256`, `Seed256` | `as_bytes()`, `AsRef<[u8; N]>`, `AsRef<[u8]>`, `TryFrom<&[u8]>`, `From<[u8; N]>` |
| `Hash128`, `Hash256` (additionally) | `From<Hash> for [u8; N]`, `Display` / `LowerHex` / `UpperHex` (hex of `as_bytes()`, `{:#x}` adds `0x`), `Hash`, `Ord` (lexicographic over bytes) |
| `Seed256` (additionally) | `from_bytes([u8; 32])` |
| `StreamId` | `get()`, `From<u64>`, `From<u128>`, `From<StreamId> for u128`, `Ord` (numeric) |
| `LustroPrngSnapshot`, `LustroXofSnapshot` | `to_le_bytes()` (56 bytes, fixed), `from_le_bytes()`, `TryFrom<&[u8]>` |
| `LustroPrngBatchSnapshot`, `LustroXofBatchSnapshot` | `to_le_bytes()` (variable length), `from_le_bytes()`, `TryFrom<&[u8]>` |
| `SnapshotKind` | `Prng`, `Xof`, `PrngBatch`, `XofBatch` — validated before snapshot state fields are decoded |
| `SnapshotError` | `UnsupportedVersion`, `InvalidKind`, `InvalidCursor`, `InvalidLength`; implements `Display` and `std::error::Error` |
| `DerivePathError` | `EmptyPath`, `Reserve(TryReserveError)` (batch only); `Display`, `std::error::Error`; `#[non_exhaustive]` |
| `BatchError` | `SizeOverflow`, `Reserve(TryReserveError)`; `Display`, `std::error::Error`; `#[non_exhaustive]` |
| Constants (`lustro::types`) | `LUSTRO_SEED_LEN` (32), `LUSTRO_BLOCK_LEN` (32), `LUSTRO_HASH128_LEN` (16), `LUSTRO_HASH256_LEN` (32), `LUSTRO_SNAPSHOT_LEN` (56), `LUSTRO_BATCH_SNAPSHOT_HEADER_LEN` (16), `LUSTRO_BATCH_SNAPSHOT_LANE_LEN` (48) |
| `LustroPrng`, `LustroXof`, `LustroPrngBatch`, `LustroXofBatch` | `Debug` is redacted: single streams show the step, batches show the lane count; `s0`/`s1` are never printed |

**Optional Integrations**

`LustroPrng` implements `rand_core::RngCore` and `rand_core::SeedableRng`
under `feature = "rand"`.

- `SeedableRng::from_seed()`, `From<Seed256>`, and `From<[u8; 32]>` all create
  **stream 0**. Use `LustroPrng::new()` directly when a specific `StreamId` is
  required. `From<Seed256>` and `From<[u8; 32]>` do not require `feature = "rand"`.

**Error Model**

The Rust API has only a few fallible operations: the four snapshot decoders,
the `TryFrom<&[u8]>` conversions for the hash, seed and snapshot types, and the
`try_*` functions listed below.
Everything else is infallible for valid arguments at the API contract level, 
except for documented misuse checks and allocation/size failures that may panic.

| Function(s) | Behavior |
|---|---|
| `LustroPrngSnapshot::from_le_bytes`, `LustroXofSnapshot::from_le_bytes`, `LustroPrngBatchSnapshot::from_le_bytes`, `LustroXofBatchSnapshot::from_le_bytes` | Returns `Result<Self, SnapshotError>`. |
| `Hash128::try_from`, `Hash256::try_from`, `Seed256::try_from` (via `TryFrom<&[u8]>`) | Returns `Result<Self, TryFromSliceError>` on wrong-length input. |
| `hash256_many_into` / `hash128_many_into` | Panics if `messages.len() != out.len()`. |
| `LustroPrngBatch::fill_blocks` / `LustroXofBatch::fill_blocks` | Panics if `out.len() != len() * steps` (or on `usize` overflow). |
| `LustroPrngBatch::fork` / `LustroXofBatch::fork` | Panics if `ids.len() != len()`. |
| `LustroPrngBatch::new_range` | Panics if lane allocation fails. `try_new_range` returns `Err(BatchError::Reserve)` instead. |
| `LustroPrngBatch::fork_many` / `LustroXofBatch::fork_many` | Panics if `len() * ids.len()` overflows `usize`, or if lane allocation fails. `try_fork_many` returns `Err(BatchError::SizeOverflow)` or `Err(BatchError::Reserve)` instead. |
| `LustroPrng::derive_path` / `LustroXof::derive_path` | Panics if `path` is empty. `try_derive_path` returns `Err(DerivePathError::EmptyPath)` instead. |
| `LustroPrngBatch::derive_path` / `LustroXofBatch::derive_path` | Panics if `path` is empty or lane allocation fails. `try_derive_path` returns `Err(DerivePathError::EmptyPath)` or `Err(DerivePathError::Reserve)` instead. |
| Everything else (`new`, `next_*`, `fill_bytes`, single-stream `fork`, `clone`, `export_snapshot`, `import_snapshot(snapshot)`, `fork_range`, `hash256`, `hash128`, `hash256_many`, `hash128_many`) | Infallible — always succeeds for valid Rust-typed arguments. |

Note: these panics are plain Rust panics (`assert_eq!`, `expect`), not `LustroError`
values — `LustroError` is part of the C FFI ABI only (see §12.3).

**Snapshot Roundtrip** (not shown in README Quick Start)

```rust
let snapshot = rng.export_snapshot();
let bytes = snapshot.to_le_bytes();               // 56 bytes

let snapshot2 = LustroPrngSnapshot::from_le_bytes(&bytes)?; // fallible
let mut restored = LustroPrng::import_snapshot(snapshot2);  // infallible
```

---

### 12.2 Python Bindings

**Functions**

| Module | Functions                                                                                                                                                                                                                                                                                                                |
|---|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Module-level | `lustro_api_version()`, `__version__`, size constants `SEED_LEN`, `BLOCK_LEN`, `HASH128_LEN`, `HASH256_LEN`, `SNAPSHOT_LEN`, `BATCH_SNAPSHOT_HEADER_LEN`, `BATCH_SNAPSHOT_LANE_LEN` (same values as the Rust constants)                                                                                                  |
| Hash | `hash256(data)`, `hash128(data)`, `hash256_many(data)`, `hash128_many(data)`, `hash256_many_var(messages)`, `hash128_many_var(messages)`                                                                                                                                                                                 |
| `LustroPrng` | `LustroPrng(seed, stream_id=0)`, `next_u32()`, `next_u64()`, `next_u128()`, `next_block()`, `fill(size)`, `fill_into(buf)`, `copy()`, `fork(id)`, `derive_path(seed, path)`, `export_snapshot()`, `import_snapshot(bytes)`                                                                                               |
| `LustroPrngBatch` | `LustroPrngBatch(seed, stream_ids)`, `new_range(seed, first_stream_id, count)`, `len()`, `is_empty()`, `suggested_steps()`, `release_buffer()`, `copy()`, `fill_blocks(out, steps)`, `fork(ids)`, `fork_many(ids)`, `fork_range(first)`, `derive_path(seed, roots, path)`, `export_snapshot()`, `import_snapshot(bytes)` |
| `LustroXof` | `LustroXof(message)`, `next_u32()`, `next_u64()`, `next_u128()`, `next_block()`, `fill(size)`, `fill_into(buf)`, `copy()`, `fork(id)`, `derive_path(message, path)`, `export_snapshot()`, `import_snapshot(bytes)`                                                                                                        |
| `LustroXofBatch` | `LustroXofBatch(messages)`, `len()`, `is_empty()`, `suggested_steps()`, `release_buffer()`, `copy()`, `fill_blocks(out, steps)`, `fork(ids)`, `fork_many(ids)`, `fork_range(first)`, `derive_path(messages, path)`, `export_snapshot()`, `import_snapshot(bytes)`                                                        |

All four classes support `copy.copy()` / `copy.deepcopy()`; the batch classes
support `len(batch)`.

**Parameter Types**

Unless otherwise noted, parameters for functions not listed here are
plain Python `int`/`bool` with no shape or dtype constraints. Return values
vary by function — e.g. `next_u64()`/`next_u128()` return Python `int`,
`next_block()` returns `bytes`, `hash256_many()` returns a NumPy array;
see the function tables above. NumPy array *parameters* are required only
where noted below: a wrong type or dtype raises `TypeError`, a wrong shape
or layout raises `ValueError`.

NumPy is required for `hash*_many*` and `fill_blocks`. Without it these
calls raise `pyo3_runtime.PanicException` (see §13).

| Function | Parameter | Expected type |
|---|---|---|
| `hash256`/`hash128(data)` | `data` | Python `bytes`, any length (`TypeError` for `bytearray`, `memoryview`, `str`) |
| `hash256_many(data)` | `data` | 2D `numpy.ndarray`, dtype `uint8`, shape `(n, msg_len)`, rows contiguous (`ValueError` otherwise; `TypeError` for wrong type/dtype) → returns shape `(n, 32)`, dtype `uint8` |
| `hash256_many_var(messages)` | `messages` | `list[bytes]`, each element independently sized → returns shape `(n, 32)`, dtype `uint8` |
| `hash128_many(data)` | `data` | 2D `numpy.ndarray`, dtype `uint8`, shape `(n, msg_len)`, rows contiguous (`ValueError` otherwise; `TypeError` for wrong type/dtype) → returns shape `(n, 16)`, dtype `uint8` |
| `hash128_many_var(messages)` | `messages` | `list[bytes]`, each element independently sized → returns shape `(n, 16)`, dtype `uint8` |
| `LustroPrng(seed, stream_id=0)` | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) |
| | `stream_id` | Python `int`, `0 ≤ stream_id < 2¹²⁸` (`ValueError` if negative, `OverflowError` if `≥ 2¹²⁸`); default `0` |
| `LustroPrng.fill(size)` | `size` | Python `int` → returns `bytes` of that length |
| `LustroPrng.fill_into(buf)` | `buf` | writable, C-contiguous buffer of bytes (e.g. `bytearray`, `memoryview`), filled in place to its full length (`TypeError` otherwise) |
| `LustroPrng.fork(id)` | `id` | Python `int`, `0 ≤ id < 2¹²⁸` (`ValueError` if negative, `OverflowError` if `≥ 2¹²⁸`) |
| `LustroPrng.export_snapshot()` | — | returns `bytes`, exactly 56 bytes |
| `LustroPrng.import_snapshot(bytes)` | `bytes` | Python `bytes`, exactly 56 bytes (staticmethod) |
| `LustroPrng.derive_path(seed, path)` | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) (staticmethod) |
| | `path` | Python `list[int]`, non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`ValueError` if negative, `OverflowError` if `≥ 2¹²⁸`) |
| `LustroPrngBatch(seed, stream_ids)` | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) |
| | `stream_ids` | Python `list[int]` |
| `LustroPrngBatch.new_range(seed, first_stream_id, count)` | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) (staticmethod) |
| | `first_stream_id`, `count` | Python `int`, Python `int` |
| `LustroPrngBatch.fill_blocks(out, steps)` | `out`, `steps` | writable, C-contiguous `numpy.ndarray`, shape `(steps, n, 4)`, dtype `uint64`, `n == len(batch)`; Python `int`. Wrong type → `TypeError`; shape mismatch or non-C-contiguous → `ValueError` |
| `LustroPrngBatch.fork(ids)` | `ids` | Python `list[int]`, `len(ids) == batch.len()` |
| `LustroPrngBatch.fork_many(ids)` | `ids` | Python `list[int]` (`ValueError` if `len() * len(ids)` overflows) |
| `LustroPrngBatch.fork_range(first)` | `first` | Python `int` |
| `LustroPrngBatch.derive_path(seed, roots, path)` | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) (staticmethod) |
| | `roots`, `path` | Python `list[int]`; `path` non-empty (`ValueError` if empty), `roots` may be empty (empty batch); each id `0 ≤ id < 2¹²⁸` (`ValueError` if negative, `OverflowError` if `≥ 2¹²⁸`) |
| `LustroPrngBatch.export_snapshot()` | — | returns `bytes`, variable length |
| `LustroPrngBatch.import_snapshot(bytes)` | `bytes` | Python `bytes`, variable length (staticmethod) |
| `LustroXof(message)` | `message` | Python `bytes`, any length (including empty) |
| `LustroXof.fill(size)` | `size` | Python `int` → returns `bytes` of that length |
| `LustroXof.fill_into(buf)` | `buf` | writable, C-contiguous buffer of bytes (e.g. `bytearray`, `memoryview`), filled in place to its full length (`TypeError` otherwise) |
| `LustroXof.fork(id)` | `id` | Python `int`, `0 ≤ id < 2¹²⁸` (`ValueError` if negative, `OverflowError` if `≥ 2¹²⁸`) |
| `LustroXof.export_snapshot()` | — | returns `bytes`, exactly 56 bytes |
| `LustroXof.import_snapshot(bytes)` | `bytes` | Python `bytes`, exactly 56 bytes (staticmethod) |
| `LustroXof.derive_path(message, path)` | `message` | Python `bytes`, any length (staticmethod) |
| | `path` | Python `list[int]`, non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`ValueError` if negative, `OverflowError` if `≥ 2¹²⁸`) |
| `LustroXofBatch(messages)` | `messages` | Python `list[bytes]` — **not** numpy; each element independently sized |
| `LustroXofBatch.fill_blocks(out, steps)` | `out`, `steps` | writable, C-contiguous `numpy.ndarray`, shape `(steps, n, 4)`, dtype `uint64`, `n == len(batch)`; Python `int`. Wrong type → `TypeError`; shape mismatch or non-C-contiguous → `ValueError` |
| `LustroXofBatch.fork(ids)` | `ids` | Python `list[int]`, `len(ids) == batch.len()` |
| `LustroXofBatch.fork_many(ids)` | `ids` | Python `list[int]` (`ValueError` if `len() * len(ids)` overflows) |
| `LustroXofBatch.fork_range(first)` | `first` | Python `int` |
| `LustroXofBatch.derive_path(messages, path)` | `messages` | Python `list[bytes]` (staticmethod) |
| | `path` | Python `list[int]`, non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`ValueError` if negative, `OverflowError` if `≥ 2¹²⁸`) |
| `LustroXofBatch.export_snapshot()` | — | returns `bytes`, variable length |
| `LustroXofBatch.import_snapshot(bytes)` | `bytes` | Python `bytes`, variable length (staticmethod) |

**Python-Specific Notes**

- The following methods release the GIL while the underlying Rust computation
  runs (`py.allow_threads`), allowing other Python threads to run concurrently:
  `hash256`/`hash128`, `hash256_many`/`hash128_many`, `hash256_many_var`/`hash128_many_var`;
  `LustroPrng`/`LustroXof.fill`; `LustroPrngBatch`/`LustroXofBatch.fill_blocks`.
  **`fill_into()` does not release the GIL** — it writes into the caller's buffer
  through an `unsafe` borrow; the GIL prevents concurrent resize or drop of the buffer.
- `import_snapshot(bytes)` raises `ValueError` on malformed input (wrong
  length, unsupported version, or kind mismatch — e.g. a PRNG snapshot passed
  to `LustroXof`), mapping the Rust `SnapshotError` variants.
- Each 32-byte block, where represented as NumPy output, is four `uint64`
  values in little-endian order — not a separate encoding from the raw byte
  stream.
- `hash*_many` copies its rows before hashing; `hash*_many_var` reads the
  `list[bytes]` without copying.
- Batch `fill_blocks` keeps an internal block buffer between calls;
  `release_buffer()` frees it.
- Methods take `&mut self`: overlapping use of one object from several
  threads raises `RuntimeError` ("Already borrowed").
- `repr()` is redacted: `LustroPrng(<redacted>)`, `LustroPrngBatch(len=N)`.

All other behavioral guarantees (fork step=0, hash128 truncation, snapshot
resume, concatenation guarantee) are as described in
[§10 Behavioral Guarantees](#10-behavioral-guarantees).

**Batch Example** (illustrates the NumPy contract; not in README)

```python
import numpy as np
from lustro import LustroPrngBatch

batch = LustroPrngBatch.new_range(seed, 0, 4)
steps = 2
out = np.empty((steps, batch.len(), 4), dtype=np.uint64)
batch.fill_blocks(out, steps)
```

---

### 12.3 C FFI Reference

C ABI exported through `extern "C"` functions, compatible with C and C++.

**Function Table**

| Module | Functions                                                                                                                                                                                                                                                                                                                                                                                                                                  |
|---|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| API Version, errors | `lustro_api_version()`, `lustro_strerror(code)`                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Hash | `lustro_hash256(data, data_len, out)`, `lustro_hash128(data, data_len, out)`                                                                                                                                                                                                                                                                                                                                                               |
| Hash Batch | `lustro_hash256_many(data_ptr, n, message_len, out_ptr)`, `lustro_hash128_many(data_ptr, n, message_len, out_ptr)`, `lustro_hash256_many_var(message_ptrs, n, message_lens, out_ptr)`, `lustro_hash128_many_var(message_ptrs, n, message_lens, out_ptr)`                                                                                                                                                                                   |
| PRNG | `lustro_prng_new(seed, stream_id_hi, stream_id_lo)`, `_free`, `_clone`, `_fill(out, out_len)`, `_next_u32`, `_next_u64`, `_next_u128`, `_next_block`, `_fork(id_hi, id_lo)`, `_derive_path(seed, ids_hi, ids_lo, n)`, `_export_snapshot(out)`, `_import_snapshot(bytes)`                                                                                                                                                                   |
| PRNG Batch | `lustro_prng_batch_new(seed, ids_hi, ids_lo, n)`, `_new_range(seed, first_hi, first_lo, count)`, `_free`, `_len`, `_fill_blocks(out, out_len, steps)`, `_fork(ids_hi, ids_lo, n)`, `_fork_many(ids_hi, ids_lo, k)`, `_fork_range(first_hi, first_lo)`, `_derive_path(seed, roots_hi, roots_lo, n_roots, path_hi, path_lo, n_path)`, `_snapshot_size`, `_suggested_steps`, `_export_snapshot(out, out_len)`, `_import_snapshot(bytes, len)` |
| XOF | `lustro_xof_new(message, message_len)`, `_free`, `_clone`, `_fill(out, out_len)`, `_next_u32`, `_next_u64`, `_next_u128`, `_next_block`, `_fork(id_hi, id_lo)`, `_derive_path(message, message_len, ids_hi, ids_lo, n)`, `_export_snapshot(out)`, `_import_snapshot(bytes)`                                                                                                                                                                 |
| XOF Batch | `lustro_xof_batch_new(message_ptrs, message_lens, n)`, `_free`, `_len`, `_fill_blocks(out, out_len, steps)`, `_fork(ids_hi, ids_lo, n)`, `_fork_many(ids_hi, ids_lo, k)`, `_fork_range(first_hi, first_lo)`, `_derive_path(message_ptrs, message_lens, n_messages, path_hi, path_lo, n_path)`, `_snapshot_size`, `_suggested_steps`, `_export_snapshot(out, out_len)`, `_import_snapshot(bytes, len)`                                      |

`fork_range(ctx, first_hi, first_lo)` variants do not take a count. They
derive `ctx.len()` children with sequential IDs starting at `first`, equivalent
to calling `*_fork` with `ids = [first, first+1, ..., first+len()-1]`.
Likewise, `*_batch_new_range(seed, first_hi, first_lo, count)` is equivalent
to `*_batch_new(seed, ids, count)` with `ids = [first, ..., first+count-1]`.
IDs advance with wrapping `u128` arithmetic, so a range near `u128::MAX`
wraps back to `0`. The same applies to the Rust and Python APIs.

**1. Type Mapping**

| C Type | Rust Origin | Notes |
|---|---|---|
| `uint8_t*` | `*const u8` / `*mut u8` | byte buffer, in or out |
| `uintptr_t` | `usize` | platform-dependent width |
| `uint64_t` | `u64` | `stream_id_hi`/`_lo` components |
| `int32_t` | `LustroError` (`#[repr(i32)]`) | see Return Value Conventions |
| `const char*` | `*const c_char` | static NUL-terminated string, `lustro_strerror()` only; do not free |
| `uint32_t` | `u32` | `lustro_api_version()` return value; `out` of `*_next_u32` |
| opaque pointer | `*mut LustroPrng` / `*mut LustroPrngBatch` / `*mut LustroXof` / `*mut LustroXofBatch` | never dereference from C; pass through only |
| `const uint8_t* const*` | `&[&[u8]]` (array of message slices) | Used by variable-length message APIs such as `*_many_var(...)` and `lustro_xof_batch_new(...)`, together with a parallel `const uintptr_t*` of lengths |

Input/output byte buffers are checked for `NULL`. Pointers to wider element
types (`uint64_t` id arrays, `uintptr_t` length arrays, pointer tables) must
also be naturally aligned; a misaligned pointer is rejected. Byte buffers
(`uint8_t*`) have no alignment requirement. Hash functions
(`lustro_hash256*`/`lustro_hash128*`) additionally
reject overlapping input/output ranges — including, for `*_many_var`, overlap
between `out` and the `message_ptrs`/`message_lens` tables themselves — with
`InvalidPointer`. Scalar writes such as `*_next_u64` use `write_unaligned`.

**2. Calling Convention**

All exported functions use `extern "C"` and expose the platform's standard
C ABI. Do not bind as `stdcall`.

**3. Return Value Conventions**

There are four categories:

- **`LustroError` (`int32`)** — returned by hash, fill, next-value, and
  snapshot-export functions.

  | Value | Name | Meaning |
  |---|---|---|
  | 0 | `Ok` | success |
  | 1 | `InvalidLength` | buffer/length argument invalid |
  | 2 | `InvalidPointer` | required pointer is `NULL`, or (hash functions only) an input/output range overlaps another — see Type Mapping notes above |
  | 3 | `OutputTooSmall` | output buffer smaller than required |
  | 4 | `AlreadyFinalised` | context already finalised |
  | 5 | `VerificationFailed` | verification step failed |
  | 6 | `InternalPanic` | internal panic caught at FFI boundary |

  The enum contains all defined ABI error values, but V1 currently returns only
  `Ok`, `InvalidLength`, `InvalidPointer`, and `InternalPanic`.
  V1 does not distinguish an undersized output buffer from other invalid-length
  arguments. Both return `InvalidLength`; output lengths are checked against
  the exact expected size.

- **Pointer-returning functions** (`*_new`, `*_clone`, `*_fork`,
  `*_fork_many`, `*_fork_range`, `*_derive_path`, `*_import_snapshot`) — `NULL`
  indicates failure; no error code is returned.

- **Value-returning functions** (`*_len`, `*_snapshot_size`, `*_suggested_steps`
  → `uintptr_t`; `lustro_api_version` → `uint32_t`; `lustro_strerror` →
  `const char*`) — value returned directly, no error path. `lustro_strerror(int32_t code)`
  returns a static NUL-terminated description, never `NULL` and not to be freed;
  an unknown `code` gives `"unknown error code"`.

- **void-returning functions** (`*_free`) — no return value; safe to call
  with `NULL` (no-op).

**4. Required Buffer Sizes**

The sizes below are available as `#define`s in `lustro.h`: `LUSTRO_SEED_LEN`,
`LUSTRO_BLOCK_LEN`, `LUSTRO_HASH128_LEN`, `LUSTRO_HASH256_LEN`,
`LUSTRO_SNAPSHOT_LEN`, `LUSTRO_BATCH_SNAPSHOT_HEADER_LEN` and
`LUSTRO_BATCH_SNAPSHOT_LANE_LEN`. A batch snapshot is
`LUSTRO_BATCH_SNAPSHOT_HEADER_LEN + n_lanes * LUSTRO_BATCH_SNAPSHOT_LANE_LEN` bytes.

Output buffers:

| Function family | Buffer | Size |
|---|---|---|
| `lustro_hash256*` | `out` | 32 bytes per message |
| `lustro_hash128*` | `out` | 16 bytes per message |
| `*_next_u32` | `out` | 4 bytes |
| `*_next_u64` | `out` | 8 bytes |
| `*_next_u128` | `out` | 16 bytes |
| `*_next_block` | `out` | 32 bytes |
| `*_export_snapshot` (single-context, PRNG/XOF) | `out` | 56 bytes |
| `*_batch_fill_blocks` | `out` | `n_lanes × steps × 32` bytes — matches `sizeof(...)` on a `[steps][n][32]` array, as in the README batch example |
| `*_batch_export_snapshot` | `out` | `16 + n_lanes × 48` bytes — call `*_batch_snapshot_size(ctx)` first |

Input buffers:

| Function family | Buffer | Size |
|---|---|---|
| `lustro_prng_new` / `lustro_prng_batch_new` / `lustro_prng_batch_new_range` | `seed` | 32 bytes, fixed |
| `lustro_prng_derive_path` / `lustro_prng_batch_derive_path` | `seed` | 32 bytes, fixed |

**5. Ownership / Lifetime**

| Handle type | Created by | Freed by |
|---|---|---|
| `LustroPrng*` | `_new`, `_clone`, `_fork`, `_derive_path`, `_import_snapshot` | `lustro_prng_free` |
| `LustroPrngBatch*` | `_new`, `_new_range`, `_fork`, `_fork_many`, `_fork_range`, `_derive_path`, `_import_snapshot` | `lustro_prng_batch_free` |
| `LustroXof*` | `_new`, `_clone`, `_fork`, `_derive_path`, `_import_snapshot` | `lustro_xof_free` |
| `LustroXofBatch*` | `_new`, `_fork`, `_fork_many`, `_fork_range`, `_derive_path`, `_import_snapshot` | `lustro_xof_batch_free` |

`*_import_snapshot` returns an independent context with the same ownership
and cleanup rules as a context created by `*_new`.

**6. Null / Zero-Length Semantics**

The `n == 0` behavior depends on the return-value category. It does **not**
make every pointer argument optional; only array/data pointers whose length
is determined by `n` may be `NULL`:

- **`LustroError`-returning functions** (`*_many`, `*_many_var`,
  `*_batch_fill_blocks`): with `n == 0`, the `*_many*` functions return
  `LustroError::Ok` without dereferencing the data/output pointers; these
  pointers may be `NULL`.
  `*_batch_fill_blocks` first checks `out_len == n_lanes × steps × 32`
  (so `out_len` must be 0 when the batch is empty or `steps == 0`), then
  returns `Ok` without dereferencing `out`; in that case only `out` may be
  `NULL`. For `n > 0`, a required `NULL` pointer returns `InvalidPointer`.
- **Batch constructors and batch fork operations** (`*_batch_new`,
  `*_batch_new_range`, `*_batch_fork_many`, etc.): with the relevant count
  at 0 (`n`, `count`, or `k` for `*_batch_fork_many`), the function returns a
  valid empty context rather than `NULL`. `*_batch_fork` additionally requires
  `n == len(ctx)`, so `n == 0` succeeds only for an empty batch. For example,
  `lustro_xof_batch_new(..., n = 0)` returns an empty `LustroXofBatch`.
  A `NULL` result otherwise only indicates that no context was created; it
  does not distinguish invalid input from a panic caught by `catch_unwind`.
- **Fixed-size inputs unrelated to `n` still require a valid pointer.**
  For example, `lustro_prng_batch_new(seed, ids_hi, ids_lo, n)` always
  requires a valid 32-byte `seed`; a `NULL` seed returns `NULL`, even when
  `n == 0`. Only the `n`-sized `ids_hi`/`ids_lo` arrays are exempt from the
  `NULL` check when `n == 0`.
- **`*_derive_path` is the one exception to "n == 0 is valid."** Unlike batch
  constructors, `lustro_prng_derive_path` and `lustro_xof_derive_path` treat
  `n == 0` (an empty path) as invalid and return `NULL`, matching the Rust
  panic and the Python `ValueError` for an empty path.
- **`*_batch_derive_path` splits this in two.** `n_roots`/`n_messages` may be
  0 (empty batch, like other batch constructors), but `n_path` follows the
  single-stream rule above and must be nonzero — `NULL` on an empty path.

**7. Thread Safety**

`LustroPrng`, `LustroPrngBatch`, `LustroXof`, and `LustroXofBatch` contain only
plain state (`u128`/`u64`/`u8` fields) and provide no internal synchronization.
All mutating FFI functions operate through `&mut *ctx`. Using the same handle concurrently 
from multiple threads without external synchronization causes a data race. Use a separate 
handle per thread, or synchronize access to handles shared across threads.

**8. Minimal Examples**

**C — hash + basic PRNG:**

```c
uint8_t out[32];
LustroError err = lustro_hash256(data, data_len, out);

LustroPrng *ctx = lustro_prng_new(seed, 0, 0);
uint64_t val;
lustro_prng_next_u64(ctx, &val);
lustro_prng_free(ctx);
```

**PRNG lifecycle with snapshot and fork:**

```c
uint8_t seed[32] = { /* 32 bytes */ };
LustroPrng *ctx = lustro_prng_new(seed, 0, 0);
if (!ctx) { /* handle failure */ }

uint8_t snap[56];
lustro_prng_export_snapshot(ctx, snap);

LustroPrng *restored = lustro_prng_import_snapshot(snap);
LustroPrng *child = lustro_prng_fork(ctx, 0, 42);

lustro_prng_free(child);
lustro_prng_free(restored);
lustro_prng_free(ctx);
```

**Python (ctypes) — input/output buffer mapping:**

This repository's reference Python wrapper maps input byte buffers to
`ctypes.c_char_p` and writable output buffers to
`ctypes.create_string_buffer()`. Other bindings may choose different but
equivalent representations.

```python
lib.lustro_hash256(data, ctypes.c_size_t(data_len), out)

ctx = lib.lustro_prng_new(seed, ctypes.c_uint64(0), ctypes.c_uint64(0))
val = ctypes.c_uint64(0)
lib.lustro_prng_next_u64(ctx, ctypes.byref(val))
lib.lustro_prng_free(ctx)
```

**9. Header Distribution**

`lustro.h` is published with each release. Use the header matching the linked
library version; do not copy or edit it manually between versions.

`lustro.h` is generated by `build.rs` through `cbindgen` when the crate is
built with `--features ffi`. The C FFI module is also gated by this feature;
Python bindings require `--features python`.

**C FFI-Specific Behavioral Notes** (in addition to §10)

- `*_next_u128` writes 16 raw little-endian bytes from the same stream as
  `*_fill`.
- `*_fill` and `*_batch_fill_blocks` preserve the concatenation guarantee from
  §10. Batch output is step-major, so consecutive `*_batch_fill_blocks` calls
  concatenate directly.
- `*_import_snapshot` returns `NULL` for malformed input. Rust and Python
  report the same errors through `SnapshotError` and the corresponding Python
  exception.

---

## 13. Security & Memory Model

1. **`Seed256` is `Copy`** — copying it duplicates the underlying seed bytes.
   This version does not provide secret-memory protection.
2. **No automatic zeroization** — `Seed256` and internal stream state are not
   cleared on drop.
3. **No hidden generator state** — each PRNG/XOF instance owns its own state.
   The Rayon thread pool used for batch parallelism is shared execution state,
   not generator state. If pool creation fails due to system limits, execution
   falls back to single-threaded mode. A process forked after the pool was
   created does not reuse it; the child runs single-threaded.
4. **FFI/Python panic handling.** All release builds use `panic = "unwind"`. 
   FFI functions that are panic-guarded catch internal unwinds and return their 
   documented failure value — `LustroError::InternalPanic` for error-returning 
   functions, or `NULL` for pointer-returning functions. Python bindings require
   `panic = "unwind"` so that Rust panics can be translated at the boundary;
   they surface as `pyo3_runtime.PanicException`, which derives from
   `BaseException`, not `Exception`.
   This does not protect against undefined behavior from invalid non-null pointers, 
   fatal process-level conditions, or builds using a different panic strategy.
   Panic catching does not make invalid non-null pointers safe; violating the 
   FFI pointer preconditions remains undefined behavior.
5. **Thread safety** — the underlying Rust contexts
   (`LustroPrng`, `LustroPrngBatch`, `LustroXof`, `LustroXofBatch`) provide no
   internal synchronization. Shared handles or objects must not be mutated
   concurrently without the synchronization required by the binding.
   For C FFI, this means caller-side synchronization around shared pointers;
   Python also applies the usual GIL and object-borrowing rules.

API robustness properties such as fail-fast misuse handling and snapshot-kind
validation are behavioral contracts, not security properties; see §8 and §10.
Fork, clone, and determinism guarantees are also covered by §10.
