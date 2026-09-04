//! Lustro V1 — domain types for hashes, seeds, streams, and snapshots.

//=================================
// HASH OUTPUT TYPES
//=================================

#[must_use]
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hash128(pub [u8; 16]);

#[must_use]
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hash256(pub [u8; 32]);

// Identifies a PRNG stream.
// Different IDs derive independent streams from the same seed.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct StreamId(pub u128);

//=================================
// PRNG STREAM ID
//=================================

impl StreamId {
    #[inline]
    pub fn get(self) -> u128 {
        self.0
    }
}

//=================================
// SEED & KEY MATERIAL
//=================================

// 256-bit seed or key material.
// Debug output is redacted; `Copy` is intentional.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Seed256(pub [u8; 32]);

impl core::fmt::Debug for Seed256 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Seed256").field(&"[redacted]").finish()
    }
}

//==========================
// PRNG/XOF SNAPSHOT TYPES
//==========================

const SNAPSHOT_VERSION: u8 = 1;

// Identifies the generator family encoded in a snapshot.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotKind {
    Prng      = 0x01,
    Xof       = 0x02,
    PrngBatch = 0x03,
    XofBatch  = 0x04,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotError {
    UnsupportedVersion,
    InvalidKind,
    InvalidCursor,
    InvalidLength,
}

// Single-stream snapshot: fixed 56-byte format.
const SINGLE_SNAPSHOT_LEN: usize = 56;

#[inline]
fn encode_single_snapshot(
    kind: SnapshotKind,
    s0: u128,
    s1: u128,
    step: u64,
    cursor: u8,
) -> [u8; SINGLE_SNAPSHOT_LEN] {
    let mut bytes = [0u8; SINGLE_SNAPSHOT_LEN];
    bytes[0] = SNAPSHOT_VERSION;
    bytes[1] = kind as u8;
    // bytes[2..8] reserved, zero.
    bytes[8..24].copy_from_slice(&s0.to_le_bytes());
    bytes[24..40].copy_from_slice(&s1.to_le_bytes());
    bytes[40..48].copy_from_slice(&step.to_le_bytes());
    bytes[48] = cursor;
    // bytes[49..56] reserved for future extensions.
    bytes
}

#[inline]
fn decode_single_snapshot(
    bytes: &[u8; SINGLE_SNAPSHOT_LEN],
    expected_kind: SnapshotKind,
) -> Result<(u128, u128, u64, u8), SnapshotError> {
    if bytes[0] != SNAPSHOT_VERSION {
        return Err(SnapshotError::UnsupportedVersion);
    }
    if bytes[1] != expected_kind as u8 {
        return Err(SnapshotError::InvalidKind);
    }
    let cursor = bytes[48];
    if cursor > 32 {
        return Err(SnapshotError::InvalidCursor);
    }
    let s0 = u128::from_le_bytes(bytes[8..24].try_into().unwrap());
    let s1 = u128::from_le_bytes(bytes[24..40].try_into().unwrap());
    let step = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
    Ok((s0, s1, step, cursor))
}

// Serialized PRNG stream state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LustroPrngSnapshot {
    s0: u128,
    s1: u128,
    step: u64,
    cursor: u8,
}

impl LustroPrngSnapshot {
    #[inline]
    pub(crate) fn new(s0: u128, s1: u128, step: u64, cursor: u8) -> Self {
        Self { s0, s1, step, cursor }
    }

    #[inline]
    pub(crate) fn into_parts(self) -> (u128, u128, u64, u8) {
        (self.s0, self.s1, self.step, self.cursor)
    }

    // Serializes the snapshot to the stable 56-byte format.
    #[must_use]
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; SINGLE_SNAPSHOT_LEN] {
        encode_single_snapshot(SnapshotKind::Prng, self.s0, self.s1, self.step, self.cursor)
    }

    // Deserializes a PRNG snapshot from the 56-byte format.
    #[must_use]
    #[inline]
    pub fn from_le_bytes(bytes: &[u8; SINGLE_SNAPSHOT_LEN]) -> Result<Self, SnapshotError> {
        let (s0, s1, step, cursor) = decode_single_snapshot(bytes, SnapshotKind::Prng)?;
        Ok(Self { s0, s1, step, cursor })
    }
}

// Serialized XOF stream state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LustroXofSnapshot {
    s0: u128,
    s1: u128,
    step: u64,
    cursor: u8,
}

impl LustroXofSnapshot {
    #[inline]
    pub(crate) fn new(s0: u128, s1: u128, step: u64, cursor: u8) -> Self {
        Self { s0, s1, step, cursor }
    }

    #[inline]
    pub(crate) fn into_parts(self) -> (u128, u128, u64, u8) {
        (self.s0, self.s1, self.step, self.cursor)
    }

    // Serializes the snapshot to the stable 56-byte format.
    #[must_use]
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; SINGLE_SNAPSHOT_LEN] {
        encode_single_snapshot(SnapshotKind::Xof, self.s0, self.s1, self.step, self.cursor)
    }

    // Deserializes a XOF snapshot from the 56-byte format.
    #[must_use]
    #[inline]
    pub fn from_le_bytes(bytes: &[u8; SINGLE_SNAPSHOT_LEN]) -> Result<Self, SnapshotError> {
        let (s0, s1, step, cursor) = decode_single_snapshot(bytes, SnapshotKind::Xof)?;
        Ok(Self { s0, s1, step, cursor })
    }
}

// Batch snapshot: 16-byte header + 48 bytes per lane.
// No cursor; batch lanes are always block-aligned.
const BATCH_HEADER_LEN: usize = 16;
const BATCH_LANE_LEN: usize = 48;

// Returns the encoded batch snapshot length, or `None` on overflow.
#[cfg(feature = "ffi")]
#[inline]
pub(crate) fn batch_snapshot_encoded_len(lane_count: usize) -> Option<usize> {
    lane_count.checked_mul(BATCH_LANE_LEN)?.checked_add(BATCH_HEADER_LEN)
}

#[inline]
fn encode_batch_header(kind: SnapshotKind, lane_count: u64) -> [u8; BATCH_HEADER_LEN] {
    let mut bytes = [0u8; BATCH_HEADER_LEN];
    bytes[0] = SNAPSHOT_VERSION;
    bytes[1] = kind as u8;
    // bytes[2..8] reserved, zero.
    bytes[8..16].copy_from_slice(&lane_count.to_le_bytes());
    bytes
}

#[inline]
fn encode_batch_lane(bytes: &mut [u8], s0: u128, s1: u128, step: u64) {
    debug_assert_eq!(bytes.len(), BATCH_LANE_LEN);
    bytes[0..16].copy_from_slice(&s0.to_le_bytes());
    bytes[16..32].copy_from_slice(&s1.to_le_bytes());
    bytes[32..40].copy_from_slice(&step.to_le_bytes());
    // bytes[40..48] reserved for future extensions.
}

#[inline]
fn decode_batch_lane(bytes: &[u8]) -> (u128, u128, u64) {
    debug_assert_eq!(bytes.len(), BATCH_LANE_LEN);
    let s0 = u128::from_le_bytes(bytes[0..16].try_into().unwrap());
    let s1 = u128::from_le_bytes(bytes[16..32].try_into().unwrap());
    let step = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
    (s0, s1, step)
}

// Validates the header and exact encoded length.
#[inline]
fn decode_batch_header(bytes: &[u8], expected_kind: SnapshotKind) -> Result<u64, SnapshotError> {
    if bytes.len() < BATCH_HEADER_LEN {
        return Err(SnapshotError::InvalidLength);
    }
    if bytes[0] != SNAPSHOT_VERSION {
        return Err(SnapshotError::UnsupportedVersion);
    }
    if bytes[1] != expected_kind as u8 {
        return Err(SnapshotError::InvalidKind);
    }
    let lane_count = u64::from_le_bytes(bytes[8..16].try_into().unwrap());

    let lane_count_usize: usize = lane_count.try_into().map_err(|_| SnapshotError::InvalidLength)?;
    let expected_len = lane_count_usize
        .checked_mul(BATCH_LANE_LEN)
        .and_then(|body_len| body_len.checked_add(BATCH_HEADER_LEN))
        .ok_or(SnapshotError::InvalidLength)?;
    if bytes.len() != expected_len {
        return Err(SnapshotError::InvalidLength);
    }

    Ok(lane_count)
}

// Serialized PRNG batch state.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LustroPrngBatchSnapshot {
    lanes: Vec<(u128, u128, u64)>,
}

impl LustroPrngBatchSnapshot {
    #[inline]
    pub(crate) fn new(lanes: Vec<(u128, u128, u64)>) -> Self {
        Self { lanes }
    }

    #[inline]
    pub(crate) fn into_lanes(self) -> Vec<(u128, u128, u64)> {
        self.lanes
    }

    // Serializes the batch snapshot to its variable-length format.
    #[must_use]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let lane_count = self.lanes.len() as u64;
        let mut out = Vec::with_capacity(BATCH_HEADER_LEN + self.lanes.len() * BATCH_LANE_LEN);
        out.extend_from_slice(&encode_batch_header(SnapshotKind::PrngBatch, lane_count));
        for &(s0, s1, step) in &self.lanes {
            let mut lane_bytes = [0u8; BATCH_LANE_LEN];
            encode_batch_lane(&mut lane_bytes, s0, s1, step);
            out.extend_from_slice(&lane_bytes);
        }
        out
    }

    // Deserializes a PRNG batch snapshot.
    // Rejects trailing or missing bytes.
    #[must_use]
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let lane_count = decode_batch_header(bytes, SnapshotKind::PrngBatch)?;
        let mut lanes = Vec::with_capacity(lane_count as usize);
        for i in 0..lane_count as usize {
            let start = BATCH_HEADER_LEN + i * BATCH_LANE_LEN;
            lanes.push(decode_batch_lane(&bytes[start..start + BATCH_LANE_LEN]));
        }
        Ok(Self { lanes })
    }
}

// Serialized XOF batch state.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LustroXofBatchSnapshot {
    lanes: Vec<(u128, u128, u64)>,
}

impl LustroXofBatchSnapshot {
    #[inline]
    pub(crate) fn new(lanes: Vec<(u128, u128, u64)>) -> Self {
        Self { lanes }
    }

    #[inline]
    pub(crate) fn into_lanes(self) -> Vec<(u128, u128, u64)> {
        self.lanes
    }

    // Serializes the batch snapshot to its variable-length format.
    #[must_use]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let lane_count = self.lanes.len() as u64;
        let mut out = Vec::with_capacity(BATCH_HEADER_LEN + self.lanes.len() * BATCH_LANE_LEN);
        out.extend_from_slice(&encode_batch_header(SnapshotKind::XofBatch, lane_count));
        for &(s0, s1, step) in &self.lanes {
            let mut lane_bytes = [0u8; BATCH_LANE_LEN];
            encode_batch_lane(&mut lane_bytes, s0, s1, step);
            out.extend_from_slice(&lane_bytes);
        }
        out
    }

    // Deserializes a XOF batch snapshot.
    // Rejects trailing or missing bytes.
    #[must_use]
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let lane_count = decode_batch_header(bytes, SnapshotKind::XofBatch)?;
        let mut lanes = Vec::with_capacity(lane_count as usize);
        for i in 0..lane_count as usize {
            let start = BATCH_HEADER_LEN + i * BATCH_LANE_LEN;
            lanes.push(decode_batch_lane(&bytes[start..start + BATCH_LANE_LEN]));
        }
        Ok(Self { lanes })
    }
}

//=================================
// TYPE METHODS & CONVERSIONS
//=================================

impl Hash128 {
    #[inline] pub fn as_bytes(&self) -> &[u8; 16] { &self.0 }

    // Constructs Hash128 from native state.
    #[inline]
    pub(crate) fn from_state(s0: u128) -> Self {
        Self(s0.to_le_bytes())
    }
}

impl AsRef<[u8; 16]> for Hash128 {
    #[inline]
    fn as_ref(&self) -> &[u8; 16] { &self.0 }
}

impl AsRef<[u8]> for Hash128 {
    #[inline]
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl TryFrom<&[u8]> for Hash128 {
    type Error = core::array::TryFromSliceError;

    // Constructs Hash128 from a byte slice of exactly 16 bytes.
    #[inline]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}

impl Hash256 {
    #[inline] pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }

    // Constructs Hash256 from native state.
    #[inline]
    pub(crate) fn from_state(s0: u128, s1: u128) -> Self {
        let mut out = [0u8; 32];
        out[..16].copy_from_slice(&s0.to_le_bytes());
        out[16..].copy_from_slice(&s1.to_le_bytes());
        Self(out)
    }
}

impl AsRef<[u8; 32]> for Hash256 {
    #[inline]
    fn as_ref(&self) -> &[u8; 32] { &self.0 }
}

impl AsRef<[u8]> for Hash256 {
    #[inline]
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl TryFrom<&[u8]> for Hash256 {
    type Error = core::array::TryFromSliceError;

    // Constructs Hash256 from a byte slice of exactly 32 bytes.
    #[inline]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}

impl Seed256 {
    // Constructs Seed256 from a raw 32-byte array.
    #[inline]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}

impl AsRef<[u8; 32]> for Seed256 {
    #[inline]
    fn as_ref(&self) -> &[u8; 32] { &self.0 }
}

impl AsRef<[u8]> for Seed256 {
    #[inline]
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl TryFrom<&[u8]> for Seed256 {
    type Error = core::array::TryFromSliceError;

    // Constructs Seed256 from a byte slice of exactly 32 bytes.
    #[inline]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}