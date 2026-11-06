//! Lustro V1 — XOF API.
//! Output stream is derived from an absorbed message.

use crate::api::{
    absorb_hash256_var_into, absorb_with_domain, derive_branch_stream, derive_path_lane, fork_lane,
    prepare_base, StreamState,
};
use crate::constants::Domain;
use crate::dispatch::{dispatch_streams, suggested_steps, StreamLane};
use crate::types::{
    fork_many_lane_count, BatchError, DerivePathError, LustroXofBatchSnapshot, LustroXofSnapshot,
    StreamId,
};
use std::collections::TryReserveError;

// ==========================================
// RUST XOF API
// ==========================================

/// XOF stream. Cloning preserves the exact stream state and future sequence.
#[must_use]
#[derive(Clone, Debug)]
pub struct LustroXof {
    state: StreamState,
}

impl LustroXof {
    /// Absorbs `message` and initializes the output stream.
    pub fn new(message: &[u8]) -> Self {
        let (s0, s1) = absorb_with_domain(message, Domain::Xof as u128);
        Self {
            state: StreamState::new(s0, s1),
        }
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
            state: self.state.fork(Domain::Xof as u128, id.get()),
        }
    }

    /// Derives one child per identifier, as a batch: lane `i` is `fork(ids[i])`.
    /// Panics if the lane buffer cannot be allocated.
    pub fn fork_batch(&self, ids: &[StreamId]) -> LustroXofBatch {
        match self.try_fork_batch(ids) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("fork_batch: lane allocation failed: {e:?}"),
            Err(e) => panic!("fork_batch: {e}"),
        }
    }

    /// As `fork_batch`, returning `Err(BatchError::Reserve)` if reserving the lane buffer fails.
    pub fn try_fork_batch(&self, ids: &[StreamId]) -> Result<LustroXofBatch, BatchError> {
        self.try_fork_lanes(ids.len(), ids.iter().map(|id| id.get()))
    }

    /// As `fork_batch` with ids `first`, `first + 1`, ... (`count` lanes, wrapping modulo
    /// 2^128). Panics if the lane buffer cannot be allocated.
    pub fn fork_batch_range(&self, first: StreamId, count: usize) -> LustroXofBatch {
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
    ) -> Result<LustroXofBatch, BatchError> {
        let first = first.get();
        self.try_fork_lanes(count, (0..count).map(|i| first.wrapping_add(i as u128)))
    }

    // Children of the current state, one lane per id, each at step 0.
    fn try_fork_lanes(
        &self,
        count: usize,
        ids: impl Iterator<Item = u128>,
    ) -> Result<LustroXofBatch, BatchError> {
        let (s0, s1, _, _) = self.state.to_parts();
        let mut streams = Vec::new();
        streams
            .try_reserve_exact(count)
            .map_err(BatchError::Reserve)?;
        let (base_s0, base_s1) = prepare_base(s0, s1, Domain::Xof as u128);
        streams.extend(ids.map(|id| {
            let (s0, s1) = derive_branch_stream(base_s0, base_s1, id);
            StreamLane { s0, s1, step: 0 }
        }));
        Ok(LustroXofBatch { streams })
    }

    /// Derives a stream from the current state along `path`: the same as `fork` for each
    /// id in turn. Panics if `path` is empty.
    pub fn fork_path(&self, path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "fork_path: path must not be empty");
        let (s0, s1, _, _) = self.state.to_parts();
        let (s0, s1) =
            derive_path_lane(s0, s1, Domain::Xof as u128, path.iter().map(|id| id.get()));
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

    /// Derives a stream from `message` along `path`.
    /// Equivalent to `new(message)` followed by `fork` for each id in `path`.
    /// Panics if `path` is empty.
    pub fn derive_path(message: &[u8], path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "derive_path: path must not be empty");
        let (s0, s1) = absorb_with_domain(message, Domain::Xof as u128);

        let (s0, s1) =
            derive_path_lane(s0, s1, Domain::Xof as u128, path.iter().map(|id| id.get()));

        Self {
            state: StreamState::new(s0, s1),
        }
    }

    /// As `derive_path`, returning `Err(DerivePathError::EmptyPath)` for an empty `path`.
    pub fn try_derive_path(message: &[u8], path: &[StreamId]) -> Result<Self, DerivePathError> {
        if path.is_empty() {
            return Err(DerivePathError::EmptyPath);
        }
        Ok(Self::derive_path(message, path))
    }

    /// Exports the current stream state.
    /// The snapshot holds the full stream state: whoever has it can reproduce the rest of the
    /// stream and its forks. Keep it as private as the seed.
    #[must_use]
    pub fn export_snapshot(&self) -> LustroXofSnapshot {
        let (s0, s1, step, cursor) = self.state.to_parts();
        LustroXofSnapshot::new(s0, s1, step, cursor)
    }

    /// Restores a stream from a snapshot.
    pub fn import_snapshot(snapshot: LustroXofSnapshot) -> Self {
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
/// use lustro::LustroXof;
/// use std::io::Read;
///
/// let mut rng = LustroXof::new(b"message");
/// let mut bytes = Vec::new();
/// rng.by_ref().take(100).read_to_end(&mut bytes).unwrap();
/// assert_eq!(bytes.len(), 100);
/// ```
impl std::io::Read for LustroXof {
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

// ==========================================
// RUST XOF BATCH API
// ==========================================

/// Independent XOF streams advanced together by `fill_blocks`.
#[must_use]
#[derive(Clone)]
pub struct LustroXofBatch {
    streams: Vec<StreamLane>,
}

// Lane states are not printed; only the lane count is shown.
impl core::fmt::Debug for LustroXofBatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LustroXofBatch")
            .field("len", &self.streams.len())
            .field("state", &"[redacted]")
            .finish()
    }
}

// Messages absorbed per call of `absorb_hash256_var_into`. One call for a whole large batch
// would need a digest buffer as long as the batch; a portion keeps it small and is still
// big enough for the pool.
pub(crate) const ABSORB_PORTION: usize = 16384;

impl LustroXofBatch {
    /// Creates a batch by independently absorbing each message.
    /// Panics if the lane buffer cannot be allocated.
    pub fn new(messages: &[&[u8]]) -> Self {
        match Self::try_new(messages) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("new: lane allocation failed: {e:?}"),
            Err(e) => panic!("new: {e}"),
        }
    }

    /// As `new`, returning `Err(BatchError::Reserve)` if reserving the lane buffer fails.
    pub fn try_new(messages: &[&[u8]]) -> Result<Self, BatchError> {
        let mut streams = Vec::new();
        streams
            .try_reserve_exact(messages.len())
            .map_err(BatchError::Reserve)?;

        // Digests are the two lane words, `s0` then `s1`, little-endian.
        let mut digests: Vec<[u8; 32]> = Vec::new();
        let portion = messages.len().min(ABSORB_PORTION);
        digests
            .try_reserve_exact(portion)
            .map_err(BatchError::Reserve)?;
        digests.resize(portion, [0; 32]);
        for part in messages.chunks(ABSORB_PORTION) {
            let out = &mut digests[..part.len()];
            absorb_hash256_var_into(part, Domain::Xof as u128, out);
            streams.extend(out.iter().map(|d| StreamLane {
                s0: u128::from_le_bytes(d[..16].try_into().unwrap()),
                s1: u128::from_le_bytes(d[16..].try_into().unwrap()),
                step: 0,
            }));
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
    /// Output is step-major: `out[step * len() + lane]`.
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
    pub fn from_streams(streams: &[LustroXof]) -> Self {
        match Self::try_from_streams(streams) {
            Ok(batch) => batch,
            Err(BatchError::Reserve(e)) => panic!("from_streams: lane allocation failed: {e:?}"),
            Err(e) => panic!("from_streams: {e}"),
        }
    }

    /// As `from_streams`, returning `Err(BatchError::PartialBlock)` for the first stream that
    /// is inside a block (part of its current block unread) and `Err(BatchError::Reserve)` if
    /// reserving the lane buffer fails. The streams are not changed.
    pub fn try_from_streams(streams: &[LustroXof]) -> Result<Self, BatchError> {
        Self::try_from_stream_refs(streams.iter())
    }

    // `try_from_streams` over borrowed streams (Python objects, C pointers).
    pub(crate) fn try_from_stream_refs<'a>(
        streams: impl ExactSizeIterator<Item = &'a LustroXof>,
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
    pub fn lane(&self, i: usize) -> Option<LustroXof> {
        let lane = self.streams.get(i)?;
        Some(LustroXof {
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
            let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Xof as u128, id.get());
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
            let (base_s0, base_s1) = prepare_base(lane.s0, lane.s1, Domain::Xof as u128);
            for &id in ids {
                let (s0, s1) = derive_branch_stream(base_s0, base_s1, id.get());
                // Child stream starts at step 0.
                streams.push(StreamLane { s0, s1, step: 0 });
            }
        }

        Ok(Self { streams })
    }

    /// Derives one lane per message by walking each stream along `path`.
    /// Equivalent to `new(messages[i])` followed by `fork` for each id.
    /// Panics if `path` is empty or the lane buffer cannot be allocated.
    /// Empty `messages` produces an empty batch.
    pub fn derive_path(messages: &[&[u8]], path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "derive_path: path must not be empty");
        Self::derive_path_lanes(messages, path).expect("derive_path: lane allocation failed")
    }

    /// As `derive_path`, returning `Err(DerivePathError::EmptyPath)` if `path`
    /// is empty and `Err(DerivePathError::Reserve)` if reserving the lane buffer fails.
    pub fn try_derive_path(messages: &[&[u8]], path: &[StreamId]) -> Result<Self, DerivePathError> {
        if path.is_empty() {
            return Err(DerivePathError::EmptyPath);
        }
        Self::derive_path_lanes(messages, path).map_err(DerivePathError::Reserve)
    }

    // `path` must not be empty.
    fn derive_path_lanes(messages: &[&[u8]], path: &[StreamId]) -> Result<Self, TryReserveError> {
        let mut streams = Vec::new();
        streams.try_reserve_exact(messages.len())?;
        for &message in messages {
            let (root_s0, root_s1) = absorb_with_domain(message, Domain::Xof as u128);
            let (s0, s1) = derive_path_lane(
                root_s0,
                root_s1,
                Domain::Xof as u128,
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
            let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Xof as u128, child_id);
            // Child stream starts at step 0.
            StreamLane { s0, s1, step: 0 }
        }));

        Ok(Self { streams })
    }

    /// As `clone`, returning `Err(BatchError::Reserve)` if reserving the lane buffer
    /// fails. Never returns `BatchError::SizeOverflow`.
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
    #[must_use]
    pub fn export_snapshot(&self) -> LustroXofBatchSnapshot {
        let lanes = self
            .streams
            .iter()
            .map(|lane| (lane.s0, lane.s1, lane.step))
            .collect();
        LustroXofBatchSnapshot::new(lanes)
    }

    /// Restores a batch from a snapshot.
    pub fn import_snapshot(snapshot: LustroXofBatchSnapshot) -> Self {
        let streams = snapshot
            .into_lanes()
            .into_iter()
            .map(|(s0, s1, step)| StreamLane { s0, s1, step })
            .collect();
        Self { streams }
    }
}
