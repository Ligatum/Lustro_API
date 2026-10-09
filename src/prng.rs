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

// RngCore
#[cfg(feature = "rand")]
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

    /// Derives a child stream from the current state and identifier.
    pub fn fork(&self, id: StreamId) -> Self {
        Self {
            state: self.state.fork(Domain::Prng as u128, id.get()),
        }
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

// Lane states are secret-derived; only the lane count is shown.
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
    pub fn new(seed: &Seed256, stream_ids: &[StreamId]) -> Self {
        let (s0, s1) = seed.to_state();
        let (base_s0, base_s1) = prepare_base(s0, s1, Domain::Prng as u128);

        let streams = stream_ids
            .iter()
            .map(|&stream_id| {
                let (s0, s1) = derive_branch_stream(base_s0, base_s1, stream_id.get());
                StreamLane { s0, s1, step: 0 }
            })
            .collect();

        Self { streams }
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

    /// Derives one child stream per lane using the corresponding identifier.
    /// `ids.len()` must equal `len()`.
    pub fn fork(&self, ids: &[StreamId]) -> Self {
        assert_eq!(
            ids.len(),
            self.streams.len(),
            "fork: ids length must match batch stream count"
        );

        let streams = self
            .streams
            .iter()
            .zip(ids.iter())
            .map(|(lane, &id)| {
                let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Prng as u128, id.get());
                // Child stream starts at step 0.
                StreamLane { s0, s1, step: 0 }
            })
            .collect();

        Self { streams }
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
            for &id in ids {
                let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Prng as u128, id.get());
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
    pub fn fork_range(&self, first: StreamId) -> Self {
        let first = first.get();

        let streams = self
            .streams
            .iter()
            .enumerate()
            .map(|(i, lane)| {
                let child_id = first.wrapping_add(i as u128);
                let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Prng as u128, child_id);
                // Child stream starts at step 0.
                StreamLane { s0, s1, step: 0 }
            })
            .collect();

        Self { streams }
    }

    /// Exports the current state of every lane.
    /// Batch snapshots are block-aligned and have no cursor.
    #[must_use]
    pub fn export_snapshot(&self) -> LustroPrngBatchSnapshot {
        let lanes = self
            .streams
            .iter()
            .map(|lane| (lane.s0, lane.s1, lane.step))
            .collect();
        LustroPrngBatchSnapshot::new(lanes)
    }

    /// Restores a batch from a snapshot.
    pub fn import_snapshot(snapshot: LustroPrngBatchSnapshot) -> Self {
        let streams = snapshot
            .into_lanes()
            .into_iter()
            .map(|(s0, s1, step)| StreamLane { s0, s1, step })
            .collect();
        Self { streams }
    }
}
