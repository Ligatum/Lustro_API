//! Lustro V1 — pure Rust XOF API.
//! Output stream is derived from an absorbed message.

use crate::api::{absorb_with_domain, derive_path_lane, fork_lane, StreamState};
use crate::constants::Domain;
use crate::dispatch::{dispatch_streams, StreamLane};
use crate::types::{LustroXofBatchSnapshot, LustroXofSnapshot, StreamId};
use std::collections::TryReserveError;

// ==========================================
// RUST XOF API
// ==========================================

// Cloning preserves the exact stream state and future sequence.
#[must_use]
#[derive(Clone, Debug)]
pub struct LustroXof {
    state: StreamState,
}

impl LustroXof {
    // Absorbs `message` and initializes the output stream.
    pub fn new(message: &[u8]) -> Self {
        let (s0, s1) = absorb_with_domain(message, Domain::Xof as u128);
        Self {
            state: StreamState::new(s0, s1),
        }
    }

    #[must_use]
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.state.read_bytes::<8>())
    }

    #[must_use]
    #[inline]
    pub fn next_u128(&mut self) -> u128 {
        u128::from_le_bytes(self.state.read_bytes::<16>())
    }

    // Returns the next 32-byte block and advances the stream.
    #[must_use]
    #[inline]
    pub fn next_block(&mut self) -> [u8; 32] {
        self.state.read_full_block()
    }

    // Fills `out` with output bytes while preserving stream continuity.
    pub fn fill_bytes(&mut self, out: &mut [u8]) {
        self.state.fill_bytes(out);
    }

    // Derives a child stream from the current state and identifier.
    pub fn fork(&self, id: StreamId) -> Self {
        Self {
            state: self.state.fork(Domain::Xof as u128, id.get()),
        }
    }

    // Derives a stream from `message` along `path`.
    // Equivalent to `new(message)` followed by `fork` for each id in `path`.
    // Panics if `path` is empty.
    pub fn derive_path(message: &[u8], path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "derive_path: path must not be empty");
        let (s0, s1) = absorb_with_domain(message, Domain::Xof as u128);

        let (s0, s1) =
            derive_path_lane(s0, s1, Domain::Xof as u128, path.iter().map(|id| id.get()));

        Self {
            state: StreamState::new(s0, s1),
        }
    }

    // Exports the current stream state.
    #[must_use]
    pub fn export_snapshot(&self) -> LustroXofSnapshot {
        let (s0, s1, step, cursor) = self.state.to_parts();
        LustroXofSnapshot::new(s0, s1, step, cursor)
    }

    // Restores a stream from a snapshot.
    pub fn import_snapshot(snapshot: LustroXofSnapshot) -> Self {
        let (s0, s1, step, cursor) = snapshot.into_parts();
        Self {
            state: StreamState::from_parts(s0, s1, step, cursor),
        }
    }
}

// ==========================================
// RUST XOF BATCH API
// ==========================================

#[must_use]
#[derive(Clone)]
pub struct LustroXofBatch {
    streams: Vec<StreamLane>,
}

impl LustroXofBatch {
    // Creates a batch by independently absorbing each message.
    pub fn new(messages: &[&[u8]]) -> Self {
        let streams = messages
            .iter()
            .map(|&message| {
                let (s0, s1) = absorb_with_domain(message, Domain::Xof as u128);
                StreamLane { s0, s1, step: 0 }
            })
            .collect();

        Self { streams }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.streams.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }

    // Fills `out` with `steps` blocks per stream.
    // Output is step-major: `out[step * len() + lane]`.
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

    // Derives one child stream per lane using the corresponding identifier.
    // `ids.len()` must equal `len()`.
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
                let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Xof as u128, id.get());
                // Child stream starts at step 0.
                StreamLane { s0, s1, step: 0 }
            })
            .collect();

        Self { streams }
    }

    // Derives `ids.len()` children per lane. Output is parent-major:
    // child `j` of lane `i` is at index `i * ids.len() + j`.
    // Empty `ids` produces an empty batch.
    // Panics if `len() * ids.len()` overflows usize or the lane buffer
    // cannot be allocated.
    pub fn fork_many(&self, ids: &[StreamId]) -> Self {
        self.try_fork_many(ids)
            .expect("fork_many: lane allocation failed")
    }

    // As `fork_many`, reporting allocation failure as `Err`.
    // Still panics if `len() * ids.len()` overflows usize.
    pub(crate) fn try_fork_many(&self, ids: &[StreamId]) -> Result<Self, TryReserveError> {
        let child_count = self
            .streams
            .len()
            .checked_mul(ids.len())
            .expect("fork_many: len() * ids.len() overflows usize");

        let mut streams = Vec::new();
        streams.try_reserve_exact(child_count)?;
        for lane in &self.streams {
            for &id in ids {
                let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Xof as u128, id.get());
                // Child stream starts at step 0.
                streams.push(StreamLane { s0, s1, step: 0 });
            }
        }

        Ok(Self { streams })
    }

    // Derives a canonical batch: one lane per message, each walked along `path`.
    // Lane `i` equals `LustroXof::new(messages[i])` followed by `fork` for
    // each id in `path`. Depends only on `messages` and `path` — independent
    // of any other stream's state.
    // Panics if `path` is empty. Empty `messages` produces an empty batch.
    pub fn derive_path(messages: &[&[u8]], path: &[StreamId]) -> Self {
        assert!(!path.is_empty(), "derive_path: path must not be empty");

        let mut streams = Vec::with_capacity(messages.len());
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

        Self { streams }
    }

    // Derives sequential child identifiers starting at `first`.
    pub fn fork_range(&self, first: StreamId) -> Self {
        let first = first.get();

        let streams = self
            .streams
            .iter()
            .enumerate()
            .map(|(i, lane)| {
                let child_id = first.wrapping_add(i as u128);
                let (s0, s1) = fork_lane(lane.s0, lane.s1, Domain::Xof as u128, child_id);
                // Child stream starts at step 0.
                StreamLane { s0, s1, step: 0 }
            })
            .collect();

        Self { streams }
    }

    // Exports the current state of every lane.
    // Batch snapshots are block-aligned and have no cursor.
    #[must_use]
    pub fn export_snapshot(&self) -> LustroXofBatchSnapshot {
        let lanes = self
            .streams
            .iter()
            .map(|lane| (lane.s0, lane.s1, lane.step))
            .collect();
        LustroXofBatchSnapshot::new(lanes)
    }

    // Restores a batch from a snapshot.
    pub fn import_snapshot(snapshot: LustroXofBatchSnapshot) -> Self {
        let streams = snapshot
            .into_lanes()
            .into_iter()
            .map(|(s0, s1, step)| StreamLane { s0, s1, step })
            .collect();
        Self { streams }
    }
}
