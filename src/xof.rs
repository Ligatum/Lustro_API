//! Lustro V1 — pure Rust XOF API.
//! Output stream is derived from an absorbed message.

use crate::api::{absorb_with_domain, fork_lane, StreamState};
use crate::constants::Domain;
use crate::dispatch::{StreamLane, dispatch_streams, dispatch_streams_many};
use crate::types::{StreamId, LustroXofSnapshot, LustroXofBatchSnapshot};

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
        Self { state: StreamState::new(s0, s1) }
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
        Self { state: self.state.fork(Domain::Xof as u128, id.get()) }
    }

    // Exports the current stream state.
    #[must_use]
    pub fn export_snapshot(&self) -> LustroXofSnapshot {
        let (s0, s1, step, cursor) = self.state.to_parts();
        LustroXofSnapshot::new(s0, s1, step, cursor)
    }

    // Restores a stream from a snapshot.
    #[must_use]
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

    // Fills `out` with one block per stream in lockstep.
    // Stream order is preserved.
    pub fn fill_blocks(&mut self, out: &mut [[u8; 32]]) {
        assert_eq!(
            out.len(), self.streams.len(),
            "fill_blocks: output buffer length must match batch stream count"
        );
        dispatch_streams(&mut self.streams, out);
    }

    // Fills `out` with `steps` blocks per stream.
    // Output is stream-major and preserves stream order.
    pub fn fill_blocks_many(&mut self, out: &mut [[u8; 32]], steps: usize) {
        let expected = self.streams.len()
            .checked_mul(steps)
            .expect("fill_blocks_many: n_streams * steps overflows usize");
        assert_eq!(
            out.len(), expected,
            "fill_blocks_many: out length must equal len() * steps"
        );
        dispatch_streams_many(&mut self.streams, out, steps);
    }

    // Derives one child stream per lane using the corresponding identifier.
    // `ids.len()` must equal `len()`.
    pub fn fork(&self, ids: &[StreamId]) -> Self {
        assert_eq!(
            ids.len(), self.streams.len(),
            "fork: ids length must match batch stream count"
        );

        let streams = self.streams
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

    // Derives sequential child identifiers starting at `first`.
    pub fn fork_range(&self, first: StreamId) -> Self {
        let first = first.get();

        let streams = self.streams
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
        let lanes = self.streams.iter().map(|lane| (lane.s0, lane.s1, lane.step)).collect();
        LustroXofBatchSnapshot::new(lanes)
    }

    // Restores a batch from a snapshot.
    #[must_use]
    pub fn import_snapshot(snapshot: LustroXofBatchSnapshot) -> Self {
        let streams = snapshot.into_lanes()
            .into_iter()
            .map(|(s0, s1, step)| StreamLane { s0, s1, step })
            .collect();
        Self { streams }
    }
}