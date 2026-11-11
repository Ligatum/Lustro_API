//! Lustro V1 — PRNG/XOF snapshot types and their binary format.

use crate::dispatch::StreamLane;

use super::{
    BatchError, LUSTRO_BATCH_SNAPSHOT_HEADER_LEN, LUSTRO_BATCH_SNAPSHOT_LANE_LEN,
    LUSTRO_SNAPSHOT_LEN,
};

//==========================
// PRNG/XOF SNAPSHOT TYPES
//==========================

// Byte 0 of every snapshot. Independent of the API and crate versions.
const SNAPSHOT_VERSION: u8 = 1;

/// Identifies the generator family encoded in a snapshot.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum SnapshotKind {
    /// Single PRNG stream.
    Prng = 0x01,
    /// Single XOF stream.
    Xof = 0x02,
    /// PRNG batch.
    PrngBatch = 0x03,
    /// XOF batch.
    XofBatch = 0x04,
}

impl SnapshotKind {
    /// Reads the kind of exported snapshot bytes from their header.
    /// Checks only the version and kind bytes, so `Ok` does not mean the import will succeed.
    /// `InvalidLength` for fewer than 2 bytes, `UnsupportedVersion` or `InvalidKind` for an
    /// unknown byte.
    pub fn detect(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let (version, kind) = match bytes {
            [version, kind, ..] => (*version, *kind),
            _ => return Err(SnapshotError::InvalidLength),
        };
        if version != SNAPSHOT_VERSION {
            return Err(SnapshotError::UnsupportedVersion);
        }
        match kind {
            0x01 => Ok(SnapshotKind::Prng),
            0x02 => Ok(SnapshotKind::Xof),
            0x03 => Ok(SnapshotKind::PrngBatch),
            0x04 => Ok(SnapshotKind::XofBatch),
            _ => Err(SnapshotError::InvalidKind),
        }
    }
}

/// Why a snapshot could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum SnapshotError {
    /// Version byte is not supported.
    UnsupportedVersion,
    /// Kind byte does not match the expected type.
    InvalidKind,
    /// Cursor is outside `1..=32`.
    InvalidCursor,
    /// Length does not match the format.
    InvalidLength,
    /// Reserved bytes are not zero.
    InvalidReserved,
    /// Reserving the decoded lane buffer failed (`try_from_le_bytes` of a batch snapshot).
    Reserve,
}

impl core::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SnapshotError::UnsupportedVersion => write!(f, "unsupported snapshot version"),
            SnapshotError::InvalidKind => write!(f, "snapshot kind does not match this type"),
            SnapshotError::InvalidCursor => write!(f, "invalid snapshot cursor value"),
            SnapshotError::InvalidLength => write!(f, "invalid snapshot length"),
            SnapshotError::InvalidReserved => write!(f, "snapshot reserved bytes are not zero"),
            SnapshotError::Reserve => write!(f, "snapshot buffer reservation failed"),
        }
    }
}

impl std::error::Error for SnapshotError {}

// Single-stream snapshot: fixed 56-byte format.
const SINGLE_SNAPSHOT_LEN: usize = LUSTRO_SNAPSHOT_LEN;

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
    // bytes[49..56] reserved, zero.
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
    // A later format version may use the reserved bytes; version 1 keeps them zero.
    if bytes[2..8].iter().chain(&bytes[49..56]).any(|&b| b != 0) {
        return Err(SnapshotError::InvalidReserved);
    }
    let cursor = bytes[48];
    if cursor == 0 || cursor > 32 {
        return Err(SnapshotError::InvalidCursor);
    }
    let s0 = u128::from_le_bytes(bytes[8..24].try_into().unwrap());
    let s1 = u128::from_le_bytes(bytes[24..40].try_into().unwrap());
    let step = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
    Ok((s0, s1, step, cursor))
}

/// Serialized PRNG stream state.
/// Debug output is redacted; `s0`/`s1` are the stream state, which continues the stream and its forks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LustroPrngSnapshot {
    s0: u128,
    s1: u128,
    step: u64,
    cursor: u8,
}

impl core::fmt::Debug for LustroPrngSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LustroPrngSnapshot")
            .field("state", &"[redacted]")
            .field("step", &self.step)
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl TryFrom<&[u8]> for LustroPrngSnapshot {
    type Error = SnapshotError;

    fn try_from(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let bytes: &[u8; SINGLE_SNAPSHOT_LEN] =
            bytes.try_into().map_err(|_| SnapshotError::InvalidLength)?;
        Self::from_le_bytes(bytes)
    }
}

impl LustroPrngSnapshot {
    #[inline]
    pub(crate) fn new(s0: u128, s1: u128, step: u64, cursor: u8) -> Self {
        Self {
            s0,
            s1,
            step,
            cursor,
        }
    }

    #[inline]
    pub(crate) fn into_parts(self) -> (u128, u128, u64, u8) {
        (self.s0, self.s1, self.step, self.cursor)
    }

    /// Serializes the snapshot to the stable 56-byte format.
    #[must_use]
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; SINGLE_SNAPSHOT_LEN] {
        encode_single_snapshot(SnapshotKind::Prng, self.s0, self.s1, self.step, self.cursor)
    }

    /// Deserializes a PRNG snapshot from the 56-byte format.
    #[inline]
    pub fn from_le_bytes(bytes: &[u8; SINGLE_SNAPSHOT_LEN]) -> Result<Self, SnapshotError> {
        let (s0, s1, step, cursor) = decode_single_snapshot(bytes, SnapshotKind::Prng)?;
        Ok(Self {
            s0,
            s1,
            step,
            cursor,
        })
    }
}

/// Serialized XOF stream state.
/// Debug output is redacted; `s0`/`s1` are the stream state, which continues the stream and its forks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LustroXofSnapshot {
    s0: u128,
    s1: u128,
    step: u64,
    cursor: u8,
}

impl core::fmt::Debug for LustroXofSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LustroXofSnapshot")
            .field("state", &"[redacted]")
            .field("step", &self.step)
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl TryFrom<&[u8]> for LustroXofSnapshot {
    type Error = SnapshotError;

    fn try_from(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let bytes: &[u8; SINGLE_SNAPSHOT_LEN] =
            bytes.try_into().map_err(|_| SnapshotError::InvalidLength)?;
        Self::from_le_bytes(bytes)
    }
}

impl LustroXofSnapshot {
    #[inline]
    pub(crate) fn new(s0: u128, s1: u128, step: u64, cursor: u8) -> Self {
        Self {
            s0,
            s1,
            step,
            cursor,
        }
    }

    #[inline]
    pub(crate) fn into_parts(self) -> (u128, u128, u64, u8) {
        (self.s0, self.s1, self.step, self.cursor)
    }

    /// Serializes the snapshot to the stable 56-byte format.
    #[must_use]
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; SINGLE_SNAPSHOT_LEN] {
        encode_single_snapshot(SnapshotKind::Xof, self.s0, self.s1, self.step, self.cursor)
    }

    /// Deserializes an XOF snapshot from the 56-byte format.
    #[inline]
    pub fn from_le_bytes(bytes: &[u8; SINGLE_SNAPSHOT_LEN]) -> Result<Self, SnapshotError> {
        let (s0, s1, step, cursor) = decode_single_snapshot(bytes, SnapshotKind::Xof)?;
        Ok(Self {
            s0,
            s1,
            step,
            cursor,
        })
    }
}

// Batch snapshot: 16-byte header + 48 bytes per lane.
// No cursor; batch lanes are always block-aligned.
const BATCH_HEADER_LEN: usize = LUSTRO_BATCH_SNAPSHOT_HEADER_LEN;
const BATCH_LANE_LEN: usize = LUSTRO_BATCH_SNAPSHOT_LANE_LEN;

// Returns the encoded batch snapshot length, or `None` on overflow.
#[cfg(any(feature = "ffi", feature = "python", test))]
#[inline]
pub(crate) fn batch_snapshot_encoded_len(lane_count: usize) -> Option<usize> {
    lane_count
        .checked_mul(BATCH_LANE_LEN)?
        .checked_add(BATCH_HEADER_LEN)
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
    bytes[40..48].fill(0); // reserved
}

// Writes the snapshot of `lanes` into `out`, which must have exactly the encoded length. Every
// byte is written, the reserved ones as zero, so `out` may hold anything beforehand. Same bytes
// as `to_le_bytes` of the batch snapshot, without an intermediate buffer.
#[cfg(any(feature = "ffi", feature = "python", test))]
pub(crate) fn encode_batch_into(kind: SnapshotKind, lanes: &[StreamLane], out: &mut [u8]) {
    assert_eq!(out.len(), BATCH_HEADER_LEN + lanes.len() * BATCH_LANE_LEN);
    let (header, body) = out.split_at_mut(BATCH_HEADER_LEN);
    header.copy_from_slice(&encode_batch_header(kind, lanes.len() as u64));
    for (chunk, lane) in body.chunks_exact_mut(BATCH_LANE_LEN).zip(lanes) {
        encode_batch_lane(chunk, lane.s0, lane.s1, lane.step);
    }
}

#[inline]
fn decode_batch_lane(bytes: &[u8]) -> StreamLane {
    debug_assert_eq!(bytes.len(), BATCH_LANE_LEN);
    let s0 = u128::from_le_bytes(bytes[0..16].try_into().unwrap());
    let s1 = u128::from_le_bytes(bytes[16..32].try_into().unwrap());
    let step = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
    StreamLane { s0, s1, step }
}

// Validates the header, the exact encoded length and the reserved bytes, before anything
// is allocated.
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
    if bytes[2..8].iter().any(|&b| b != 0) {
        return Err(SnapshotError::InvalidReserved);
    }
    let lane_count = u64::from_le_bytes(bytes[8..16].try_into().unwrap());

    let lane_count_usize: usize = lane_count
        .try_into()
        .map_err(|_| SnapshotError::InvalidLength)?;
    let expected_len = lane_count_usize
        .checked_mul(BATCH_LANE_LEN)
        .and_then(|body_len| body_len.checked_add(BATCH_HEADER_LEN))
        .ok_or(SnapshotError::InvalidLength)?;
    if bytes.len() != expected_len {
        return Err(SnapshotError::InvalidLength);
    }
    if bytes[BATCH_HEADER_LEN..]
        .chunks_exact(BATCH_LANE_LEN)
        .any(|lane| lane[40..].iter().any(|&b| b != 0))
    {
        return Err(SnapshotError::InvalidReserved);
    }

    Ok(lane_count)
}

/// Serialized PRNG batch state.
/// Debug output is redacted; each lane holds stream state, which continues that stream and its forks.
#[derive(Clone, PartialEq, Eq)]
pub struct LustroPrngBatchSnapshot {
    lanes: Vec<StreamLane>,
}

impl core::fmt::Debug for LustroPrngBatchSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LustroPrngBatchSnapshot")
            .field("lane_count", &self.lanes.len())
            .field("lanes", &"[redacted]")
            .finish()
    }
}

impl TryFrom<&[u8]> for LustroPrngBatchSnapshot {
    type Error = SnapshotError;

    fn try_from(bytes: &[u8]) -> Result<Self, SnapshotError> {
        Self::try_from_le_bytes(bytes)
    }
}

impl LustroPrngBatchSnapshot {
    #[inline]
    pub(crate) fn new(lanes: Vec<StreamLane>) -> Self {
        Self { lanes }
    }

    #[inline]
    pub(crate) fn into_lanes(self) -> Vec<StreamLane> {
        self.lanes
    }

    /// Serializes the batch snapshot to its variable-length format.
    /// Panics if the output buffer cannot be allocated.
    #[must_use]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self.try_to_le_bytes() {
            Ok(out) => out,
            Err(BatchError::Reserve(e)) => panic!("to_le_bytes: allocation failed: {e:?}"),
            Err(e) => panic!("to_le_bytes: {e}"),
        }
    }

    /// As `to_le_bytes`, returning `Err(BatchError::Reserve)` if reserving the output fails.
    /// Never returns another variant.
    pub fn try_to_le_bytes(&self) -> Result<Vec<u8>, BatchError> {
        let lane_count = self.lanes.len() as u64;
        let mut out = Vec::new();
        // No overflow: the lanes already fill at most isize::MAX bytes, at least 40 each.
        out.try_reserve_exact(BATCH_HEADER_LEN + self.lanes.len() * BATCH_LANE_LEN)
            .map_err(BatchError::Reserve)?;
        out.extend_from_slice(&encode_batch_header(SnapshotKind::PrngBatch, lane_count));
        for &StreamLane { s0, s1, step } in &self.lanes {
            let mut lane_bytes = [0u8; BATCH_LANE_LEN];
            encode_batch_lane(&mut lane_bytes, s0, s1, step);
            out.extend_from_slice(&lane_bytes);
        }
        Ok(out)
    }

    /// Deserializes a PRNG batch snapshot.
    /// Rejects trailing or missing bytes. Panics if the lane buffer cannot be allocated.
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        match Self::try_from_le_bytes(bytes) {
            Err(SnapshotError::Reserve) => panic!("from_le_bytes: lane allocation failed"),
            r => r,
        }
    }

    /// As `from_le_bytes`, returning `Err(SnapshotError::Reserve)` if reserving the lane
    /// buffer fails.
    pub fn try_from_le_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let lane_count = decode_batch_header(bytes, SnapshotKind::PrngBatch)?;
        let mut lanes = Vec::new();
        lanes
            .try_reserve_exact(lane_count as usize)
            .map_err(|_| SnapshotError::Reserve)?;
        for i in 0..lane_count as usize {
            let start = BATCH_HEADER_LEN + i * BATCH_LANE_LEN;
            lanes.push(decode_batch_lane(&bytes[start..start + BATCH_LANE_LEN]));
        }
        Ok(Self { lanes })
    }
}

/// Serialized XOF batch state.
/// Debug output is redacted; each lane holds stream state, which continues that stream and its forks.
#[derive(Clone, PartialEq, Eq)]
pub struct LustroXofBatchSnapshot {
    lanes: Vec<StreamLane>,
}

impl core::fmt::Debug for LustroXofBatchSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LustroXofBatchSnapshot")
            .field("lane_count", &self.lanes.len())
            .field("lanes", &"[redacted]")
            .finish()
    }
}

impl TryFrom<&[u8]> for LustroXofBatchSnapshot {
    type Error = SnapshotError;

    fn try_from(bytes: &[u8]) -> Result<Self, SnapshotError> {
        Self::try_from_le_bytes(bytes)
    }
}

impl LustroXofBatchSnapshot {
    #[inline]
    pub(crate) fn new(lanes: Vec<StreamLane>) -> Self {
        Self { lanes }
    }

    #[inline]
    pub(crate) fn into_lanes(self) -> Vec<StreamLane> {
        self.lanes
    }

    /// Serializes the batch snapshot to its variable-length format.
    /// Panics if the output buffer cannot be allocated.
    #[must_use]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self.try_to_le_bytes() {
            Ok(out) => out,
            Err(BatchError::Reserve(e)) => panic!("to_le_bytes: allocation failed: {e:?}"),
            Err(e) => panic!("to_le_bytes: {e}"),
        }
    }

    /// As `to_le_bytes`, returning `Err(BatchError::Reserve)` if reserving the output fails.
    /// Never returns another variant.
    pub fn try_to_le_bytes(&self) -> Result<Vec<u8>, BatchError> {
        let lane_count = self.lanes.len() as u64;
        let mut out = Vec::new();
        // No overflow: the lanes already fill at most isize::MAX bytes, at least 40 each.
        out.try_reserve_exact(BATCH_HEADER_LEN + self.lanes.len() * BATCH_LANE_LEN)
            .map_err(BatchError::Reserve)?;
        out.extend_from_slice(&encode_batch_header(SnapshotKind::XofBatch, lane_count));
        for &StreamLane { s0, s1, step } in &self.lanes {
            let mut lane_bytes = [0u8; BATCH_LANE_LEN];
            encode_batch_lane(&mut lane_bytes, s0, s1, step);
            out.extend_from_slice(&lane_bytes);
        }
        Ok(out)
    }

    /// Deserializes an XOF batch snapshot.
    /// Rejects trailing or missing bytes. Panics if the lane buffer cannot be allocated.
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        match Self::try_from_le_bytes(bytes) {
            Err(SnapshotError::Reserve) => panic!("from_le_bytes: lane allocation failed"),
            r => r,
        }
    }

    /// As `from_le_bytes`, returning `Err(SnapshotError::Reserve)` if reserving the lane
    /// buffer fails.
    pub fn try_from_le_bytes(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let lane_count = decode_batch_header(bytes, SnapshotKind::XofBatch)?;
        let mut lanes = Vec::new();
        lanes
            .try_reserve_exact(lane_count as usize)
            .map_err(|_| SnapshotError::Reserve)?;
        for i in 0..lane_count as usize {
            let start = BATCH_HEADER_LEN + i * BATCH_LANE_LEN;
            lanes.push(decode_batch_lane(&bytes[start..start + BATCH_LANE_LEN]));
        }
        Ok(Self { lanes })
    }
}
