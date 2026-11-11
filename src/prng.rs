//! Lustro V1 — PRNG API.
//! Each instance is an independent stream.

use crate::api::{derive_branch_stream, derive_path_lane, fork_lane, prepare_base, StreamState};
use crate::constants::Domain;
use crate::dispatch::{dispatch_streams, suggested_steps, StreamLane};
use crate::types::{
    fork_many_lane_count, BatchError, DerivePathError, LustroPrngBatchSnapshot, LustroPrngSnapshot,
    Seed256, StreamId,
};
use std::collections::TryReserveError;

// rand_core (LustroPrng and LustroXof)
#[cfg(feature = "rand")]
#[path = "rand/rand_impl.rs"]
mod rand_impl;

// ==========================================
// RUST STREAM SINGLE API
// ==========================================

/// PRNG stream. Cloning preserves the exact stream state and future sequence.
#[must_use]
#[derive(Clone, Debug)]
pub struct LustroPrng {
    state: StreamState,
}

impl LustroPrng {
    /// Creates a stream from a seed and stream identifier.
    pub fn new(seed: &Seed256, stream_id: StreamId) -> Self {
        let (s0, s1) = seed.to_state();

        let (base_s0, base_s1) = prepare_base(s0, s1, Domain::Prng as u128);
        let (s0, s1) = derive_branch_stream(base_s0, base_s1, stream_id.get());

        Self {
            state: StreamState::new(s0, s1),
        }
    }

    /// Creates stream 0 of a seed taken from the operating system's entropy source.
    ///
    /// The seed is not kept, so the stream cannot be reproduced. To keep it, use
    /// `Seed256::from_entropy()` and `LustroPrng::new`. Fails when the source is unavailable.
    /// `LustroPrng` does not implement `CryptoRng`.
    #[cfg(feature = "entropy")]
    pub fn from_entropy() -> Result<Self, crate::types::EntropyError> {
        Self::from_entropy_with_id(StreamId(0))
    }

    /// As `from_entropy`, for the stream `stream_id` of the fresh seed.
    #[cfg(feature = "entropy")]
    pub fn from_entropy_with_id(stream_id: StreamId) -> Result<Self, crate::types::EntropyError> {
        Ok(Self::new(&Seed256::from_entropy()?, stream_id))
    }

    /// Returns the next 4 bytes of the stream as a little-endian `u32`.
    #[must_use]
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.state.read_bytes::<4>())
    }

    /// Returns the next 8 bytes of the stream as a little-endian `u64`.
    #[must_use]
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.state.read_bytes::<8>())
    }

    /// Returns the next 16 bytes of the stream as a little-endian `u128`.
    #[must_use]
    #[inline]
    pub fn next_u128(&mut self) -> u128 {
        u128::from_le_bytes(self.state.read_bytes::<16>())
    }

    /// Returns the next full 32-byte block. Unread bytes of the current block are discarded.
    #[must_use]
    #[inline]
    pub fn next_block(&mut self) -> [u8; 32] {
        self.state.read_full_block()
    }

    /// Fills `out` and advances the stream.
    pub fn fill_bytes(&mut self, out: &mut [u8]) {
        self.state.fill_bytes(out);
    }

    /// Skips `n` bytes: the same state as `fill_bytes` of `n` bytes, without writing them.
    /// Costs O(n); there is no jump-ahead, because each block is the input of the next.
    pub fn advance_bytes(&mut self, n: u64) {
        self.state.advance_bytes(n);
    }

    /// Skips `n` blocks: the same state as `n` calls of `next_block`. Unread bytes of the
    /// current block are discarded first (for `n > 0`). Costs O(n).
    pub fn advance_blocks(&mut self, n: u64) {
        self.state.advance_blocks(n);
    }

    /// Bytes taken from the start of the stream. Bytes discarded by `next_block` and
    /// `advance_blocks` count as taken. A fork and a fresh stream start at 0; an imported
    /// snapshot continues from its own position. Meaningless after 2^64 blocks, when the
    /// block counter wraps.
    #[must_use]
    pub fn position(&self) -> u128 {
        self.state.position()
    }

    /// Derives a child stream from the current state and identifier.
    /// The read position inside the current block does not affect the child: forking after
    /// 1 byte or after 31 bytes of the same block gives the same stream.
    pub fn fork(&self, id: StreamId) -> Self {
        Self {
            state: self.state.fork(Domain::Prng as u128, id.get()),
        }
    }

    /// Derives one child per identifier, as a batch: lane `i` is `fork(ids[i])`.
    /// Panics if the lane buffer cannot be allocated.
    pub fn fork_batch(&self, ids: &[StreamId]) -> LustroPrngBatch {
        match self.try_fork_batch(ids) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("fork_batch: lane allocation failed: {e:?}"),
            Err(e) => panic!("fork_batch: {e}"),
        }
    }

    /// As `fork_batch`, returning `Err(BatchError::Reserve)` if reserving the lane buffer fails.
    pub fn try_fork_batch(&self, ids: &[StreamId]) -> Result<LustroPrngBatch, BatchError> {
        self.try_fork_lanes(ids.len(), ids.iter().map(|id| id.get()))
    }

    /// As `fork_batch` with ids `first`, `first + 1`, ... (`count` lanes, wrapping modulo
    /// 2^128). Panics if the lane buffer cannot be allocated.
    pub fn fork_batch_range(&self, first: StreamId, count: usize) -> LustroPrngBatch {
        match self.try_fork_batch_range(first, count) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => {
                panic!("fork_batch_range: lane allocation failed: {e:?}")
            }
            Err(e) => panic!("fork_batch_range: {e}"),
        }
    }

    /// As `fork_batch_range`, returning `Err(BatchError::Reserve)` if reserving the lane
    /// buffer fails.
    pub fn try_fork_batch_range(
        &self,
        first: StreamId,
        count: usize,
    ) -> Result<LustroPrngBatch, BatchError> {
        let first = first.get();
        self.try_fork_lanes(count, (0..count).map(|i| first.wrapping_add(i as u128)))
    }

    // Children of the current state, one lane per id, each at step 0.
    fn try_fork_lanes(
        &self,
        count: usize,
        ids: impl Iterator<Item = u128>,
    ) -> Result<LustroPrngBatch, BatchError> {
        let (s0, s1, _, _) = self.state.to_parts();
        let mut streams = Vec::new();
        streams
            .try_reserve_exact(count)
            .map_err(BatchError::Reserve)?;
        let (base_s0, base_s1) = prepare_base(s0, s1, Domain::Prng as u128);
        streams.extend(ids.map(|id| {
            let (s0, s1) = derive_branch_stream(base_s0, base_s1, id);
            StreamLane { s0, s1, step: 0 }
        }));
        Ok(LustroPrngBatch { streams })
    }

    /// Derives a stream from the current state along `path`: the same as `fork` for each
    /// id in turn. Panics if `path` is empty.
    pub fn fork_path(&self, path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "fork_path: path must not be empty");
        let (s0, s1, _, _) = self.state.to_parts();
        let (s0, s1) =
            derive_path_lane(s0, s1, Domain::Prng as u128, path.iter().map(|id| id.get()));
        Self {
            state: StreamState::new(s0, s1),
        }
    }

    /// As `fork_path`, returning `Err(DerivePathError::EmptyPath)` for an empty `path`.
    pub fn try_fork_path(&self, path: &[StreamId]) -> Result<Self, DerivePathError> {
        if path.is_empty() {
            return Err(DerivePathError::EmptyPath);
        }
        Ok(self.fork_path(path))
    }

    /// Derives a stream from `seed` along `path`.
    /// Equivalent to `new(seed, path[0])` followed by `fork` for remaining ids.
    /// Panics if `path` is empty.
    pub fn derive_path(seed: &Seed256, path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "derive_path: path must not be empty");
        let (s0, s1) = seed.to_state();

        let (s0, s1) =
            derive_path_lane(s0, s1, Domain::Prng as u128, path.iter().map(|id| id.get()));

        Self {
            state: StreamState::new(s0, s1),
        }
    }

    /// As `derive_path`, returning `Err(DerivePathError::EmptyPath)` for an empty `path`.
    pub fn try_derive_path(seed: &Seed256, path: &[StreamId]) -> Result<Self, DerivePathError> {
        if path.is_empty() {
            return Err(DerivePathError::EmptyPath);
        }
        Ok(Self::derive_path(seed, path))
    }

    /// Exports the current stream state.
    /// The snapshot holds the full stream state: whoever has it can reproduce the rest of the
    /// stream and its forks. Keep it as private as the seed.
    #[must_use]
    pub fn export_snapshot(&self) -> LustroPrngSnapshot {
        let (s0, s1, step, cursor) = self.state.to_parts();
        LustroPrngSnapshot::new(s0, s1, step, cursor)
    }

    /// Restores a stream from a snapshot.
    pub fn import_snapshot(snapshot: LustroPrngSnapshot) -> Self {
        let (s0, s1, step, cursor) = snapshot.into_parts();
        Self {
            state: StreamState::from_parts(s0, s1, step, cursor),
        }
    }
}

/// Reads stream bytes like `fill_bytes`: `read` fills the whole buffer and returns its
/// length, an empty buffer gives `Ok(0)` and does not move the stream.
///
/// The stream has no end, so `read_to_end` and `read_to_string` return `InvalidInput` and
/// read nothing. Bound the read, for example with `take`:
///
/// ```
/// use lustro::{LustroPrng, Seed256, StreamId};
/// use std::io::Read;
///
/// let mut rng = LustroPrng::new(&Seed256::from_bytes([0u8; 32]), StreamId(0));
/// let mut bytes = Vec::new();
/// rng.by_ref().take(100).read_to_end(&mut bytes).unwrap();
/// assert_eq!(bytes.len(), 100);
/// ```
impl std::io::Read for LustroPrng {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.fill_bytes(buf);
        Ok(buf.len())
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> std::io::Result<usize> {
        Err(crate::api::unbounded_read_error())
    }

    fn read_to_string(&mut self, _buf: &mut String) -> std::io::Result<usize> {
        Err(crate::api::unbounded_read_error())
    }
}

impl From<Seed256> for LustroPrng {
    /// Creates stream 0 from a Seed256.
    /// Use LustroPrng::new() directly when a non-zero StreamId is required.
    fn from(seed: Seed256) -> Self {
        Self::new(&seed, StreamId(0))
    }
}

impl From<[u8; 32]> for LustroPrng {
    /// Creates stream 0 from a raw 32-byte array.
    /// Use LustroPrng::new() directly when a non-zero StreamId is required.
    fn from(seed: [u8; 32]) -> Self {
        Self::new(&Seed256::from_bytes(seed), StreamId(0))
    }
}

// ==========================================
// RUST STREAM BATCH API
// ==========================================

/// Independent PRNG streams advanced together by `fill_blocks`.
#[must_use]
#[derive(Clone)]
pub struct LustroPrngBatch {
    streams: Vec<StreamLane>,
}

// Lane states are not printed; only the lane count is shown.
impl core::fmt::Debug for LustroPrngBatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LustroPrngBatch")
            .field("len", &self.streams.len())
            .field("state", &"[redacted]")
            .finish()
    }
}

impl LustroPrngBatch {
    /// Creates a batch from explicit stream identifiers.
    /// Panics if the lane buffer cannot be allocated.
    pub fn new(seed: &Seed256, stream_ids: &[StreamId]) -> Self {
        match Self::try_new(seed, stream_ids) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("new: lane allocation failed: {e:?}"),
            Err(e) => panic!("new: {e}"),
        }
    }

    /// As `new`, returning `Err(BatchError::Reserve)` if reserving the lane buffer fails.
    pub fn try_new(seed: &Seed256, stream_ids: &[StreamId]) -> Result<Self, BatchError> {
        let (s0, s1) = seed.to_state();
        let (base_s0, base_s1) = prepare_base(s0, s1, Domain::Prng as u128);

        let mut streams = Vec::new();
        streams
            .try_reserve_exact(stream_ids.len())
            .map_err(BatchError::Reserve)?;
        streams.extend(stream_ids.iter().map(|&stream_id| {
            let (s0, s1) = derive_branch_stream(base_s0, base_s1, stream_id.get());
            StreamLane { s0, s1, step: 0 }
        }));

        Ok(Self { streams })
    }

    /// Creates a batch with stream IDs `first_stream_id`, `first_stream_id + 1`, ...
    /// (wrapping modulo 2^128). Panics if the lane buffer cannot be allocated.
    pub fn new_range(seed: &Seed256, first_stream_id: StreamId, count: usize) -> Self {
        match Self::try_new_range(seed, first_stream_id, count) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("new_range: lane allocation failed: {e:?}"),
            Err(e) => panic!("new_range: {e}"),
        }
    }

    /// As `new_range`, returning `Err(BatchError::Reserve)` if reserving the lane buffer fails.
    pub fn try_new_range(
        seed: &Seed256,
        first_stream_id: StreamId,
        count: usize,
    ) -> Result<Self, BatchError> {
        let first_stream_id = first_stream_id.get();
        let (s0, s1) = seed.to_state();
        let (base_s0, base_s1) = prepare_base(s0, s1, Domain::Prng as u128);

        let mut streams = Vec::new();
        streams
            .try_reserve_exact(count)
            .map_err(BatchError::Reserve)?;
        for i in 0..count {
            let stream_id = first_stream_id.wrapping_add(i as u128);
            let (s0, s1) = derive_branch_stream(base_s0, base_s1, stream_id);
            streams.push(StreamLane { s0, s1, step: 0 });
        }

        Ok(Self { streams })
    }

    /// Number of lanes.
    #[inline]
    pub fn len(&self) -> usize {
        self.streams.len()
    }

    /// Returns `true` if the batch has no lanes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }

    /// Suggested `steps` for `fill_blocks` at the current `len()`.
    #[inline]
    pub fn suggested_steps(&self) -> usize {
        suggested_steps(self.streams.len())
    }

    /// Fills `out` with `steps` blocks per stream.
    /// Output is step-major: `out[step * len() + lane]`. Panics if `out.len() != len() * steps`.
    pub fn fill_blocks(&mut self, out: &mut [[u8; 32]], steps: usize) {
        let expected = self
            .streams
            .len()
            .checked_mul(steps)
            .expect("fill_blocks: n_streams * steps overflows usize");
        assert_eq!(
            out.len(),
            expected,
            "fill_blocks: out length must equal len() * steps"
        );
        dispatch_streams(&mut self.streams, out, steps);
    }

    /// Advances every lane by `steps` and returns the blocks in the layout of `fill_blocks`:
    /// `out[step * len() + lane]`. Panics if `len() * steps` overflows `usize` or the
    /// buffer cannot be allocated.
    pub fn blocks(&mut self, steps: usize) -> Vec<[u8; 32]> {
        match self.try_blocks(steps) {
            Ok(out) => out,
            Err(BatchError::SizeOverflow) => panic!("blocks: len() * steps overflows usize"),
            Err(BatchError::Reserve(e)) => panic!("blocks: output allocation failed: {e:?}"),
            Err(e) => panic!("blocks: {e}"),
        }
    }

    /// As `blocks`, returning `Err(BatchError::SizeOverflow)` if `len() * steps` overflows
    /// `usize`, or `Err(BatchError::Reserve)` if reserving the output fails (also when it
    /// would exceed `isize::MAX` bytes). The state is unchanged on error.
    pub fn try_blocks(&mut self, steps: usize) -> Result<Vec<[u8; 32]>, BatchError> {
        let count = self
            .streams
            .len()
            .checked_mul(steps)
            .ok_or(BatchError::SizeOverflow)?;
        let mut out = Vec::new();
        out.try_reserve_exact(count).map_err(BatchError::Reserve)?;
        out.resize(count, [0u8; 32]);
        self.fill_blocks(&mut out, steps);
        Ok(out)
    }

    /// Batch with one lane per stream, in order. Lane `i` continues `streams[i]` at its
    /// next block, so `lane(i)` returns a copy of `streams[i]`. Panics if a stream is inside
    /// a block or the lane buffer cannot be allocated.
    pub fn from_streams(streams: &[LustroPrng]) -> Self {
        match Self::try_from_streams(streams) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("from_streams: lane allocation failed: {e:?}"),
            Err(e) => panic!("from_streams: {e}"),
        }
    }

    /// As `from_streams`, returning `Err(BatchError::PartialBlock)` for the first stream that
    /// is inside a block (part of its current block unread) and `Err(BatchError::Reserve)` if
    /// reserving the lane buffer fails. The streams are not changed.
    pub fn try_from_streams(streams: &[LustroPrng]) -> Result<Self, BatchError> {
        Self::try_from_stream_refs(streams.iter())
    }

    // `try_from_streams` over borrowed streams (Python objects, C pointers).
    pub(crate) fn try_from_stream_refs<'a>(
        streams: impl ExactSizeIterator<Item = &'a LustroPrng>,
    ) -> Result<Self, BatchError> {
        let mut lanes = Vec::new();
        lanes
            .try_reserve_exact(streams.len())
            .map_err(BatchError::Reserve)?;
        for (index, s) in streams.enumerate() {
            let (s0, s1, step) = s
                .state
                .to_block_aligned()
                .ok_or(BatchError::PartialBlock { index })?;
            lanes.push(StreamLane { s0, s1, step });
        }
        Ok(Self { streams: lanes })
    }

    /// Copy of lane `i` as a single stream, positioned at the lane's next block.
    /// Later calls on the batch do not affect it. `None` if `i >= len()`.
    #[must_use]
    pub fn lane(&self, i: usize) -> Option<LustroPrng> {
        let lane = self.streams.get(i)?;
        Some(LustroPrng {
            state: StreamState::from_block_aligned(lane.s0, lane.s1, lane.step),
        })
    }

    /// Derives one child stream per lane using the corresponding identifier.
    /// `ids.len()` must equal `len()`. Panics on a length mismatch or if the lane
    /// buffer cannot be allocated.
    pub fn fork(&self, ids: &[StreamId]) -> Self {
        match self.try_fork(ids) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("fork: lane allocation failed: {e:?}"),
            Err(e) => panic!("fork: {e}"),
        }
    }

    /// As `fork`, returning `Err(BatchError::LengthMismatch)` if `ids.len() != len()` and
    /// `Err(BatchError::Reserve)` if reserving the lane buffer fails.
    pub fn try_fork(&self, ids: &[StreamId]) -> Result<Self, BatchError> {
        if ids.len() != self.streams.len() {
            return Err(BatchError::LengthMismatch {
                expected: self.streams.len(),
                actual: ids.len(),
            });
        }

        let mut streams = Vec::new();
        streams
            .try_reserve_exact(self.streams.len())
            .map_err(BatchError::Reserve)?;
        streams.extend(self.streams.iter().zip(ids.iter()).map(|(lane, &id)| {
            let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Prng as u128, id.get());
            // Child stream starts at step 0.
            StreamLane { s0, s1, step: 0 }
        }));

        Ok(Self { streams })
    }

    /// Derives `ids.len()` children per lane. Output is parent-major:
    /// child `j` of lane `i` is at index `i * ids.len() + j`.
    /// Empty `ids` produces an empty batch.
    /// Panics if `len() * ids.len()` overflows usize or the lane buffer
    /// cannot be allocated.
    pub fn fork_many(&self, ids: &[StreamId]) -> Self {
        match self.try_fork_many(ids) {
            Ok(batch) => batch,
            Err(BatchError::SizeOverflow) => {
                panic!("fork_many: len() * ids.len() overflows usize")
            }
            Err(BatchError::Reserve(e)) => panic!("fork_many: lane allocation failed: {e:?}"),
            Err(e) => panic!("fork_many: {e}"),
        }
    }

    /// As `fork_many`, returning `Err(BatchError::SizeOverflow)` if
    /// `len() * ids.len()` overflows `usize`, or `Err(BatchError::Reserve)` if
    /// reserving the lane buffer fails.
    pub fn try_fork_many(&self, ids: &[StreamId]) -> Result<Self, BatchError> {
        let child_count = fork_many_lane_count(self.streams.len(), ids.len())?;

        let mut streams = Vec::new();
        streams
            .try_reserve_exact(child_count)
            .map_err(BatchError::Reserve)?;
        for lane in &self.streams {
            // The midstate depends on the parent lane only; its children share it.
            let (base_s0, base_s1) = prepare_base(lane.s0, lane.s1, Domain::Prng as u128);
            for &id in ids {
                let (s0, s1) = derive_branch_stream(base_s0, base_s1, id.get());
                // Child stream starts at step 0.
                streams.push(StreamLane { s0, s1, step: 0 });
            }
        }

        Ok(Self { streams })
    }

    /// Derives one lane per root by walking each root along `path`.
    /// Equivalent to `new(seed, roots[i])` followed by `fork` for each id.
    /// Panics if `path` is empty or the lane buffer cannot be allocated.
    /// Empty `roots` produces an empty batch.
    pub fn derive_path(seed: &Seed256, roots: &[StreamId], path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "derive_path: path must not be empty");
        Self::derive_path_lanes(seed, roots, path).expect("derive_path: lane allocation failed")
    }

    /// As `derive_path`, returning `Err(DerivePathError::EmptyPath)` if `path`
    /// is empty and `Err(DerivePathError::Reserve)` if reserving the lane buffer fails.
    pub fn try_derive_path(
        seed: &Seed256,
        roots: &[StreamId],
        path: &[StreamId],
    ) -> Result<Self, DerivePathError> {
        if path.is_empty() {
            return Err(DerivePathError::EmptyPath);
        }
        Self::derive_path_lanes(seed, roots, path).map_err(DerivePathError::Reserve)
    }

    // `path` must not be empty.
    fn derive_path_lanes(
        seed: &Seed256,
        roots: &[StreamId],
        path: &[StreamId],
    ) -> Result<Self, TryReserveError> {
        let (s0, s1) = seed.to_state();
        let (base_s0, base_s1) = prepare_base(s0, s1, Domain::Prng as u128);

        let mut streams = Vec::new();
        streams.try_reserve_exact(roots.len())?;
        for &root in roots {
            let (root_s0, root_s1) = derive_branch_stream(base_s0, base_s1, root.get());
            let (s0, s1) = derive_path_lane(
                root_s0,
                root_s1,
                Domain::Prng as u128,
                path.iter().map(|id| id.get()),
            );
            streams.push(StreamLane { s0, s1, step: 0 });
        }

        Ok(Self { streams })
    }

    /// Derives one child per lane with IDs `first`, `first + 1`, ... (wrapping modulo 2^128).
    /// Panics if the lane buffer cannot be allocated.
    pub fn fork_range(&self, first: StreamId) -> Self {
        match self.try_fork_range(first) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("fork_range: lane allocation failed: {e:?}"),
            Err(e) => panic!("fork_range: {e}"),
        }
    }

    /// As `fork_range`, returning `Err(BatchError::Reserve)` if reserving the lane buffer fails.
    pub fn try_fork_range(&self, first: StreamId) -> Result<Self, BatchError> {
        let first = first.get();

        let mut streams = Vec::new();
        streams
            .try_reserve_exact(self.streams.len())
            .map_err(BatchError::Reserve)?;
        streams.extend(self.streams.iter().enumerate().map(|(i, lane)| {
            let child_id = first.wrapping_add(i as u128);
            let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Prng as u128, child_id);
            // Child stream starts at step 0.
            StreamLane { s0, s1, step: 0 }
        }));

        Ok(Self { streams })
    }

    /// As `clone`, returning `Err(BatchError::Reserve)` if reserving the lane buffer
    /// fails. Never returns another variant.
    pub fn try_clone(&self) -> Result<Self, BatchError> {
        let mut streams = Vec::new();
        streams
            .try_reserve_exact(self.streams.len())
            .map_err(BatchError::Reserve)?;
        streams.extend_from_slice(&self.streams);
        Ok(Self { streams })
    }

    /// Exports the current state of every lane.
    /// Batch snapshots are block-aligned and have no cursor.
    /// The snapshot holds every lane's state: whoever has it can reproduce the rest of each
    /// stream and its forks. Keep it as private as the seed.
    /// Panics if the lane buffer cannot be allocated.
    #[must_use]
    pub fn export_snapshot(&self) -> LustroPrngBatchSnapshot {
        match self.try_export_snapshot() {
            Ok(snapshot) => snapshot,
            Err(BatchError::Reserve(e)) => panic!("export_snapshot: lane allocation failed: {e:?}"),
            Err(e) => panic!("export_snapshot: {e}"),
        }
    }

    /// As `export_snapshot`, returning `Err(BatchError::Reserve)` if reserving the lane buffer
    /// fails. Never returns another variant.
    pub fn try_export_snapshot(&self) -> Result<LustroPrngBatchSnapshot, BatchError> {
        let mut lanes = Vec::new();
        lanes
            .try_reserve_exact(self.streams.len())
            .map_err(BatchError::Reserve)?;
        lanes.extend_from_slice(&self.streams);
        Ok(LustroPrngBatchSnapshot::new(lanes))
    }

    // Writes what `export_snapshot().to_le_bytes()` gives into `out`, with no allocation.
    // `out.len()` must be `16 + len() * 48`.
    #[cfg(any(feature = "ffi", feature = "python", test))]
    pub(crate) fn write_snapshot_into(&self, out: &mut [u8]) {
        crate::types::encode_batch_into(crate::types::SnapshotKind::PrngBatch, &self.streams, out);
    }

    /// Restores a batch from a snapshot.
    pub fn import_snapshot(snapshot: LustroPrngBatchSnapshot) -> Self {
        Self {
            streams: snapshot.into_lanes(),
        }
    }
}
