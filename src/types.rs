//! Lustro V1 — domain types for hashes, seeds, streams, and snapshots.

mod batch;
mod digest;
mod runtime;
mod seed;
mod snapshot;

pub use batch::{BatchError, DerivePathError};
pub use digest::{Hash128, Hash256};
pub use runtime::{ConfigError, RuntimeInfo, RuntimeOptions};
#[cfg(feature = "entropy")]
pub use seed::EntropyError;
pub use seed::Seed256;
pub use snapshot::{
    LustroPrngBatchSnapshot, LustroPrngSnapshot, LustroXofBatchSnapshot, LustroXofSnapshot,
    SnapshotError, SnapshotKind,
};

pub(crate) use batch::fork_many_lane_count;
#[cfg(feature = "ffi")]
pub(crate) use snapshot::batch_snapshot_encoded_len;

//=================================
// PRNG STREAM ID
//=================================

/// Identifies a PRNG stream.
/// Different IDs derive independent streams from the same seed.
/// `Ord` compares the identifiers numerically.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StreamId(pub u128);

impl StreamId {
    /// Returns the raw 128-bit identifier.
    #[inline]
    pub fn get(self) -> u128 {
        self.0
    }
}

impl From<u64> for StreamId {
    #[inline]
    fn from(id: u64) -> Self {
        Self(id as u128)
    }
}

impl From<u128> for StreamId {
    #[inline]
    fn from(id: u128) -> Self {
        Self(id)
    }
}

impl From<StreamId> for u128 {
    #[inline]
    fn from(id: StreamId) -> Self {
        id.0
    }
}

//=================================
// FORMAT LENGTHS
//=================================

/// Seed length in bytes.
pub const LUSTRO_SEED_LEN: usize = 32;
/// Output block length in bytes.
pub const LUSTRO_BLOCK_LEN: usize = 32;
/// `Hash128` length in bytes.
pub const LUSTRO_HASH128_LEN: usize = 16;
/// `Hash256` length in bytes.
pub const LUSTRO_HASH256_LEN: usize = 32;
/// Single-stream snapshot length in bytes.
pub const LUSTRO_SNAPSHOT_LEN: usize = 56;
/// Batch snapshot header length in bytes.
pub const LUSTRO_BATCH_SNAPSHOT_HEADER_LEN: usize = 16;
/// Batch snapshot length per lane in bytes.
pub const LUSTRO_BATCH_SNAPSHOT_LANE_LEN: usize = 48;

const _: () = {
    assert!(core::mem::size_of::<Seed256>() == LUSTRO_SEED_LEN);
    assert!(core::mem::size_of::<Hash128>() == LUSTRO_HASH128_LEN);
    assert!(core::mem::size_of::<Hash256>() == LUSTRO_HASH256_LEN);
};

// Fails to compile if the block type is no longer `[u8; LUSTRO_BLOCK_LEN]`.
const _: fn(&mut crate::prng::LustroPrng) -> [u8; LUSTRO_BLOCK_LEN] =
    crate::prng::LustroPrng::next_block;
const _: fn(&mut crate::xof::LustroXof) -> [u8; LUSTRO_BLOCK_LEN] =
    crate::xof::LustroXof::next_block;
