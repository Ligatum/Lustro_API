# Lustro API V1 — Architecture

This document describes the architecture, execution semantics, and
binding references for Lustro. For a quick start, see [README](README.md).

For binding details please see [Rust API](#121-rust-api-pure-rust-no-bindings),
[Python Bindings](#122-python-bindings), and [C FFI Reference](#123-c-ffi-reference).

---

## 1. Architecture at a Glance

### Design Principles

- Single internal transformation engine shared by all public modules.
- Minimalism, modularity and a straightforward pipeline.
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

absorb_hash256_var_into        StreamLane           StreamLane
absorb_hash128_var_into             │                    │
        │                             │                    │
dispatch_hash256_var_into      dispatch_streams()  dispatch_streams()
dispatch_hash128_var_into           │                    │
        │                             │                    │
    Hash Batch                   PRNG Batch           XOF Batch
                                                    (independent absorption,
                                                     lockstep dispatch)
```

Batch stream execution may use an AVX2 implementation of the stream-step
computation on supported `x86_64` CPUs; the scalar path remains the reference
execution path.

---

## 2. Core Model

- The state is 256-bit, represented as `(s0: u128, s1: u128)`.
- The state is evaluated in two modes:
  - **Stateless**: `evaluate_scalar()`, a single IDM + ERD with `step = 0`.
  - **Stateful**: `stream_step()`, the same IDM + ERD driven by a
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

`fork()` reuses the parent's domain (`Domain::Prng` for PRNG and
`Domain::Xof` for XOF).

PRNG and XOF-fork derivation hashes the 48-byte message `state || id`
(`s0` and `s1` little-endian, then the 16-byte identifier) under the domain
tag, with the same absorb step as hashing a message of that length. The state
is the first 32-byte block and the identifier the final, terminated block, so
the identifier goes through the mixing step together with the state. Within one 
input state different identifiers give different inputs to that step. This applies 
to every derivation: `new`, the batch constructors, the roots of `derive_path`, and `fork`.

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
little-endian bytes; `Hash128` serializes only `s0` (see
[§10 Behavioral Guarantees](#10-behavioral-guarantees)).

**XOF initialization**: the absorbed `(s0, s1)` becomes the initial
`StreamState`. All further output (`next_block`, `fill_bytes`, ...) uses the
same stream mechanics as PRNG (see [§6](#6-stream-model-streamstate)).

---

## 5. Engine

The engine functions below are internal (`pub(crate)`), not part of the public API.
Split across `core.rs`, `core_avx2.rs`, `api.rs`, and `dispatch.rs` (with `dispatch/`: `backend`, `config`, `params`, `pool`).

### Core Transformation

| Function | Description |
|---|---|
| `idm_scalar_ref` | IDM (Initial Diffusion Module), the initial state mixing layer. |
| `erd_round_scalar_ref` | ERD (Evolving Representation Dynamics) round: state permutation with Weyl counter perturbation. |
| `evaluate_scalar` | Single stateless IDM + ERD evaluation (`step = 0`). Used for message absorption (Hash/XOF) and branch derivation. |
| `stream_step` | Counter-driven IDM + ERD evaluation. Called by `StreamState::refill()` on every PRNG/XOF block advance. |

### SIMD Backend

On `x86_64` the batch execution may use the AVX2 backend when the CPU
supports it. `core_avx2.rs` processes four independent `StreamLane` at a time
(stream steps) and four messages at a time (hash kernels `hash_fixed_x4`,
`hash_var_x4`).

### Absorption Layer

| Function | Description |
|---|---|
| `absorb_with_domain` | Block-by-block message absorption with domain separation. See [§4](#4-absorption--hash--xof-init-model). |
| `absorb_hash256_fixed_into` | Delegates N-message 256-bit hashing of equal-length messages (stored back to back) to `dispatch_hash256_fixed_into`; writes directly into caller-provided `&mut [[u8;32]]`. |
| `absorb_hash128_fixed_into` | Same for 128-bit digests, into `&mut [[u8;16]]`. |
| `absorb_hash256_var_into` | Delegates N-message 256-bit batch hashing to `dispatch_hash256_var_into`; writes directly into caller-provided `&mut [[u8;32]]`. |
| `absorb_hash128_var_into` | Delegates N-message 128-bit batch hashing to `dispatch_hash128_var_into`; writes directly into caller-provided `&mut [[u8;16]]`. |
| `dispatch_hash256_var_raw_into` / `dispatch_hash128_var_raw_into` | `ffi` only. Variable-length batch hashing of the pointer/length table of the C API (`RawMessages`), with the same plan as `dispatch_hash*_var_into`. Messages are turned into slices block by block on the hashing thread, so no table of slices is built. |
| `dispatch_hash256_blocks_parallel` / `dispatch_hash128_blocks_parallel` | `python` only. Hash block-wise messages on the pool when the plan for that count says so; they return `false` without writing `out` otherwise. Used by the Python fused path (see §12.2, Python-Specific Notes). |
| `MessageBlocks` | Trait for messages read block by block (`range`, `for_each_block`, blocks of up to 256 messages). Implemented by `RawMessages` (C API) and `ListMessages` (Python). |

### Branch Derivation Pipeline

This is the shared mechanism behind stream initialization (PRNG) and forking (PRNG/XOF, single and batch):

| Function | Description |
|---|---|
| `prepare_base` | Stage 1: absorbs `(s0, s1)` as the first block of the message `state \|\| id` under the domain tag. Accepts either seed-derived state or live stream state. Computed once per parent where siblings share it: PRNG batch `new`, `new_range` and `derive_path`, `fork_many`, `fork_batch`, `fork_batch_range`. |
| `derive_branch_stream` | Stage 2: absorbs the identifier as the final block of the message. Returns `(s0, s1)` ready for `StreamState::new()`. |
| `fork_lane` | Per-lane fork helper: `prepare_base` + `derive_branch_stream` on raw `(s0, s1)` pairs. Used by `StreamState::fork` and by batch `fork` and `fork_range`, where each lane has its own parent. |
| `derive_path_lane` | Applies `fork_lane` once per identifier, in path order. Backs single-stream and batch `derive_path` (PRNG and XOF). |

### Dispatch Layer

| Function | Description |
|---|---|
| `dispatch_hash256_fixed_into` | 256-bit batch evaluation of equal-length messages using either the scalar or Rayon path, selected by workload. Writes directly into caller-provided `&mut [[u8;32]]`. |
| `dispatch_hash128_fixed_into` | Same split as the 256-bit variant, 128-bit digests. |
| `dispatch_hash256_var_into` | 256-bit batch evaluation using either the scalar or Rayon path, selected by workload. Writes directly into caller-provided `&mut [[u8;32]]`. |
| `dispatch_hash128_var_into` | Same scalar/parallel split as the 256-bit variant, without the `s1` serialization. |
| `dispatch_streams` | Advances N independent `StreamLane`s by `steps` rounds each. Output is step-major: `out[step * n + lane]`. Scalar or Rayon path depends on workload size; parallel workers own disjoint lane ranges and run them through all steps. |

Threshold values between scalar and parallel execution are listed in [§9 Execution Policy](#9-execution-policy).

---

## 6. Stream Model (`StreamState`)

Shared component used by both `LustroPrng` and `LustroXof`.

| Method | Description |
|---|---|
| `refill` | Advances the stream by one `stream_step()`; resets cursor to 0. |
| `fill_bytes` | Buffered byte output with a continuous cursor; sequential split calls are equivalent to a single call of the concatenated length (see §10). |
| `read_bytes<N>` | Typed read of exactly N bytes (≤ 32); delegates to `fill_bytes`. |
| `read_full_block` | Returns one full 32-byte block; always triggers one engine step and resets the cursor to 32. |
| `advance_bytes` | Same state as `fill_bytes` of `n` bytes, without writing them. |
| `advance_blocks` | Same state as `n` calls of `read_full_block`; the rest of the current block is discarded. |
| `position` | Bytes taken from the start (`u128`); the discarded rest of a block counts as taken. |
| `from_block_aligned` | Stream at the next block of a batch lane (cursor 32). Used by `lane(i)`. |
| `to_block_aligned` | Lane state `(s0, s1, step)` if the cursor is 32, otherwise `None`. Used by `from_streams`. |
| `fork` | Derives a new `StreamState` via `prepare_base` + `derive_branch_stream`. Does not mutate `self`. The cursor is not used in the derivation; it belongs to the consumption state and is saved in snapshots (see §8). |
| `to_parts` | Returns the complete internal state as `(s0: u128, s1: u128, step: u64, cursor: u8)`. Used by `export_snapshot()` on `LustroPrng` / `LustroXof`. |
| `from_parts` | Reconstructs `StreamState` from raw parts. Panics unless `cursor` is in `1..=32`. Used by `import_snapshot()` on `LustroPrng` / `LustroXof`. |

A new `StreamState` starts with `cursor = 32`; the first read triggers `refill()`.

---

## 7. Batch Model (`StreamLane`)

`StreamLane` is the block-oriented batch equivalent of `StreamState`: the per-lane
state is just `(s0, s1, step)`, with no cursor. Each lane advances in full 32-byte
blocks, so batch has no `next_u64()`-style API and its snapshot format has no cursor.

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
| `LustroPrngBatch` | Multi-stream execution context over a shared seed. `new()`: explicit stream IDs; `new_range()`: sequential stream IDs; `fill_blocks(out, steps)`: advances every lane by `steps` rounds, writing directly into caller-provided output via the dispatcher; `blocks(steps)` / `try_blocks(steps)`: same blocks as `fill_blocks`, in a newly allocated `Vec<[u8; 32]>`; `lane(i)`: lane `i` as a single stream (see §10); `from_streams(streams)` / `try_from_streams(streams)`: one lane per single stream, the reverse of `lane(i)` (see §10); `len()` / `is_empty()`: batch geometry; `suggested_steps()`: `steps` hint for `fill_blocks` (see §9). |
| `LustroXofBatch` | Multi-stream execution context derived from independently absorbed messages. Same `fill_blocks()` / `blocks()` / `try_blocks()` / `lane()` / `from_streams()` / `try_from_streams()` / `len()` / `is_empty()` / `suggested_steps()` surface as `LustroPrngBatch`; `new(messages)` absorbs each message independently before dispatch. |

`fill_blocks()` writes directly into caller-provided output; `blocks(steps)` (Rust and Python) allocates and returns the output instead. The Python buffer kept between calls is described in §12.2.
`lane(i)` copies the state of one lane into a new single stream (Rust, Python) or context (C). 
The new stream starts at the lane's next block, with an empty output buffer (cursor 32 in snapshot terms).
`from_streams(streams)` is the reverse: it copies `(s0, s1, step)` of each stream into a lane.
Only a stream at a block boundary is accepted (see §10).

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
  (rejecting e.g. a PRNG snapshot imported as XOF), then, for single-stream
  snapshots, the `cursor` bounds (`1..=32`), before any state field is decoded.
  Batch snapshots check the minimum header length first, then `version`,
  `kind`, and finally `lane_count` against the total length.
- **Single-stream** (`LustroPrngSnapshot`, `LustroXofSnapshot`): fixed
  56-byte layout: `version(1) | kind(1) | reserved(6) | s0(16) | s1(16) |
  step(8) | cursor(1) | reserved(7)`.
- **Batch** (`LustroPrngBatchSnapshot`, `LustroXofBatchSnapshot`):
  variable length: a 16-byte header (`version(1) | kind(1) | reserved(6) |
  lane_count(8)`) followed by `lane_count × 48` bytes, each lane encoding
  `s0(16) | s1(16) | step(8) | reserved(8)`. There is no cursor field, since batch
  lanes are always block-aligned.

The `version` byte is 1; it does not follow the crate or API version.
`SnapshotKind` values: `Prng` = 1, `Xof` = 2, `PrngBatch` = 3, `XofBatch` = 4.
`SnapshotError` values: `UnsupportedVersion`, `InvalidKind`, `InvalidCursor`,
`InvalidLength`. Snapshots have no checksum: a changed `s0`, `s1` or `step` is
not detected, and the reserved bytes are not checked.
Format lengths are exposed as constants: `LUSTRO_SNAPSHOT_LEN` (56),
`LUSTRO_BATCH_SNAPSHOT_HEADER_LEN` (16) and `LUSTRO_BATCH_SNAPSHOT_LANE_LEN` (48).
`Debug` output on all four snapshot types is redacted, as for `Seed256`,
because snapshots contain internal stream state.
This affects only formatting; `to_le_bytes()` / `from_le_bytes()` are unaffected.

---

## 9. Execution Policy

Thresholds used by the dispatch layer to select the scalar or Rayon path, tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).

```text
Hash parallelization (batch)

Scalar backend (fixed and variable length: separate parameters, same values):
  Rayon path if (count >= 2048 messages OR total input >= 80 KiB)
  AND count >= 2
  messages per Rayon work item: at least ceil(count / (threads × 4))

AVX2 backend, fixed length (`x86_64`, AVX2 available):
  minimum messages for AVX2:    4 (fixed length 0 stays scalar)
  Rayon path if (count >= 4096 messages OR total input >= 192 KiB)
  AND count >= 2
  messages per Rayon work item: whole groups of 4,
                                max(4, ceil4(ceil(count / (threads × 2))))

AVX2 backend, variable length:
  minimum messages for AVX2:    4
  Rayon path if (count >= 3072 messages OR total input >= 192 KiB)
  AND count >= 16
  messages per Rayon work item: as for fixed length

AVX2 kernels process 4 messages per group; the last 0..=3 messages run scalar.
For variable lengths, messages are taken in windows of up to 256. If the first groups of a
window do not already have equal block counts, the window is regrouped by block count so
that the lanes of a group finish together. A group of unequal lengths whose lanes would be
less than 40% busy (sum of blocks < 1.6 × longest) runs scalar.

Stream parallelization (`fill_blocks`)

Scalar backend:
  parallel work-size threshold: 4096 (lanes × steps)
  lanes per Rayon work item:    ceil4(lanes / 8), at most floor4(512 / steps),
                                at least 4 and at least ceil4(lanes / threads)

AVX2 backend (`x86_64`, AVX2 available):
  minimum lanes for AVX2:       4
  parallel work-size threshold: 6144 (lanes × steps)
  lanes per Rayon work item:    as scalar, with 768 in place of 512
  AVX2 kernel width:            4 lanes

Parallel execution needs a pool of at least 2 threads and at least
min(4, threads) Rayon work items. ceil4 / floor4 round up / down to a
multiple of 4.
```

### Thread pool

The Rayon pool is initialized lazily on the first parallel dispatch and reused afterwards. If pool creation fails, dispatch falls back to single-threaded execution. A forked child process whose parent had already claimed the pool (PID differs from the pool owner) also runs single-threaded; a child forked before the first parallel dispatch may start its own pool. Without the `parallel` Cargo feature there is no pool and every batch runs on the calling thread.

### Runtime Execution Controls

The dispatcher takes its settings once, on first use, from `configure()` where it was
called and from the environment variables below otherwise:

- `LUSTRO_DISABLE_MT`: disables Rayon parallel execution (values below).
- `LUSTRO_DISABLE_SIMD`: disables AVX2 execution (stream and hash batches) and uses the scalar backend (values below).
- `LUSTRO_NUM_THREADS=N`: sets the Rayon pool size, capped at the number of logical CPUs. Invalid or zero values are ignored. `1` (or a single logical CPU) disables the pool, like `LUSTRO_DISABLE_MT`.

For `LUSTRO_DISABLE_MT` and `LUSTRO_DISABLE_SIMD` the value is read ignoring case and surrounding whitespace:

| Value | Effect |
|---|---|
| `1`, `true`, `on`, `yes` | disables |
| `0`, `false`, `off`, `no`, an empty value, or the variable not defined | does not disable |
| any other value, including one that is not valid UTF-8 | disables |

An unrecognised value disables, because these settings change only how fast the result is computed, not the result.

`configure()` (Python `lustro.configure()`, C `lustro_configure()`) sets threads and SIMD from code. A value given there replaces the variables of the same purpose: `threads` replaces `LUSTRO_NUM_THREADS` and `LUSTRO_DISABLE_MT`, `allow_simd` replaces `LUSTRO_DISABLE_SIMD`. A value left unset follows the environment. `threads` is capped at the number of logical CPUs, and `1` disables the pool.

The settings are fixed once, by whichever comes first: `configure()`, the first batch call, or `runtime_info()`. `suggested_steps()`, `runtime_preview()` and a single `hash256()` / `hash128()` call do not fix them. When calls race, one of them wins.

A later `configure()` returns `Ok` if it asks for the same effect as the fixed settings, and `AlreadyConfigured` otherwise. Same effect: the same SIMD choice and, with `parallel`, the same pool size (`threads` above the CPU count equals the CPU count; `1` equals `LUSTRO_DISABLE_MT`).

`runtime_info()` (Python `lustro.runtime_info()`, C `lustro_runtime_info()`, `lustro_runtime_threads()` and `lustro_runtime_backend()`) reports the backend and the thread count under these settings. `threads` is the configured pool size (0 = batches run on the calling thread), not the number of running workers; the call does not start the pool. `runtime_preview()` (Rust, Python, C `lustro_runtime_preview()`) returns the same report without fixing the settings.

### Suggested steps

`suggested_steps()` on `LustroPrngBatch` / `LustroXofBatch` returns a `steps`
hint for `fill_blocks` from a table of the stream backend, keyed by the lane count,
so `LUSTRO_DISABLE_SIMD` can change it. It is a hint only: the output for a given
`steps` does not depend on it, and the same policy applies to PRNG and XOF.
It does not fix the runtime settings. Until they are fixed it follows the current environment, so a later `configure()` can still change it.

---

## 10. Behavioral Guarantees

These guarantees hold across Rust, Python, and C FFI layers.

### Stream semantics

- Typed reads (`next_u32`, `next_u64`, `next_u128`) are little-endian, drawn from the same
  underlying byte stream as `fill_bytes`/`fill`/`*_fill`.
- `next_block()` always advances the stream by exactly one engine round and
  returns one 32-byte block, regardless of the cursor position before
  the call; the unread rest of the current block is discarded.
- `advance_bytes(n)` leaves the stream in the same state as `fill_bytes` of `n` bytes, 
  so it can be mixed with `fill_bytes` without breaking the concatenation guarantee below. 
  `advance_blocks(n)` does the same as `n` calls of `next_block()`; `n = 0` changes nothing. 
  Both cost O(n): there is no jump-ahead, because each block is the input of the next.
- `position()` is the number of bytes taken from the start. The rest of a block discarded by 
  `next_block()` or `advance_blocks()` counts as taken. A fresh stream and a fork are at 0; 
  an imported snapshot continues from its own position. In C these are
  `lustro_*_advance_bytes(ctx, n)`, `lustro_*_advance_blocks(ctx, n)` (`n` is a
  `uint64_t`) and `lustro_*_position(ctx, out)`, which writes 16 bytes,
  little-endian.
- `fill_bytes()` / `fill()` / `fill_into()` / `fill_u64()` / `*_fill()` have a
  concatenation guarantee: splitting one large request into several sequential
  calls yields the same bytes as a single call, as long as no other
  stream-consuming call (`next_u32`/`next_u64`/`next_u128`/`next_block`) is
  interleaved.
- Rust `std::io::Read::read` and Python `read()` / `readinto()` read the same
  byte stream as `fill_bytes` / `fill`, so the concatenation guarantee covers
  them too. They fill the whole request; an empty buffer leaves the stream
  where it was.
- `fill_u64(n)` (Python) returns the same values as `n` calls of `next_u64()`,
  from any cursor position, and leaves the stream where those calls would.
- Single-stream `export_snapshot()` / `import_snapshot()` (PRNG/XOF) resume
  exactly: the restored context continues from the exported `step` and
  `cursor`. Batch snapshots restore each lane's `(s0, s1, step)` state;
  batch lanes have no cursor (see §7 and §8).

### Fork semantics

- `fork()` (single-stream and batch, PRNG and XOF) always starts the child at
  `step = 0`; the child does not inherit the parent's step counter or cursor.
- The parent stream is unaffected by any number of `fork()` calls.
- Fork is deterministic: the same parent state and identifier produce the same
  child. The identifier is part of the branch derivation; different identifiers
  are intended to produce different derived streams.
- A fork depends on the parent's current block. Two forks with the same
  identifier are identical while the parent stays in that block, and reading
  further bytes from it changes nothing. The read that moves the parent to the
  next block (also the first read of a new stream) changes the state the child
  is derived from.
- The 32 bytes of an output block are the stream state (`s0`, `s1`) at that
  point. A full block and the number of blocks generated so far are enough to
  compute every following block; nothing is claimed about going backwards.
  `fork(id)` follows from this: once a PRNG has generated a block, it gives the
  same child as `LustroPrng::new` seeded with the latest generated block and
  the same identifier. Lustro makes no cryptographic claim, so this is not a
  security boundary.
- `fork_many(ids)` (batch only, PRNG and XOF) derives `ids.len()` children per
  lane, producing a batch of `len() * ids.len()` lanes in parent-major order:
  child `j` of lane `i` is at index `i * ids.len() + j`. Same `step = 0` and
  parent-unaffected guarantees as `fork()`. Empty `ids` produces an empty batch.
- `fork_batch(ids)` and `fork_batch_range(first, count)` (single stream, PRNG and
  XOF) return a batch whose lane `i` is `fork(ids[i])`, or `fork(first + i)` with
  wrapping modulo 2^128. `fork_path(path)` returns `fork(path[0]).fork(path[1])...`
  without the intermediate streams. All three derive from the current state like
  `fork()` and leave the parent unchanged. Empty `ids` or `count = 0` gives an empty
  batch; an empty `path` is an error, as in `derive_path`.

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
- On a stream that has produced no output, `fork_path` matches `derive_path`:
  `LustroPrng::new(seed, id).fork_path(path)` equals `derive_path(seed, [id] + path)`,
  and `LustroXof::new(message).fork_path(path)` equals
  `LustroXof::derive_path(message, path)`.

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
- `blocks(steps)` (Rust and Python) returns the blocks `fill_blocks` would write, in a new
  `Vec<[u8; 32]>` (Rust) or array (Python), and advances the lanes the same way. On
  a size or allocation error the state is unchanged; in Python this does not cover a
  failure while the array object is created, after generation. An empty batch
  gives an empty result.
- `lane(i)` (batch only, PRNG and XOF) returns lane `i` as an independent single
  stream positioned at that lane's next block. Its bytes are the concatenation
  of the blocks the batch would write for lane `i` from now on. The batch is not
  changed, and later use of the batch or the lane does not affect the other.
  `lane(i).fork(id)` equals element `i` of `fork(ids)` with `ids[i] = id`.
  An index `i >= len()` is an error: `None` (Rust), `IndexError` (Python),
  `InvalidArgument` (C).
- `from_streams(streams)` (PRNG and XOF) builds a batch with one lane per
  stream, in order. Lane `i` continues `streams[i]` at its next block, so the
  blocks the batch writes for lane `i` are the blocks `streams[i].next_block()`
  would return, and `lane(i)` returns a copy of `streams[i]`. The streams are
  not changed. They may come from different seeds or messages, and the same
  stream may appear more than once; an empty list gives an empty batch.
  Every stream must be at a block boundary (cursor 32: freshly created, or read
  in whole blocks). A stream with part of its current block unread is an error,
  so no output byte is dropped: `BatchError::PartialBlock { index }` from
  `try_from_streams` (Rust; `from_streams` panics), `ValueError` (Python),
  `InvalidArgument` (C). `index` is the first such stream. A stream whose
  `position()` is not a multiple of 32 is inside a block; `32 - position() % 32`
  more bytes bring it to the next boundary.
- Execution fallback is transparent (see §9, Thread pool).

### API robustness semantics

- **Misuse is fail-fast.** Length or shape mismatches supplied by the caller are
  programming errors and may panic in the Rust API (see §12.1 Error Model).
  Malformed *runtime data*, such as corrupt snapshot bytes, is different: APIs
  that validate external input report it through `Result` or `LustroError`.
  All release builds use `panic = "unwind"` (see §13).
- For an empty `derive_path` path, a wrong number of identifiers in `fork`, a `len() * ids.len()` overflow in `fork_many`,
  a `len() * steps` overflow in `blocks`, a stream inside a block in `from_streams`, and lane or output allocation failure,
  the Rust API also provides `try_*` variants that return `Err` instead of
  panicking (see §12.1).
- **Snapshot kind is validated before decoding.** A PRNG snapshot cannot be
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
| Fill bytes | `fill_bytes(out)` | `fill(size)` → `bytes`, `fill_into(buf)`, `fill_u64(n)` → `uint64` array | `lustro_*_fill(ctx, out, out_len)` |
| Read as a stream | `std::io::Read::read(buf)` | `read(size)` → `bytes`, `readinto(buf)` → bytes written | `lustro_*_fill` (no separate function) |
| Typed reads | `next_u32()`, `next_u64()`, `next_u128()`, `next_block()` | same | `lustro_*_next_u32`, `_next_u64`, `_next_u128`, `_next_block` (write to `out`) |
| Skip and position | `advance_bytes(n)`, `advance_blocks(n)`, `position()` | same | `lustro_*_advance_bytes(ctx, n)`, `_advance_blocks(ctx, n)`, `_position(ctx, out)` (16 bytes, little-endian) |
| Copy | `clone()`, `try_clone()` (batch) | `copy()`, `copy.copy()` | `lustro_prng_clone`, `lustro_xof_clone`, `lustro_prng_batch_clone`, `lustro_xof_batch_clone` |
| Release | `Drop` | garbage collection | `lustro_*_free` |
| Batch constructor | `new(seed, stream_ids)`, `new_range(...)` | `LustroPrngBatch(seed, stream_ids)`, `new_range(...)` | `lustro_prng_batch_new`, `_new_range` |
| Batch length | `len()`, `is_empty()` | `len()`, `is_empty()`, `len(batch)` | `lustro_*_batch_len`, `lustro_*_batch_get_len(ctx, &len)` |
| Batch output | `fill_blocks(out, steps)`, `blocks(steps)` → `Vec<[u8; 32]>` | `fill_blocks(out, steps)`, `blocks(steps)` → `uint64` array | `lustro_*_batch_fill_blocks(ctx, out, out_len, steps)` |
| Batch lane as a single stream | `lane(i)` → `Option<…>` | `lane(i)` (`IndexError` if out of range) | `lustro_*_batch_lane(ctx, index, out_ctx)` (`InvalidArgument` if out of range) |
| Single stream forked into a batch | `fork_batch(&ids)`, `fork_batch_range(first, count)`, `try_` variants | `fork_batch(ids)`, `fork_batch_range(first, count)` | `lustro_*_fork_batch(ctx, ids_hi, ids_lo, n, out_ctx)`, `lustro_*_fork_batch_range(ctx, first_hi, first_lo, count, out_ctx)` |
| Single streams as a batch | `from_streams(&[…])`, `try_from_streams(&[…])` | `from_streams(streams)` (staticmethod; `ValueError` for a stream inside a block) | `lustro_*_batch_from_streams(streams, n, out_ctx)` (`InvalidArgument` for a stream inside a block) |
| Snapshot | `export_snapshot()` → snapshot type, `to_le_bytes()` | `export_snapshot()` → `bytes` | `lustro_*_export_snapshot`; batch: `_batch_snapshot_size` first |
| Restore | `import_snapshot(snapshot)` | `import_snapshot(bytes)` | `lustro_*_import_snapshot` |
| Hash, batch | `hash256_many_var(&[&[u8]])` (variable length), `hash256_many_fixed(data, message_len)` and `hash256_many_fixed_into(data, message_len, out)` (fixed length) | `hash256_many_fixed` (2D array, fixed length), `hash256_many_fixed_into(data, out)`, `hash256_many_var`, `hash256_many_var_into(messages, out)` | `lustro_hash256_many_fixed` (fixed length), `_many_var` |

`fork`, `fork_many`, `fork_range`, `fork_batch`, `fork_batch_range`, `fork_path`,
`derive_path` and `suggested_steps` have the same name in all three layers (C functions
carry the `lustro_<type>_` prefix).

---

## 12. Language Bindings

### 12.1 Rust API (pure Rust, no bindings)

The main types (`LustroPrng`, `LustroXof`, `LustroPrngBatch`, `LustroXofBatch`,
`Seed256`, `StreamId`, `Hash128`, `Hash256`) and the hash functions are
re-exported at the crate root, e.g. `lustro::LustroPrng`. The module paths
(`lustro::prng::LustroPrng`, `lustro::types::Seed256`) remain valid and refer
to the same items. `EntropyError` is re-exported at the crate root with
`feature = "entropy"`. `BatchError`, `ConfigError`, `DerivePathError`, `ParseHashError`,
`RuntimeInfo`, `RuntimeOptions`, `SnapshotError`, `SnapshotKind` and the
snapshot types are re-exported at the crate root as well.
`LustroError` is only available under `lustro::errors`.

**Functions**

| Module | Functions |
|---|---|
| API Version, runtime | `lustro_api_version()`, `runtime_info()`, `runtime_preview()`, `configure(options)` |
| Hash | `hash256(message)`, `hash128(message)`, `hash256_many_fixed(data, message_len)`, `hash128_many_fixed(data, message_len)`, `hash256_many_fixed_into(data, message_len, out)`, `hash128_many_fixed_into(data, message_len, out)`, `hash256_many_var(messages)`, `hash128_many_var(messages)`, `hash256_many_var_into(messages, out)`, `hash128_many_var_into(messages, out)` |
| `LustroPrng` | `new(seed, stream_id)`, `from_entropy()` and `from_entropy_with_id(id)` (`feature = "entropy"`), `next_u32()`,`next_u64()`, `next_u128()`, `next_block()`, `fill_bytes(out)`, `advance_bytes(n)`, `advance_blocks(n)`, `position()`, `fork(id)`, `fork_batch(ids)`, `try_fork_batch(ids)`, `fork_batch_range(first, count)`, `try_fork_batch_range(first, count)`, `fork_path(path)`, `try_fork_path(path)`, `derive_path(seed, path)`, `try_derive_path(seed, path)`, `clone()`, `export_snapshot()`, `import_snapshot(snapshot)` |
| `LustroPrngBatch` | `new(seed, stream_ids)`, `try_new(seed, stream_ids)`, `new_range(seed, first_stream_id, count)`, `try_new_range(seed, first_stream_id, count)`, `from_streams(streams)`, `try_from_streams(streams)`, `len()`, `is_empty()`, `suggested_steps()`, `fill_blocks(out, steps)`, `blocks(steps)`, `try_blocks(steps)`, `lane(i)`, `fork(ids)`, `try_fork(ids)`, `fork_many(ids)`, `try_fork_many(ids)`, `fork_range(first)`, `try_fork_range(first)`, `derive_path(seed, roots, path)`, `try_derive_path(seed, roots, path)`, `clone()`, `try_clone()`, `export_snapshot()`, `import_snapshot(snapshot)` |
| `LustroXof` | `new(message)`, `next_u32()`,`next_u64()`, `next_u128()`, `next_block()`, `fill_bytes(out)`, `advance_bytes(n)`, `advance_blocks(n)`, `position()`, `fork(id)`, `fork_batch(ids)`, `try_fork_batch(ids)`, `fork_batch_range(first, count)`, `try_fork_batch_range(first, count)`, `fork_path(path)`, `try_fork_path(path)`, `derive_path(message, path)`, `try_derive_path(message, path)`, `clone()`, `export_snapshot()`, `import_snapshot(snapshot)` |
| `LustroXofBatch` | `new(messages)`, `try_new(messages)`, `from_streams(streams)`, `try_from_streams(streams)`, `len()`, `is_empty()`, `suggested_steps()`, `fill_blocks(out, steps)`, `blocks(steps)`, `try_blocks(steps)`, `lane(i)`, `fork(ids)`, `try_fork(ids)`, `fork_many(ids)`, `try_fork_many(ids)`, `fork_range(first)`, `try_fork_range(first)`, `derive_path(messages, path)`, `try_derive_path(messages, path)`, `clone()`, `try_clone()`, `export_snapshot()`, `import_snapshot(snapshot)` |

**Types**

| Type | Methods / Traits |
|---|---|
| `Hash128`, `Hash256`, `Seed256` | `as_bytes()`, `AsRef<[u8; N]>`, `AsRef<[u8]>`, `TryFrom<&[u8]>`, `From<[u8; N]>` |
| `Hash128`, `Hash256` (additionally) | `From<Hash> for [u8; N]`, `Display` / `LowerHex` / `UpperHex` (hex of `as_bytes()`, `{:#x}` adds `0x`), `FromStr` (exactly 2·N hex digits, either case, optional `0x` / `0X`; `Err(ParseHashError)`), `Hash`, `Ord` (lexicographic over bytes) |
| `Hash256` (additionally) | `first_128() -> Hash128` (first 16 bytes; equals `hash128` of the same input) |
| `ParseHashError` | `InvalidLength { expected, actual }`, `InvalidDigit { index }`, `Display`, `std::error::Error`; `#[non_exhaustive]` |
| `Seed256` (additionally) | `from_bytes([u8; 32])`; with `feature = "entropy"`: `from_entropy() -> Result<Seed256, EntropyError>` |
| `EntropyError` (`feature = "entropy"`) | `Display`, `std::error::Error` (`source()` returns the OS error) |
| `StreamId` | `get()`, `From<u8>`, `From<u16>`, `From<u32>`, `From<u64>`, `From<u128>`, `From<StreamId> for u128`, `Ord` (numeric) |
| `LustroPrngSnapshot`, `LustroXofSnapshot` | `to_le_bytes()` (56 bytes, fixed), `from_le_bytes()`, `TryFrom<&[u8]>` |
| `LustroPrngBatchSnapshot`, `LustroXofBatchSnapshot` | `to_le_bytes()` (variable length), `from_le_bytes()`, `TryFrom<&[u8]>` |
| `SnapshotKind` | `Prng`, `Xof`, `PrngBatch`, `XofBatch`; validated before snapshot state fields are decoded |
| `SnapshotError` | `UnsupportedVersion`, `InvalidKind`, `InvalidCursor`, `InvalidLength`; `#[non_exhaustive]`; implements `Display` and `std::error::Error` |
| `RuntimeInfo` | `backend: &'static str` (`"scalar"` or `"avx2"`), `threads: usize`, `fixed: bool`, `parallel_built: bool`, `simd_available: bool`, `pool_usable: bool`, `logical_cpus: usize`; returned by `runtime_info()` and `runtime_preview()`. Fields: see [Runtime Execution Controls](#runtime-execution-controls). `#[non_exhaustive]` |
| `RuntimeOptions` | `new()`, `threads(NonZeroUsize)`, `allow_simd(bool)` (builder, `const fn`); `Default`, `Copy`. A setting that is not called follows the environment. Passed to `configure()` |
| `ConfigError` | `AlreadyConfigured`; `#[non_exhaustive]`; `Display`, `std::error::Error` |
| `LustroError` | `#[non_exhaustive]`; `From<SnapshotError>`, `From<ConfigError>`, `From<BatchError>`, `From<DerivePathError>` |
| `DerivePathError` | `EmptyPath`, `Reserve(TryReserveError)` (batch only); `Display`, `std::error::Error`; `#[non_exhaustive]` |
| `BatchError` | `SizeOverflow` (a computed size overflows `usize`), `Reserve(TryReserveError)` (a buffer reservation failed: allocator refusal or capacity overflow), `LengthMismatch { expected, actual }` (`try_fork` got a wrong number of identifiers), `PartialBlock { index }` (`try_from_streams`: stream `index` is inside a block); `Display`, `std::error::Error`; `#[non_exhaustive]` |
| Constants (`lustro::types`) | `LUSTRO_SEED_LEN` (32), `LUSTRO_BLOCK_LEN` (32), `LUSTRO_HASH128_LEN` (16), `LUSTRO_HASH256_LEN` (32), `LUSTRO_SNAPSHOT_LEN` (56), `LUSTRO_BATCH_SNAPSHOT_HEADER_LEN` (16), `LUSTRO_BATCH_SNAPSHOT_LANE_LEN` (48) |
| `LustroPrng`, `LustroXof`, `LustroPrngBatch`, `LustroXofBatch` | `Debug` is redacted: single streams show the step, batches show the lane count; `s0`/`s1` are never printed |
| `LustroPrng`, `LustroXof` | `std::io::Read`: `read` behaves like `fill_bytes` and returns `Ok(buf.len())`; the stream has no end, so `read_to_end` and `read_to_string` return `InvalidInput` (bound the read, for example with `take`) |
| `LustroPrng`, `LustroXof` (`feature = "rand"`) | `rand_core::TryRng` (`Error = Infallible`, the `try_*` methods always return `Ok`) and `rand_core::Rng`; the methods read the same stream as `next_u32`, `next_u64` and `fill_bytes`. `LustroPrng` also implements `rand_core::SeedableRng` (`from_seed` creates stream 0) |

**Optional Integrations**

Under `feature = "rand"`, `LustroPrng` and `LustroXof` implement
`rand_core::TryRng` with `Error = Infallible`, and through it `rand_core::Rng`
(`next_u32`, `next_u64`, `fill_bytes`). `LustroPrng` also implements
`rand_core::SeedableRng`. The integration targets `rand_core` 0.10.
`feature = "rand"`, `feature = "python"` and `feature = "ffi"` enable `feature = "entropy"`.

- `SeedableRng::from_seed()`, `From<Seed256>`, and `From<[u8; 32]>` all create
  **stream 0**. Use `LustroPrng::new()` directly when a specific `StreamId` is
  required. `From<Seed256>` and `From<[u8; 32]>` do not require `feature = "rand"`.
- `feature = "entropy"` adds `Seed256::from_entropy()`, `LustroPrng::from_entropy()`
  (stream 0 of a fresh seed) and `LustroPrng::from_entropy_with_id(id)`. All return `Err(EntropyError)` when the operating system
  cannot provide entropy. `rand_core` 0.10 has no `SeedableRng::from_entropy()`;
  use `LustroPrng::from_entropy()?`.
- `LustroXof` does not implement `SeedableRng`: `LustroXof::new` takes a
  message of any length, not a fixed-size seed.
- `SeedableRng` also has a `fork()` method without arguments. On `LustroPrng`
  the inherent `fork(id)` takes precedence; call `SeedableRng::fork(&mut rng)`
  to use the trait method, which seeds a new generator from 32 bytes of `rng`.

**Error Model**

Fallible operations return `Result`: the four snapshot decoders, the
`TryFrom<&[u8]>` conversions for `Hash128`, `Hash256`, and `Seed256`,
`Seed256::from_entropy()`, `LustroPrng::from_entropy()` and `from_entropy_with_id()` (`feature = "entropy"`), `configure()` and the
`try_*` batch variants. Everything else succeeds for valid arguments;
documented misuse checks and, in the non-`try_` variants, allocation or size
failures may panic.

| Function(s) | Behavior |
|---|---|
| `LustroPrngSnapshot::from_le_bytes`, `LustroXofSnapshot::from_le_bytes`, `LustroPrngBatchSnapshot::from_le_bytes`, `LustroXofBatchSnapshot::from_le_bytes` | Returns `Result<Self, SnapshotError>`. |
| `Hash128::try_from`, `Hash256::try_from`, `Seed256::try_from` (via `TryFrom<&[u8]>`) | Returns `Result<Self, TryFromSliceError>` on wrong-length input. |
| `Hash128::from_str`, `Hash256::from_str` (`str::parse`) | Returns `Result<Self, ParseHashError>`. The length is checked first: `InvalidLength { expected, actual }`, where `actual` counts the bytes after an optional `0x` / `0X`. Then the digits: `InvalidDigit { index }`, where `index` is the byte offset in the whole input, prefix included, always at a char boundary. Spaces are not trimmed. |
| `Seed256::from_entropy`, `LustroPrng::from_entropy`, `LustroPrng::from_entropy_with_id` (`feature = "entropy"`) | Returns `Result<Self, EntropyError>` when the operating system cannot provide entropy. |
| `LustroPrng` / `LustroXof` `read_to_end`, `read_to_string` | Always returns `Err` with `ErrorKind::InvalidInput` and reads nothing: the stream has no end. A limited read (`take`) works. |
| `configure` | Returns `Result<(), ConfigError>`. `Ok(())` if the settings are not fixed yet, or are fixed with the same effect as the requested ones. `Err(ConfigError::AlreadyConfigured)` if they are fixed with another effect. |
| `hash256_many_fixed` / `hash128_many_fixed` | Panics if `message_len` is 0 or `data.len()` is not a multiple of `message_len`. |
| `hash256_many_fixed_into` / `hash128_many_fixed_into` | Panics if `data.len() != out.len() * message_len`. |
| `hash256_many_var_into` / `hash128_many_var_into` | Panics if `messages.len() != out.len()`. |
| `LustroPrngBatch::fill_blocks` / `LustroXofBatch::fill_blocks` | Panics if `out.len() != len() * steps` (or on `usize` overflow). |
| `LustroPrngBatch::blocks` / `LustroXofBatch::blocks` | Panics if `len() * steps` overflows `usize`, or if the output allocation fails. `try_blocks` returns `Err(BatchError::SizeOverflow)` or `Err(BatchError::Reserve)` instead; the state is unchanged. |
| `LustroPrngBatch::lane` / `LustroXofBatch::lane` | Returns `None` if `i >= len()`. |
| `LustroPrngBatch::from_streams` / `LustroXofBatch::from_streams` | Panics if a stream is inside a block, or if lane allocation fails. `try_from_streams` returns `Err(BatchError::PartialBlock { index })` (the first such stream) or `Err(BatchError::Reserve)` instead; the streams are not changed. |
| `LustroPrngBatch::new` / `LustroXofBatch::new` | Panics if lane allocation fails. `try_new` returns `Err(BatchError::Reserve)` instead. |
| `LustroPrngBatch::fork` / `LustroXofBatch::fork` | Panics if `ids.len() != len()`, or if lane allocation fails. `try_fork` returns `Err(BatchError::LengthMismatch { expected, actual })` or `Err(BatchError::Reserve)` instead. |
| `LustroPrngBatch::fork_range` / `LustroXofBatch::fork_range` | Panics if lane allocation fails. `try_fork_range` returns `Err(BatchError::Reserve)` instead. |
| `LustroPrngBatch::new_range` | Panics if lane allocation fails. `try_new_range` returns `Err(BatchError::Reserve)` instead. |
| `LustroPrngBatch::fork_many` / `LustroXofBatch::fork_many` | Panics if `len() * ids.len()` overflows `usize`, or if lane allocation fails. `try_fork_many` returns `Err(BatchError::SizeOverflow)` or `Err(BatchError::Reserve)` instead. |
| `LustroPrngBatch::try_clone` / `LustroXofBatch::try_clone` | Returns `Err(BatchError::Reserve)` if the lane buffer cannot be reserved (never `SizeOverflow`). `clone()` aborts the process in that case. |
| `LustroPrng::derive_path` / `LustroXof::derive_path` | Panics if `path` is empty. `try_derive_path` returns `Err(DerivePathError::EmptyPath)` instead. |
| `LustroPrng::fork_batch`, `fork_batch_range` / the same on `LustroXof` | Panics if lane allocation fails. `try_fork_batch` and `try_fork_batch_range` return `Err(BatchError::Reserve)` instead. |
| `LustroPrng::fork_path` / `LustroXof::fork_path` | Panics if `path` is empty. `try_fork_path` returns `Err(DerivePathError::EmptyPath)` instead. |
| `LustroPrngBatch::derive_path` / `LustroXofBatch::derive_path` | Panics if `path` is empty or lane allocation fails. `try_derive_path` returns `Err(DerivePathError::EmptyPath)` or `Err(DerivePathError::Reserve)` instead. |
| Everything else (single-stream `new`, `next_*`, `fill_bytes`, `Read::read`, single-stream `fork`, `clone`, `export_snapshot`, `import_snapshot(snapshot)`, `hash256`, `hash128`, `hash256_many_var`, `hash128_many_var`) | Infallible for valid Rust-typed arguments (`Read::read` always returns `Ok`). |

Note: these panics are plain Rust panics (`assert_eq!`, `expect`), not `LustroError`
values; `LustroError` is part of the C FFI ABI only (see §12.3).

---

### 12.2 Python Bindings

**Functions**

| Module | Functions |
|---|---|
| Module-level | `lustro_api_version()`, `runtime_info()`, `runtime_preview()` (both return a `dict` with the `RuntimeInfo` fields, typed as `TypedDict` in the stubs), `configure(threads=None, allow_simd=None)`, `__version__`, size constants `SEED_LEN`, `BLOCK_LEN`, `HASH128_LEN`, `HASH256_LEN`, `SNAPSHOT_LEN`, `BATCH_SNAPSHOT_HEADER_LEN`, `BATCH_SNAPSHOT_LANE_LEN` (same values as the Rust constants), exception `SnapshotError` (subclass of `ValueError`) |
| Hash | `hash256(data)`, `hash128(data)`, `hash256_many_fixed(data)`, `hash128_many_fixed(data)`, `hash256_many_fixed_into(data, out, *, release_gil=True)`, `hash128_many_fixed_into(data, out, *, release_gil=True)`, `hash256_many_var(messages, *, release_gil=True)`, `hash128_many_var(messages, *, release_gil=True)`, `hash256_many_var_into(messages, out, *, release_gil=True)`, `hash128_many_var_into(messages, out, *, release_gil=True)` |
| `LustroPrng` | `LustroPrng(seed, stream_id=0)`, `from_entropy(stream_id=0)`, `next_u32()`, `next_u64()`, `next_u128()`, `next_block()`, `advance_bytes(n)`, `advance_blocks(n)`, `position()`, `fill(size)`, `fill_into(buf)`, `read(size)`, `readinto(buf)`, `fill_u64(n)`, `copy()`, `fork(id)`, `fork_batch(ids)`, `fork_batch_range(first, count)`, `fork_path(path)`, `derive_path(seed, path)`, `export_snapshot()`, `import_snapshot(bytes)` |
| `LustroPrngBatch` | `LustroPrngBatch(seed, stream_ids)`, `new_range(seed, first_stream_id, count)`, `from_streams(streams)`, `len()`, `is_empty()`, `suggested_steps()`, `release_buffer()`, `copy()`, `fill_blocks(out, steps, *, release_gil=True)`, `blocks(steps)`, `lane(i)`, `batch[i]`, `iter(batch)`, `fork(ids)`, `fork_many(ids)`, `fork_range(first)`, `derive_path(seed, roots, path)`, `export_snapshot()`, `import_snapshot(bytes)` |
| `LustroXof` | `LustroXof(message)`, `next_u32()`, `next_u64()`, `next_u128()`, `next_block()`, `advance_bytes(n)`, `advance_blocks(n)`, `position()`, `fill(size)`, `fill_into(buf)`, `read(size)`, `readinto(buf)`, `fill_u64(n)`, `copy()`, `fork(id)`, `fork_batch(ids)`, `fork_batch_range(first, count)`, `fork_path(path)`, `derive_path(message, path)`, `export_snapshot()`, `import_snapshot(bytes)` |
| `LustroXofBatch` | `LustroXofBatch(messages)`, `from_streams(streams)`, `len()`, `is_empty()`, `suggested_steps()`, `release_buffer()`, `copy()`, `fill_blocks(out, steps, *, release_gil=True)`, `blocks(steps)`, `lane(i)`, `batch[i]`, `iter(batch)`, `fork(ids)`, `fork_many(ids)`, `fork_range(first)`, `derive_path(messages, path)`, `export_snapshot()`, `import_snapshot(bytes)` |

All four classes support `copy.copy()` / `copy.deepcopy()` and `pickle` (the
classes report `__module__ == "lustro"`); the batch classes support `len(batch)`.

**Parameter Types**

Unless otherwise noted, parameters for functions not listed here are
plain Python `int`/`bool` with no shape or dtype constraints. Return values
vary by function, e.g. `next_u64()`/`next_u128()` return Python `int`,
`next_block()` returns `bytes`, `hash256_many_fixed()` returns a NumPy array;
see the function tables above. NumPy array *parameters* are required only
where noted below: a wrong type or dtype raises `TypeError`, a wrong shape
or layout raises `ValueError`.

NumPy is required for `hash*_many*`, `fill_blocks`, `blocks` and `fill_u64`.
Without it these calls raise `ImportError`; the stream calls (`fill_blocks`,
`blocks`, `fill_u64`) do so before the stream state moves.

Byte inputs. Parameters shown as Python `bytes` below (`data`, `seed`, `message` and snapshot `bytes`) also accept `bytearray`, `memoryview`, `array('B')` and NumPy `uint8` arrays. The buffer must be C-contiguous with byte items, else `TypeError`. `bytes` is read without copying; any other buffer is copied while the GIL is held, so other Python threads cannot change it during the computation (writes from native code without the GIL are not covered). `MemoryError` if the copy cannot be allocated.

| Function                                                  | Parameter | Expected type |
|-----------------------------------------------------------|---|---|
| `configure(threads=None, allow_simd=None)`                | `threads` | Python `int`, `threads >= 1` (`ValueError` if smaller, `TypeError` for a non-int); capped at the logical CPU count; `None` follows the environment. A value outside `i64` raises `OverflowError` |
|                                                           | `allow_simd` | `bool`; `False` selects the scalar backend; `None` follows the environment. A wrong type raises `TypeError`. Settings already fixed with another effect: `RuntimeError` |
| `hash256`/`hash128(data)`                                 | `data` | Python `bytes`, or a byte buffer (`bytearray`, `memoryview`, `array('B')`, NumPy `uint8`), any length; `TypeError` for `str`, other types, non-contiguous buffers or buffers with other item formats |
| `hash256_many_fixed(data)`                                | `data` | 2D `numpy.ndarray`, dtype `uint8`, shape `(n, msg_len)`, rows contiguous (`ValueError` otherwise; `TypeError` for wrong type/dtype) → returns shape `(n, 32)`, dtype `uint8` |
| `hash256_many_var(messages)`                              | `messages` | sequence of `bytes` or byte buffers, each element independently sized (`TypeError` for a single string or buffer, a non-sequence such as a generator, or an item that is not a byte buffer) → returns shape `(n, 32)`, dtype `uint8` |
| `hash128_many_fixed(data)`                                | `data` | 2D `numpy.ndarray`, dtype `uint8`, shape `(n, msg_len)`, rows contiguous (`ValueError` otherwise; `TypeError` for wrong type/dtype) → returns shape `(n, 16)`, dtype `uint8` |
| `hash128_many_var(messages)`                              | `messages` | sequence of `bytes` or byte buffers, each element independently sized (`TypeError` for a single string or buffer, a non-sequence such as a generator, or an item that is not a byte buffer) → returns shape `(n, 16)`, dtype `uint8` |
| `hash256_many_fixed_into(data, out, *, release_gil=True)` | `data`, `out` | `data` as in `hash256_many_fixed`; `out` a writable, C-contiguous `uint8` array of shape `(n, 32)` (`ValueError` for another shape or a non-contiguous `out`, `TypeError` for another type, dtype or ndim). Returns `None` |
| `hash128_many_fixed_into(data, out, *, release_gil=True)` | `data`, `out` | as above, `out` of shape `(n, 16)` |
| `hash256_many_var_into(messages, out, *, release_gil=True)` | `messages`, `out` | `messages` as in `hash256_many_var`; `out` a writable, C-contiguous `uint8` array of shape `(len(messages), 32)`, errors as above. Returns `None` |
| `hash128_many_var_into(messages, out, *, release_gil=True)` | `messages`, `out` | as above, `out` of shape `(len(messages), 16)` |
| `hash*_many_fixed_into`, `hash*_many_var`, `hash*_many_var_into`, `fill_blocks` | `release_gil` | `bool`, keyword-only, default `True`. `False` keeps the GIL held and works in place where the input allows (see Python-Specific Notes) |
| `LustroPrng(seed, stream_id=0)`                           | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) |
|                                                           | `stream_id` | Python `int`, `0 ≤ stream_id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`); default `0` |
| `LustroPrng.fill(size)`                                   | `size` | Python `int` → returns `bytes` of that length (`OverflowError` if negative or above `isize::MAX`, `MemoryError` if the buffer cannot be allocated) |
| `LustroPrng.advance_bytes(n)`, `advance_blocks(n)`        | `n` | Python `int`, `0 ≤ n < 2⁶⁴` (`OverflowError` if negative or above `2⁶⁴ - 1`). Costs O(n), the GIL is released. Returns `None` |
| `LustroPrng.position()`                                   | — | returns Python `int`: bytes taken from the start |
| `LustroPrng.from_entropy(stream_id=0)`                    | `stream_id` | as in `LustroPrng(seed, stream_id)` (staticmethod). `OSError` if the system entropy source is unavailable |
| `LustroPrng.fill_into(buf)`                               | `buf` | writable, C-contiguous buffer of bytes (e.g. `bytearray`, `memoryview`), filled in place to its full length (`TypeError` otherwise) |
| `LustroPrng.read(size)`                                   | `size` | Python `int`, same as `fill(size)` → returns `bytes` of that length (`OverflowError` if negative; there is no form without `size`) |
| `LustroPrng.readinto(buf)`                                | `buf` | writable, C-contiguous buffer with any item format (e.g. `bytearray`, `memoryview`, `array('I')`, NumPy array of any dtype), filled in place to its full length with raw stream bytes, without conversion → returns the number of bytes written (not items). `TypeError` for `bytes`, objects without a buffer, read-only or non-contiguous buffers; the stream is not moved then. An empty buffer returns `0` and does not move the stream |
| `LustroPrng.fill_u64(n)`                                  | `n` | Python `int`, `n >= 0` (`OverflowError` if negative, above `2⁶⁴ - 1`, or if `8 * n` exceeds `isize::MAX`) → returns a new writable `numpy.ndarray`, shape `(n,)`, dtype `uint64`, equal to `n` calls of `next_u64()`. `MemoryError` if the output buffer cannot be allocated |
| `LustroPrng.fork(id)`                                     | `id` | Python `int`, `0 ≤ id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`) |
| `LustroPrng.fork_batch(ids)`                              | `ids` | Python sequence of `int` (`list`, `tuple`, `range`, ...), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise) → returns a `LustroPrngBatch` whose lane `i` is `fork(ids[i])`. `MemoryError` if the lanes cannot be allocated |
| `LustroPrng.fork_batch_range(first, count)`               | `first`, `count` | `first` Python `int`, `0 ≤ first < 2¹²⁸`; `count` Python `int`, `0 ≤ count < 2⁶⁴` (`OverflowError` otherwise) → as `fork_batch` with ids `first`, `first + 1`, ... (modulo 2¹²⁸). `MemoryError` if the lanes cannot be allocated |
| `LustroPrng.fork_path(path)`                              | `path` | Python sequence of `int`, non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise) → returns `fork(path[0]).fork(path[1])...` |
| `LustroPrng.export_snapshot()`                            | — | returns `bytes`, exactly 56 bytes |
| `LustroPrng.import_snapshot(bytes)`                       | `bytes` | Python `bytes`, exactly 56 bytes (staticmethod) |
| `LustroPrng.derive_path(seed, path)`                      | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) (staticmethod) |
|                                                           | `path` | Python sequence of `int` (`list`, `tuple`, `range`, ...), non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`) |
| `LustroPrngBatch(seed, stream_ids)`                       | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) |
|                                                           | `stream_ids` | Python sequence of `int` (`list`, `tuple`, `range`, ...), each `0 ≤ id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`). `MemoryError` if the lanes cannot be allocated |
| `LustroPrngBatch.new_range(seed, first_stream_id, count)` | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) (staticmethod) |
|                                                           | `first_stream_id`, `count` | `first_stream_id` Python `int`, `0 ≤ id < 2¹²⁸`; `count` Python `int`, `0 ≤ count < 2⁶⁴` (`OverflowError` otherwise). `MemoryError` if the lanes cannot be allocated |
| `LustroPrngBatch.fill_blocks(out, steps, *, release_gil=True)` | `out`, `steps` | writable, C-contiguous `numpy.ndarray`, shape `(steps, n, 4)`, dtype `uint64`, `n == len(batch)`; Python `int`. Wrong type → `TypeError`; shape mismatch or non-C-contiguous → `ValueError`. For non-empty output the data must also be aligned for `uint64`: a misaligned buffer (for example `np.frombuffer` at an odd offset) raises `ValueError` |
| `LustroPrngBatch.blocks(steps)`                           | `steps` | Python `int`, `steps >= 0` (`OverflowError` if negative or above `2⁶⁴ - 1`) → returns a new writable, C-contiguous `numpy.ndarray`, shape `(steps, n, 4)`, dtype `uint64`, laid out as `fill_blocks` writes it. `ValueError` if the output size overflows, `MemoryError` if the output buffer cannot be allocated. A very large `steps` gives `ValueError` also for an empty batch, because NumPy rejects the shape `(steps, 0, 4)` (the limit depends on NumPy; Rust and C return an empty result there) |
| `LustroPrngBatch.lane(i)`                                 | `i` | Python `int`, `0 ≤ i < len(batch)` → returns a new `LustroPrng` at the lane's next block, independent of the batch. `IndexError` if `i >= len(batch)`, `OverflowError` if negative or above `2⁶⁴ - 1`, `TypeError` for a non-int |
| `LustroPrngBatch[i]`                                      | `i` | Python `int` (or an object with `__index__`), `-len(batch) ≤ i < len(batch)`; a negative `i` counts from the end → returns a new `LustroPrng`, the same as `lane(i + len(batch))`. `IndexError` if out of range (also if it does not fit a signed 64-bit integer, as for a list), `TypeError` for a slice or a non-int |
| `iter(LustroPrngBatch)`                                   | — | Yields `lane(0)`, `lane(1)`, … in order, from a copy of the lane states taken by `iter()` (later calls on the batch do not affect it). `MemoryError` if the copy cannot be allocated |
| `LustroPrngBatch.from_streams(streams)`                   | `streams` | Python sequence of `LustroPrng` (`list`, `tuple`, a batch, ...) (staticmethod) → returns a batch with one lane per stream, lane `i` continuing `streams[i]` at its next block; the streams are not changed. `ValueError` if a stream is inside a block, `TypeError` for a non-sequence (such as a generator or a single stream) or an item that is not a `LustroPrng`, `RuntimeError` if a stream is in use in another thread, `MemoryError` if the lanes cannot be allocated |
| `LustroPrngBatch.fork(ids)`                               | `ids` | Python sequence of `int` (`list`, `tuple`, `range`, ...), `len(ids) == batch.len()` (`ValueError` otherwise), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise). `MemoryError` if the lanes cannot be allocated |
| `LustroPrngBatch.fork_many(ids)`                          | `ids` | Python sequence of `int` (`list`, `tuple`, `range`, ...), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise); `ValueError` if `len() * len(ids)` overflows. `MemoryError` if the lanes cannot be allocated |
| `LustroPrngBatch.fork_range(first)`                       | `first` | Python `int`, `0 ≤ first < 2¹²⁸` (`OverflowError` otherwise). `MemoryError` if the lanes cannot be allocated |
| `LustroPrngBatch.derive_path(seed, roots, path)`          | `seed` | Python `bytes`, exactly 32 bytes (`ValueError` otherwise) (staticmethod) |
|                                                           | `roots`, `path` | Python sequences of `int` (`list`, `tuple`, `range`, ...); `path` non-empty (`ValueError` if empty), `roots` may be empty (empty batch); each id `0 ≤ id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`). `MemoryError` if the lanes cannot be allocated |
| `LustroPrngBatch.copy()` / `LustroXofBatch.copy()`        | — | returns an independent batch (`MemoryError` if the lanes cannot be allocated); also `copy.copy()` / `copy.deepcopy()` |
| `LustroPrngBatch.export_snapshot()`                       | — | returns `bytes`, variable length |
| `LustroPrngBatch.import_snapshot(bytes)`                  | `bytes` | Python `bytes`, variable length (staticmethod) |
| `LustroXof(message)`                                      | `message` | Python `bytes`, any length (including empty) |
| `LustroXof.fill(size)`                                    | `size` | Python `int` → returns `bytes` of that length (`OverflowError` if negative or above `isize::MAX`, `MemoryError` if the buffer cannot be allocated) |
| `LustroXof.advance_bytes(n)`, `advance_blocks(n)`         | `n` | Python `int`, `0 ≤ n < 2⁶⁴` (`OverflowError` if negative or above `2⁶⁴ - 1`). Costs O(n), the GIL is released. Returns `None` |
| `LustroXof.position()`                                    | — | returns Python `int`: bytes taken from the start |
| `LustroXof.fill_into(buf)`                                | `buf` | writable, C-contiguous buffer of bytes (e.g. `bytearray`, `memoryview`), filled in place to its full length (`TypeError` otherwise) |
| `LustroXof.read(size)`                                    | `size` | Python `int`, same as `fill(size)` → returns `bytes` of that length (`OverflowError` if negative; there is no form without `size`) |
| `LustroXof.readinto(buf)`                                 | `buf` | writable, C-contiguous buffer with any item format (e.g. `bytearray`, `memoryview`, `array('I')`, NumPy array of any dtype), filled in place to its full length with raw stream bytes, without conversion → returns the number of bytes written (not items). `TypeError` for `bytes`, objects without a buffer, read-only or non-contiguous buffers; the stream is not moved then. An empty buffer returns `0` and does not move the stream |
| `LustroXof.fill_u64(n)`                                   | `n` | Python `int`, `n >= 0` (`OverflowError` if negative, above `2⁶⁴ - 1`, or if `8 * n` exceeds `isize::MAX`) → returns a new writable `numpy.ndarray`, shape `(n,)`, dtype `uint64`, equal to `n` calls of `next_u64()`. `MemoryError` if the output buffer cannot be allocated |
| `LustroXof.fork(id)`                                      | `id` | Python `int`, `0 ≤ id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`) |
| `LustroXof.fork_batch(ids)`                               | `ids` | Python sequence of `int` (`list`, `tuple`, `range`, ...), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise) → returns a `LustroXofBatch` whose lane `i` is `fork(ids[i])`. `MemoryError` if the lanes cannot be allocated |
| `LustroXof.fork_batch_range(first, count)`                | `first`, `count` | `first` Python `int`, `0 ≤ first < 2¹²⁸`; `count` Python `int`, `0 ≤ count < 2⁶⁴` (`OverflowError` otherwise) → as `fork_batch` with ids `first`, `first + 1`, ... (modulo 2¹²⁸). `MemoryError` if the lanes cannot be allocated |
| `LustroXof.fork_path(path)`                               | `path` | Python sequence of `int`, non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise) → returns `fork(path[0]).fork(path[1])...` |
| `LustroXof.export_snapshot()`                             | — | returns `bytes`, exactly 56 bytes |
| `LustroXof.import_snapshot(bytes)`                        | `bytes` | Python `bytes`, exactly 56 bytes (staticmethod) |
| `LustroXof.derive_path(message, path)`                    | `message` | Python `bytes`, any length (staticmethod) |
|                                                           | `path` | Python sequence of `int` (`list`, `tuple`, `range`, ...), non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`) |
| `LustroXofBatch(messages)`                                | `messages` | sequence of `bytes` or byte buffers, each element independently sized (`TypeError` for a single string or buffer, a non-sequence such as a generator, or an item that is not a byte buffer). `MemoryError` if the lanes cannot be allocated |
| `LustroXofBatch.fill_blocks(out, steps, *, release_gil=True)` | `out`, `steps` | writable, C-contiguous `numpy.ndarray`, shape `(steps, n, 4)`, dtype `uint64`, `n == len(batch)`; Python `int`. Wrong type → `TypeError`; shape mismatch or non-C-contiguous → `ValueError`. For non-empty output the data must also be aligned for `uint64`: a misaligned buffer (for example `np.frombuffer` at an odd offset) raises `ValueError` |
| `LustroXofBatch.blocks(steps)`                            | `steps` | Python `int`, `steps >= 0` (`OverflowError` if negative or above `2⁶⁴ - 1`) → returns a new writable, C-contiguous `numpy.ndarray`, shape `(steps, n, 4)`, dtype `uint64`, laid out as `fill_blocks` writes it. `ValueError` if the output size overflows, `MemoryError` if the output buffer cannot be allocated. A very large `steps` gives `ValueError` also for an empty batch, because NumPy rejects the shape `(steps, 0, 4)` (the limit depends on NumPy; Rust and C return an empty result there) |
| `LustroXofBatch.lane(i)`                                  | `i` | Python `int`, `0 ≤ i < len(batch)` → returns a new `LustroXof` at the lane's next block, independent of the batch. `IndexError` if `i >= len(batch)`, `OverflowError` if negative or above `2⁶⁴ - 1`, `TypeError` for a non-int |
| `LustroXofBatch[i]`                                       | `i` | Python `int` (or an object with `__index__`), `-len(batch) ≤ i < len(batch)`; a negative `i` counts from the end → returns a new `LustroXof`, the same as `lane(i + len(batch))`. `IndexError` if out of range (also if it does not fit a signed 64-bit integer, as for a list), `TypeError` for a slice or a non-int |
| `iter(LustroXofBatch)`                                    | — | Yields `lane(0)`, `lane(1)`, … in order, from a copy of the lane states taken by `iter()` (later calls on the batch do not affect it). `MemoryError` if the copy cannot be allocated |
| `LustroXofBatch.from_streams(streams)`                    | `streams` | Python sequence of `LustroXof` (`list`, `tuple`, a batch, ...) (staticmethod) → as `LustroPrngBatch.from_streams`, for `LustroXof` items |
| `LustroXofBatch.fork(ids)`                                | `ids` | Python sequence of `int` (`list`, `tuple`, `range`, ...), `len(ids) == batch.len()` (`ValueError` otherwise), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise). `MemoryError` if the lanes cannot be allocated |
| `LustroXofBatch.fork_many(ids)`                           | `ids` | Python sequence of `int` (`list`, `tuple`, `range`, ...), each `0 ≤ id < 2¹²⁸` (`OverflowError` otherwise); `ValueError` if `len() * len(ids)` overflows. `MemoryError` if the lanes cannot be allocated |
| `LustroXofBatch.fork_range(first)`                        | `first` | Python `int`, `0 ≤ first < 2¹²⁸` (`OverflowError` otherwise). `MemoryError` if the lanes cannot be allocated |
| `LustroXofBatch.derive_path(messages, path)`              | `messages` | sequence of `bytes` or byte buffers (staticmethod; `TypeError` as for `hash256_many_var`) |
|                                                           | `path` | Python sequence of `int` (`list`, `tuple`, `range`, ...), non-empty (`ValueError` if empty), each `0 ≤ id < 2¹²⁸` (`OverflowError` if negative or `≥ 2¹²⁸`). `MemoryError` if the lanes cannot be allocated |
| `LustroXofBatch.export_snapshot()`                        | — | returns `bytes`, variable length |
| `LustroXofBatch.import_snapshot(bytes)`                   | `bytes` | Python `bytes`, variable length (staticmethod) |

**Python-Specific Notes**

- The following methods release the GIL while the underlying Rust computation
  runs (`py.detach`), allowing other Python threads to run concurrently:
  `hash256`/`hash128` (inputs of 4096 bytes or more), `hash*_many_fixed`, `hash*_many_fixed_into`, `hash*_many_var`, `hash*_many_var_into`;
  `LustroPrng`/`LustroXof.fill`, `read`, `fill_u64`, `fill_into` and `readinto` (buffers of 4096 bytes or more), `advance_bytes`, `advance_blocks`; `LustroPrngBatch`/`LustroXofBatch.fill_blocks`, `blocks`.
  With `release_gil=False` (`hash*_many_fixed_into`, `hash*_many_var`, `hash*_many_var_into`,
  `fill_blocks`) the GIL stays held, input is read in place where its type and layout
  allow, and output is written directly into `out` (or the returned array); other Python
  threads wait. This is the maximum-throughput mode, not a universal no-copy mode: see the
  notes below. For `fill_blocks` the flag has an effect on little-endian targets only.
  `fill_into()` and `readinto()` write into the caller's buffer through an `unsafe` borrow. For
  buffers below 4096 bytes the GIL stays held (releasing it costs more than the write). From 4096
  bytes it is released; the buffer view held during the write blocks resizing of the exporter
  (`bytearray` raises `BufferError`) and the call itself keeps the object alive. A second call on
  the same stream meanwhile raises `RuntimeError: Already borrowed`. Other threads that write
  to the bytes of the same buffer are not synchronized.
- `import_snapshot(bytes)` raises `lustro.SnapshotError` (a subclass of
  `ValueError`) on malformed input: wrong length, unsupported version, kind
  mismatch (e.g. a PRNG snapshot passed to `LustroXof`) or invalid cursor.
  It maps the Rust `SnapshotError` variants.
- Pickling goes through the snapshot: `__reduce__` returns
  `(Class.import_snapshot, (export_snapshot(),))`. A restored object continues
  from the pickled position, so it is a copy of the stream, not a new one.
  The internal `fill_blocks` buffer of a batch is not pickled. Loading a pickle
  with an invalid snapshot raises `lustro.SnapshotError`.
- Batch `lane(i)` reports an out-of-range index with `IndexError`, where Rust
  returns `None` and C returns `InvalidArgument`. This is the usual Python
  convention for an invalid index; the three cases mean the same.
- In Python, `batch[i]` is `lane(i)` with list-style indexes: a negative `i`
  counts from the end, so `batch[-1]` is the last lane. `lane(i)` itself keeps
  rejecting a negative `i`. `iter(batch)` works on a copy of the lane states taken
  at that call and yields `lane(0)`, `lane(1)`, … from it. Slices are not supported.
- A batch is a sequence of its lanes, so `from_streams(batch)` gives a copy of `batch`.
- Each 32-byte block, where represented as NumPy output, is four `uint64`
  values in little-endian order, the same bytes as the raw stream.
- `hash*_many_fixed` copies its rows before hashing; `hash*_many_var` reads `bytes`
  items without copying and copies other byte buffers. By default `hash*_many_fixed_into`
  hashes a copy of `data` and copies the digests into `out` after the GIL is taken back.
- With `release_gil=False`, `hash*_many_fixed_into` reads a C-contiguous `data` in place.
  A view that is not C-contiguous as a whole (for example `a[:, :16]`) is copied and hashed
  with the GIL released, as in the default mode. `hash*_many_var` and `hash*_many_var_into`
  read an exact `list` or `tuple` of exact `bytes` without taking a reference on each item;
  other input is handled as by default.
- The same array, or views of one array, as `data` and `out` of `hash*_many_fixed_into`
  raise `TypeError` in both modes (NumPy borrow check). Memory shared through separate
  wrappers (two `np.frombuffer` views over one `bytearray`; a `memoryview` of `out` used as
  a message of `hash*_many_var_into`) raises `ValueError` with `release_gil=False`; by default
  the input is copied first and the call is allowed.
- With `release_gil=False`, an exact `list` or `tuple` of 16384 or more items given to
  `hash*_many_var_into` is hashed while it is read, so `out` may be partly written if the
  call then fails. By default nothing is written before all items are read.
- That read-while-hashing (fused) path is used only for an exact `list` or `tuple` of 16384
  or more items that are all exact `bytes`, and only when the dispatcher chooses the thread
  pool. Any other call takes the ordinary path.
- Batch `fill_blocks` with the default `release_gil=True` keeps an internal block buffer
  between calls; `release_buffer()` frees it. `release_gil=False` and `blocks` do not use it.
- `fill_u64` and `blocks` leave the stream state unchanged when they fail with
  `OverflowError`, `ValueError` or `MemoryError`. The exception is a failure while the NumPy
  array object is created, which happens after generation.
- Methods take `&mut self`: overlapping use of one object from several
  threads raises `RuntimeError` ("Already borrowed").
- `repr()` is redacted: `LustroPrng(<redacted>)`, `LustroXof(<redacted>)`, `LustroPrngBatch(len=N)`, `LustroXofBatch(len=N)`.
- `configure()` is best called before the first batch call or `runtime_info()`, which fix the settings (`suggested_steps()` 
  and `runtime_preview()` do not). A later call returns `None` if it asks for the same effect and raises `RuntimeError` otherwise. 
  `True` is accepted as `threads` and counts as 1.

All other behavioral guarantees (fork step=0, hash128 truncation, snapshot
resume, concatenation guarantee) are as described in
[§10 Behavioral Guarantees](#10-behavioral-guarantees).

---

### 12.3 C FFI Reference

C ABI exported through `extern "C"` functions, compatible with C and C++.

**Function Table**

| Module | Functions |
|---|---|
| API Version, errors, runtime | `lustro_api_version()`, `lustro_strerror(code)`, `lustro_runtime_threads()`, `lustro_runtime_backend()`, `lustro_runtime_info(out, out_size)`, `lustro_runtime_preview(out, out_size)`, `lustro_configure(threads, simd)` |
| Entropy | `lustro_seed_from_entropy(out_seed)`, `lustro_prng_from_entropy(stream_id_hi, stream_id_lo, out_ctx)` |
| Hash | `lustro_hash256(data, data_len, out)`, `lustro_hash128(data, data_len, out)` |
| Hash Batch | `lustro_hash256_many_fixed(data_ptr, n, message_len, out_ptr)`, `lustro_hash128_many_fixed(data_ptr, n, message_len, out_ptr)`, `lustro_hash256_many_var(message_ptrs, n, message_lens, out_ptr)`, `lustro_hash128_many_var(message_ptrs, n, message_lens, out_ptr)` |
| PRNG | `lustro_prng_new(seed, stream_id_hi, stream_id_lo, out_ctx)`, `_free`, `_clone(out_ctx)`, `_fill(out, out_len)`, `_next_u32`, `_next_u64`, `_next_u128`, `_next_block`, `_advance_bytes(n)`, `_advance_blocks(n)`, `_position(out)`, `_fork(id_hi, id_lo, out_ctx)`, `_fork_batch(ids_hi, ids_lo, n, out_ctx)`, `_fork_batch_range(first_hi, first_lo, count, out_ctx)`, `_fork_path(ids_hi, ids_lo, n, out_ctx)`, `_derive_path(seed, ids_hi, ids_lo, n, out_ctx)`, `_export_snapshot(out)`, `_import_snapshot(bytes, out_ctx)` |
| PRNG Batch | `lustro_prng_batch_new(seed, ids_hi, ids_lo, n, out_ctx)`, `_new_range(seed, first_hi, first_lo, count, out_ctx)`, `_free`, `_clone(out_ctx)`, `_len`, `_get_len(out_len)`, `_fill_blocks(out, out_len, steps)`, `_lane(index, out_ctx)`, `_from_streams(streams, n, out_ctx)`, `_fork(ids_hi, ids_lo, n, out_ctx)`, `_fork_many(ids_hi, ids_lo, k, out_ctx)`, `_fork_range(first_hi, first_lo, out_ctx)`, `_derive_path(seed, roots_hi, roots_lo, n_roots, path_hi, path_lo, n_path, out_ctx)`, `_snapshot_size`, `_suggested_steps`, `_export_snapshot(out, out_len)`, `_import_snapshot(bytes, len, out_ctx)` |
| XOF | `lustro_xof_new(message, message_len, out_ctx)`, `_free`, `_clone(out_ctx)`, `_fill(out, out_len)`, `_next_u32`, `_next_u64`, `_next_u128`, `_next_block`, `_advance_bytes(n)`, `_advance_blocks(n)`, `_position(out)`, `_fork(id_hi, id_lo, out_ctx)`, `_fork_batch(ids_hi, ids_lo, n, out_ctx)`, `_fork_batch_range(first_hi, first_lo, count, out_ctx)`, `_fork_path(ids_hi, ids_lo, n, out_ctx)`, `_derive_path(message, message_len, ids_hi, ids_lo, n, out_ctx)`, `_export_snapshot(out)`, `_import_snapshot(bytes, out_ctx)` |
| XOF Batch | `lustro_xof_batch_new(message_ptrs, message_lens, n, out_ctx)`, `_free`, `_clone(out_ctx)`, `_len`, `_get_len(out_len)`, `_fill_blocks(out, out_len, steps)`, `_lane(index, out_ctx)`, `_from_streams(streams, n, out_ctx)`, `_fork(ids_hi, ids_lo, n, out_ctx)`, `_fork_many(ids_hi, ids_lo, k, out_ctx)`, `_fork_range(first_hi, first_lo, out_ctx)`, `_derive_path(message_ptrs, message_lens, n_messages, path_hi, path_lo, n_path, out_ctx)`, `_snapshot_size`, `_suggested_steps`, `_export_snapshot(out, out_len)`, `_import_snapshot(bytes, len, out_ctx)` |

`fork_range(ctx, first_hi, first_lo)` variants do not take a count. They
derive `ctx.len()` children with sequential IDs starting at `first`, equivalent
to calling `*_fork` with `ids = [first, first+1, ..., first+len()-1]`.
Likewise, `*_batch_new_range(seed, first_hi, first_lo, count)` is equivalent
to `*_batch_new(seed, ids, count)` with `ids = [first, ..., first+count-1]`.
IDs advance with wrapping `u128` arithmetic, so a range near `u128::MAX`
wraps back to `0`. The same applies to the Rust and Python APIs.

`lustro_prng_batch_lane(ctx, index, out_ctx)` and `lustro_xof_batch_lane(...)`
copy lane `index` into a new **single-stream** context (`LustroPrng*` /
`LustroXof*`), positioned at the lane's next block. The batch is not changed.
Free the result with `lustro_prng_free` / `lustro_xof_free`, not with
`*_batch_free`. An `index` not below `lustro_*_batch_len(ctx)` returns
`InvalidArgument`; `ctx` and `out_ctx` are checked first.

`lustro_prng_batch_from_streams(streams, n, out_ctx)` and
`lustro_xof_batch_from_streams(...)` take an array of `n` single-stream contexts
(`const LustroPrng *const *` / `const LustroXof *const *`) and create a batch
with one lane per stream, in order; the streams are not changed and stay owned
by the caller. A stream inside a block returns `InvalidArgument`; the pointers
(`out_ctx`, the array, each element) are checked first.

**1. Type Mapping**

| C Type | Rust Origin | Notes |
|---|---|---|
| `uint8_t*` | `*const u8` / `*mut u8` | byte buffer, in or out |
| `uintptr_t` | `usize` | platform-dependent width |
| `uint64_t` | `u64` | `stream_id_hi`/`_lo` components; `n` of `*_advance_bytes` and `*_advance_blocks` |
| `int32_t` | `LustroError` (`#[repr(i32)]`) | see Return Value Conventions |
| `const char*` | `*const c_char` | static NUL-terminated string, `lustro_strerror()`, `lustro_runtime_backend()` and the `backend` field of `LustroRuntimeInfo`; do not free |
| `struct LustroRuntimeInfo*` | `*mut LustroRuntimeInfo` (`#[repr(C)]`) | `out` of `lustro_runtime_info()` and `lustro_runtime_preview()`, with its `sizeof` as `out_size`; must be non-null and aligned |
| `uint32_t` | `u32` | `lustro_api_version()` return value; `out` of `*_next_u32` |
| opaque pointer | `*mut LustroPrng` / `*mut LustroPrngBatch` / `*mut LustroXof` / `*mut LustroXofBatch` | never dereference from C; pass through only |
| `struct LustroPrng**` (and the other three handle types) | `*mut *mut LustroPrng` | `out_ctx` of the constructors; written only if non-null and aligned |
| `const uint8_t* const*` | `&[&[u8]]` (array of message slices) | Used by variable-length message APIs such as `*_many_var(...)` and `lustro_xof_batch_new(...)`, together with a parallel `const uintptr_t*` of lengths |
| `const struct LustroPrng* const*` (and `LustroXof`) | `*const *const LustroPrng` | `streams` of `*_batch_from_streams`: an array of `n` contexts, each non-null and aligned |

Input/output byte buffers are checked for `NULL`. Pointers to wider element
types (`uint64_t` id arrays, `uintptr_t` length arrays, pointer tables) must
also be naturally aligned; a misaligned pointer is rejected. Byte buffers
(`uint8_t*`) have no alignment requirement. The same alignment rule applies to
the `out_ctx` of a constructor: if it is `NULL` or misaligned, the constructor
returns `InvalidPointer` and writes nothing. Hash functions
(`lustro_hash256*`/`lustro_hash128*`) additionally
reject overlapping input/output ranges with `InvalidPointer`; for `*_many_var`
this includes an overlap between `out` and the `message_ptrs`/`message_lens`
tables. Scalar writes such as `*_next_u64` use `write_unaligned`.

**2. Calling Convention**

All exported functions use `extern "C"` and expose the platform's standard
C ABI. Do not bind as `stdcall`.

**3. Return Value Conventions**

There are five categories:

- **`LustroError` (`int32`)**: returned by hash, fill, next-value, advance,
  position and snapshot-export functions, `*_batch_get_len`, constructors, `lustro_configure`, `lustro_runtime_info`,
  `lustro_runtime_preview` and `lustro_seed_from_entropy`.

  | Value | Name | Meaning |
  |---|---|---|
  | 0 | `Ok` | success |
  | 1 | `InvalidLength` | buffer/length argument invalid |
  | 2 | `InvalidPointer` | required pointer is `NULL` or misaligned, or (hash functions only) an input/output range overlaps another (see Type Mapping above) |
  | 3 | `OutputTooSmall` | reserved, not returned by V1 (`InvalidLength` is used instead) |
  | 4 | `AlreadyFinalised` | reserved, not returned by V1 |
  | 5 | `VerificationFailed` | reserved, not returned by V1 |
  | 6 | `InternalPanic` | internal panic caught at FFI boundary |
  | 7 | `UnsupportedVersion` | snapshot version byte is not supported (constructors only) |
  | 8 | `InvalidKind` | snapshot kind does not match the type (constructors only) |
  | 9 | `InvalidCursor` | snapshot cursor is outside `1..=32` (constructors only) |
  | 10 | `OutOfMemory` | reserving a batch lane buffer or an input table failed (constructors only) |
  | 11 | `AlreadyConfigured` | runtime settings are already fixed with another effect (`lustro_configure` only) |
  | 12 | `InvalidArgument` | an argument value is not allowed, nothing was changed (`lustro_configure`, `*_batch_lane`, `*_batch_from_streams`) |
  | 13 | `EntropyUnavailable` | the system entropy source is unavailable (`lustro_seed_from_entropy`, `lustro_prng_from_entropy`) |

  Hash, fill, next-value and export functions use only `Ok`, `InvalidLength`,
  `InvalidPointer` and `InternalPanic`; codes 7–10 are reported only by
  constructors, code 11 only by `lustro_configure`, code 12 only by
  `lustro_configure`, `*_batch_lane` and `*_batch_from_streams`, code 13 only by the entropy functions.
  In Rust the enum is `#[non_exhaustive]`; C code should treat an unknown value
  as a generic failure.
  V1 does not distinguish an undersized output buffer from other invalid-length
  arguments. Both return `InvalidLength`; output lengths are checked against
  the exact expected size.

- **Constructors** (`*_new`, `*_new_range`, `*_clone`, `*_fork`, `*_fork_many`,
  `*_fork_range`, `*_fork_batch`, `*_fork_batch_range`, `*_fork_path`, `*_batch_lane`,
  `*_batch_from_streams`, `*_derive_path`, `*_import_snapshot`,
  `lustro_prng_from_entropy`) return
  a `LustroError` and store the new context through a trailing `T **out_ctx`.
  `*out_ctx` is set to `NULL` first and receives the context only when the
  status is `Ok`. If `out_ctx` itself is `NULL` or misaligned, the function
  returns `InvalidPointer` and writes nothing. `out_ctx` must be valid for
  writing one pointer and must not overlap any input buffer or the memory of
  `ctx`.

  | Status | Reported when |
  |---|---|
  | `InvalidPointer` | a required pointer is `NULL` (`out_ctx`, `ctx`, `seed`, and byte buffers of nonzero length), or an array of `u64`, `usize` or pointers of nonzero length is `NULL` or misaligned, or `out_ctx` is misaligned, or an element of `streams` in `*_batch_from_streams` is `NULL` or misaligned |
  | `InvalidLength` | empty path (`n == 0`, `n_path == 0`); a count or length too large to address; `n` differs from the batch length in `*_batch_fork`; `len() * k` overflows in `*_batch_fork_many`; a batch snapshot length does not match (single-stream `*_import_snapshot` takes a fixed 56 bytes and never reports it) |
  | `InvalidArgument` | `index` is not below the batch length in `*_batch_lane`; a stream is inside a block in `*_batch_from_streams`; nothing is created |
  | `EntropyUnavailable` | `lustro_prng_from_entropy`: the system entropy source is unavailable; nothing is created |
  | `UnsupportedVersion`, `InvalidKind` | `*_import_snapshot`, the same conditions as in `SnapshotError` |
  | `InvalidCursor` | `lustro_prng_import_snapshot`, `lustro_xof_import_snapshot` (batch snapshots carry no cursor) |
  | `OutOfMemory` | the lane buffer or an input table (identifiers, messages) cannot be reserved (`*_derive_path`, `*_fork_batch`, `*_fork_batch_range`, `*_fork_path`, `*_batch_new`, `*_batch_new_range`, `*_batch_from_streams`, `*_batch_clone`, `*_batch_fork`, `*_batch_fork_many`, `*_batch_fork_range`, `*_batch_derive_path`); covers allocator refusal and capacity overflow |
  | `InternalPanic` | a panic was caught at the FFI boundary |

  `OutOfMemory` is reported only for these explicit reservations. Other
  allocations (the context itself and the internal buffers of
  `*_batch_import_snapshot`) are not fallible; their failure aborts the process,
  as in any Rust code.

- **Value-returning functions** (`*_len`, `*_snapshot_size`, `*_suggested_steps`,
  `lustro_runtime_threads` → `uintptr_t`; `lustro_api_version` → `uint32_t`;
  `lustro_strerror`, `lustro_runtime_backend` → `const char*`): value returned
  directly, no error path. `*_len`, `*_snapshot_size` and `*_suggested_steps`
  return 0 for a `NULL` context (`*_suggested_steps` also after a caught panic);
  a valid batch gets a `steps` of at least 1. For a length that does not mix up `NULL`
  and an empty batch, `*_batch_get_len(ctx, &len)` writes it through a pointer and returns
  `InvalidPointer` for a `NULL` `ctx` or `out_len`. `lustro_strerror(int32_t code)` returns a static
  NUL-terminated description, never `NULL` and not to be freed; an unknown `code`
  gives `"unknown error code"`. `lustro_runtime_backend()` returns `"scalar"` or
  `"avx2"` under the same rules. `lustro_runtime_threads()` and
  `lustro_runtime_backend()` fix the runtime settings like `runtime_info()`.

- **`lustro_runtime_info(LustroRuntimeInfo *out, size_t out_size)`, `lustro_runtime_preview(LustroRuntimeInfo *out, size_t out_size)`**
  store the report of `runtime_info()` / `runtime_preview()` in `*out` and return a
  `LustroError`: `Ok`; `InvalidPointer` for a `NULL` or misaligned `out`; `InvalidLength`
  if `out_size` is too small for the fields up to `pool_usable`; `InternalPanic` after a
  caught panic. Nothing is written on an error. `lustro_runtime_info` fixes the
  runtime settings, `lustro_runtime_preview` does not (`fixed` is 0 until they are
  fixed). Fields: `uintptr_t threads`, `uintptr_t logical_cpus`, `const char *backend`
  (static, never `NULL`, do not free), then `uint8_t fixed`, `parallel_built`,
  `simd_available`, `pool_usable`, each 0 or 1; meanings as in `RuntimeInfo`. `out_size`
  is `sizeof(LustroRuntimeInfo)` from the caller's header. Only the fields that fit in it
  are written, so a struct from an older header is never overflowed. Fields the library
  does not know are left unchanged (zero the struct first).

- **`lustro_seed_from_entropy(uint8_t *out_seed)`** writes 32 bytes from the
  operating system's entropy source. `InvalidPointer` for `NULL`; `EntropyUnavailable`
  when the source is unavailable, and then `out_seed` is not written.
  **`lustro_prng_from_entropy(stream_id_hi, stream_id_lo, out_ctx)`** works as
  `lustro_prng_new` with such a seed, which is not kept; `*out_ctx` is `NULL` on
  failure; `InvalidPointer` for a `NULL` or misaligned `out_ctx`, `EntropyUnavailable`
  when the source is unavailable. Both need the `entropy` feature, which `ffi` enables.

- **`lustro_configure(uintptr_t threads, int32_t simd)`** sets the execution
  settings of the process (best before first use) and returns a `LustroError`. `threads`:
  `0` = not set (follows the environment), `1` = no pool, larger values are capped
  at the logical CPU count. `simd`: `LUSTRO_SIMD_KEEP` (0) follows
  `LUSTRO_DISABLE_SIMD`, `LUSTRO_SIMD_AUTO` (1) allows AVX2, `LUSTRO_SIMD_SCALAR` (2)
  selects the scalar backend. A value given here replaces the environment variables
  of the same purpose.

  | Status | Reported when |
  |---|---|
  | `Ok` | the settings were set, or they were already fixed with the same effect as the requested ones |
  | `AlreadyConfigured` | the settings are already fixed with another effect (by `lustro_configure`, a batch call, `lustro_runtime_info()`, `lustro_runtime_threads()` or `lustro_runtime_backend()`). `*_batch_suggested_steps` does not fix them |
  | `InvalidArgument` | `simd` is not 0, 1 or 2; nothing is changed, also when the settings are already fixed |
  | `InternalPanic` | a panic was caught at the FFI boundary |

  Without the `parallel` feature `threads` is accepted and has no effect.

- **void-returning functions** (`*_free`): no return value; safe to call
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
| `*_position` | `out` | 16 bytes |
| `lustro_seed_from_entropy` | `out_seed` | 32 bytes |
| `*_next_block` | `out` | 32 bytes |
| `*_export_snapshot` (single-context, PRNG/XOF) | `out` | 56 bytes |
| `*_batch_fill_blocks` | `out` | `n_lanes × steps × 32` bytes, the size of a `uint8_t [steps][n_lanes][32]` array. The block of `lane` at `step` starts at byte `lustro_batch_block_offset(step, n_lanes, lane)` |
| `*_batch_export_snapshot` | `out` | `16 + n_lanes × 48` bytes; call `*_batch_snapshot_size(ctx)` first |

Input buffers:

| Function family | Buffer | Size |
|---|---|---|
| `lustro_prng_new` / `lustro_prng_batch_new` / `lustro_prng_batch_new_range` | `seed` | 32 bytes, fixed |
| `lustro_prng_derive_path` / `lustro_prng_batch_derive_path` | `seed` | 32 bytes, fixed |

**5. Ownership / Lifetime**

| Handle type | Created by | Freed by |
|---|---|---|
| `LustroPrng*` | `_new`, `_clone`, `_fork`, `_fork_path`, `_derive_path`, `_import_snapshot`, `lustro_prng_from_entropy`, `lustro_prng_batch_lane` | `lustro_prng_free` |
| `LustroPrngBatch*` | `_new`, `_new_range`, `_from_streams`, `_clone`, `_fork`, `_fork_many`, `_fork_range`, `_derive_path`, `_import_snapshot`, `lustro_prng_fork_batch`, `lustro_prng_fork_batch_range` | `lustro_prng_batch_free` |
| `LustroXof*` | `_new`, `_clone`, `_fork`, `_fork_path`, `_derive_path`, `_import_snapshot`, `lustro_xof_batch_lane` | `lustro_xof_free` |
| `LustroXofBatch*` | `_new`, `_from_streams`, `_clone`, `_fork`, `_fork_many`, `_fork_range`, `_derive_path`, `_import_snapshot`, `lustro_xof_fork_batch`, `lustro_xof_fork_batch_range` | `lustro_xof_batch_free` |

`*_import_snapshot` produces an independent context with the same ownership
and cleanup rules as a context created by `*_new`.

The caller owns a context only when the constructor returned `Ok`. On failure
`*out_ctx` is `NULL` and there is nothing to free.

**6. Null / Zero-Length Semantics**

The `n == 0` behavior depends on the return-value category. It does **not**
make every pointer argument optional; only array/data pointers whose length
is determined by `n` may be `NULL`:

- **Hash and fill functions** (`*_many_fixed`, `*_many_var`,
  `*_batch_fill_blocks`): with `n == 0`, the `*_many_*` functions return
  `LustroError::Ok` without dereferencing the data/output pointers; these
  pointers may be `NULL`.
  `*_batch_fill_blocks` first checks `out_len == n_lanes × steps × 32`
  (so `out_len` must be 0 when the batch is empty or `steps == 0`), then
  returns `Ok` without dereferencing `out`; in that case only `out` may be
  `NULL`. For `n > 0`, a required `NULL` pointer returns `InvalidPointer`.
- **Batch constructors and batch fork operations** (`*_batch_new`,
  `*_batch_new_range`, `*_batch_fork_many`, etc.): with the relevant count
  at 0 (`n`, `count`, or `k` for `*_batch_fork_many`), the function returns `Ok`
  and stores a valid empty context. `*_batch_fork` additionally requires
  `n == len(ctx)`, so `n == 0` succeeds only for an empty batch. For example,
  `lustro_xof_batch_new(..., n = 0, &out)` stores an empty `LustroXofBatch`.
  The same holds for `*_batch_from_streams` with `n == 0`; the `streams` array
  may then be `NULL`.
- **Fixed-size inputs unrelated to `n` still require a valid pointer.**
  For example, `lustro_prng_batch_new(seed, ids_hi, ids_lo, n, out_ctx)` always
  requires a valid 32-byte `seed`; a `NULL` seed returns `InvalidPointer`, even when
  `n == 0`. Only the `n`-sized `ids_hi`/`ids_lo` arrays are exempt from the
  `NULL` check when `n == 0`.
- **`*_derive_path` and `*_fork_path` are the exception to "n == 0 is valid."**
  Unlike batch constructors, `lustro_*_derive_path` and `lustro_*_fork_path` treat
  `n == 0` (an empty path) as invalid and return `InvalidLength`, matching the Rust
  panic and the Python `ValueError` for an empty path. `lustro_*_fork_batch` with
  `n == 0` and `lustro_*_fork_batch_range` with `count == 0` give an empty batch.
- **`*_batch_derive_path` splits this in two.** `n_roots`/`n_messages` may be
  0 (empty batch, like other batch constructors), but `n_path` follows the
  single-stream rule above and must be nonzero (`InvalidLength` on an empty path).

**7. Thread Safety**

`LustroPrng`, `LustroPrngBatch`, `LustroXof`, and `LustroXofBatch` contain
generator state (plain integer fields; the batch types also own a vector of lane states) and
provide no internal synchronization.
Functions that change a context (`*_fill`, `*_next_*`, `*_advance_*`, `*_batch_fill_blocks`) need exclusive access to the handle. Using the same handle concurrently 
from multiple threads without external synchronization causes a data race. Use a separate 
handle per thread, or synchronize access to handles shared across threads.

**8. ctypes Buffer Mapping**

The repository's native Python bindings are implemented with PyO3. The C FFI can also be called from Python using ctypes, but this repository does not provide a ctypes wrapper. Such a wrapper must declare `argtypes` and `restype` for every function (ctypes assumes `int` otherwise, which truncates pointers), map `uintptr_t` to `ctypes.c_size_t`, and pass buffers as ctypes types, for example `ctypes.create_string_buffer()` or `(ctypes.c_uint8 * n)()` for writable output. Arrays of `uint64_t` or `uintptr_t` must be naturally aligned (a ctypes array is). The `DLL BINDING` section of `Lustro_Speed_Tester.py` declares the signatures of the C API this way.

**9. Header Distribution**

`lustro.h` is published with each release. Use the header matching the linked
library version; do not copy or edit it manually between versions.

`lustro.h` is generated by `build.rs` through `cbindgen` when the crate is
built with `--features ffi`. The header is written to cargo's `OUT_DIR`; the
source tree is modified only when `LUSTRO_HEADER_DIR` is set (path relative
to the crate root, e.g. `include`). The C FFI module is also gated by this
feature; Python bindings require `--features python`.

Besides declarations, `lustro.h` contains three `static inline` functions:
`lustro_batch_block_offset(step, batch_len, lane)`,
`lustro_u128_from_parts(hi, lo, out)` and `lustro_u128_to_parts(in, &hi, &lo)`.
They need C99 or C++ and are not exported from the library. None of them
validates its arguments. For `lustro_batch_block_offset` the caller keeps
`step < steps` and `lane < batch_len`, and `steps * batch_len * 32` must fit in
`size_t`. The two `u128` helpers need valid pointers (`out` and `in` are 16 bytes)
and do not depend on the host byte order.

**C FFI-Specific Behavioral Notes** (in addition to §10)

- `*_next_u128` writes 16 raw little-endian bytes from the same stream as
  `*_fill`. Byte 0 is the lowest byte of the value. 128-bit inputs have the other
  form: stream IDs are two `uint64_t`, `hi` (upper 64 bits) first, then `lo`, and
  ID arrays are parallel `*_hi` / `*_lo` arrays. `lustro_u128_from_parts` and
  `lustro_u128_to_parts` convert between the two forms. New C functions that
  return a 128-bit value use the 16-byte form.
- `*_fill` and `*_batch_fill_blocks` preserve the concatenation guarantee from
  §10. Batch output is step-major, so consecutive `*_batch_fill_blocks` calls
  concatenate directly.
- `*_import_snapshot` reports malformed input through the returned
  `LustroError` and leaves `*out_ctx` as `NULL`. Rust and Python report the same
  errors through `SnapshotError` and the corresponding Python exception.
- `*_batch_lane` returns the lane as a single stream: the next bytes it yields
  are the block that `*_batch_fill_blocks` would write for that lane next.
- `*_batch_from_streams` is the reverse: the first block `*_batch_fill_blocks`
  writes for lane `i` is the block `*_next_block` would return on `streams[i]`.

---

## 13. Security & Memory Model

1. **`Seed256` is `Copy`.** Copying it duplicates the underlying seed bytes.
   This version does not provide secret-memory protection.
2. **No automatic zeroization.** `Seed256` and internal stream state are not
   cleared on drop.
3. **Not a cryptographic generator.** Lustro makes no cryptographic claim. An
   output block is the stream state, so a full block and the step counter are
   enough to compute every following block.
4. **No hidden generator state.** Each PRNG/XOF instance owns its own state.
   The Rayon thread pool used for batch parallelism is shared execution state,
   not generator state (see §9, Thread pool).
5. **FFI/Python panic handling.** All release builds use `panic = "unwind"`. 
   FFI functions that are panic-guarded catch internal unwinds and return their 
   documented failure value: `LustroError::InternalPanic` for error-returning 
   functions, constructors included (`*out_ctx` stays `NULL`). Python bindings require
   `panic = "unwind"` so that Rust panics can be translated at the boundary;
   they surface as `pyo3_runtime.PanicException`, which derives from
   `BaseException`, not `Exception`.
   This does not protect against fatal process-level conditions or builds with a
   different panic strategy. Violating the FFI pointer preconditions, for example
   by passing an invalid non-null pointer, remains undefined behavior.
6. **Thread safety.** The underlying Rust contexts
   (`LustroPrng`, `LustroPrngBatch`, `LustroXof`, `LustroXofBatch`) provide no
   internal synchronization. Shared handles or objects must not be mutated
   concurrently without the synchronization required by the binding.
   For C FFI, this means caller-side synchronization around shared pointers;
   Python also applies the usual GIL and object-borrowing rules. The module declares 
   that it needs the GIL (`gil_used = true`); free-threaded CPython is not supported.
7. **Entropy.** `Seed256::from_entropy()` and `LustroPrng::from_entropy()` (`feature = "entropy"`) 
   read the operating system's entropy source through `getrandom` and never return a partially 
   filled seed. `LustroPrng::from_entropy()` and `from_entropy_with_id()` do not keep the seed. Python has `LustroPrng.from_entropy(stream_id=0)`
   (`OSError` on failure; the `python` feature enables `entropy`). C has `lustro_seed_from_entropy()` and `lustro_prng_from_entropy()` (the `ffi` feature enables `entropy`); an unavailable source is reported as `EntropyUnavailable`.