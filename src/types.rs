//! Lustro V1 — domain types for hashes, seeds, streams, and snapshots.

//=================================
// HASH OUTPUT TYPES
//=================================

/// 128-bit digest, equal to the first 16 bytes of `Hash256`.
/// `Display` and `LowerHex` print the bytes of `as_bytes()` as 32 lowercase hex digits,
/// `UpperHex` as uppercase; `{:#x}` and `{:#X}` add a `0x` prefix.
/// `Ord` compares those bytes lexicographically, not as a number.
#[must_use]
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Hash128(pub [u8; 16]);

/// 256-bit digest as 32 little-endian bytes (`s0 || s1`).
/// `Display` and `LowerHex` print the bytes of `as_bytes()` as 64 lowercase hex digits,
/// `UpperHex` as uppercase; `{:#x}` and `{:#X}` add a `0x` prefix.
/// `Ord` compares those bytes lexicographically, not as a number.
#[must_use]
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Hash256(pub [u8; 32]);

/// Identifies a PRNG stream.
/// Different IDs derive independent streams from the same seed.
/// `Ord` compares the identifiers numerically.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StreamId(pub u128);

//=================================
// PRNG STREAM ID
//=================================

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
// SEED & KEY MATERIAL
//=================================

/// 256-bit seed or key material.
/// Debug output is redacted; `Copy` is intentional.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Seed256(pub [u8; 32]);

impl core::fmt::Debug for Seed256 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Seed256").field(&"[redacted]").finish()
    }
}

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

//==========================
// PRNG/XOF SNAPSHOT TYPES
//==========================

// Byte 0 of every snapshot. Independent of the API and crate versions.
const SNAPSHOT_VERSION: u8 = 1;

/// Identifies the generator family encoded in a snapshot.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// Why a snapshot could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotError {
    /// Version byte is not supported.
    UnsupportedVersion,
    /// Kind byte does not match the expected type.
    InvalidKind,
    /// Cursor is outside `1..=32`.
    InvalidCursor,
    /// Length does not match the format.
    InvalidLength,
}

impl core::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SnapshotError::UnsupportedVersion => write!(f, "unsupported snapshot version"),
            SnapshotError::InvalidKind => write!(f, "snapshot kind does not match this type"),
            SnapshotError::InvalidCursor => write!(f, "invalid snapshot cursor value"),
            SnapshotError::InvalidLength => write!(f, "invalid snapshot length"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Why a path derivation was rejected by a `try_derive_path` function.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DerivePathError {
    /// `path` is empty.
    EmptyPath,
    /// Reserving the lane buffer failed (allocator refusal or capacity overflow).
    /// Batch functions only.
    Reserve(std::collections::TryReserveError),
}

impl core::fmt::Display for DerivePathError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DerivePathError::EmptyPath => write!(f, "path must not be empty"),
            DerivePathError::Reserve(_) => write!(f, "lane buffer reservation failed"),
        }
    }
}

impl std::error::Error for DerivePathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DerivePathError::EmptyPath => None,
            DerivePathError::Reserve(e) => Some(e),
        }
    }
}

/// Why a batch could not be built by a `try_new_range` or `try_fork_many` function.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchError {
    /// The lane count overflows `usize`.
    SizeOverflow,
    /// Reserving the lane buffer failed (allocator refusal or capacity overflow).
    Reserve(std::collections::TryReserveError),
}

impl core::fmt::Display for BatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BatchError::SizeOverflow => write!(f, "lane count overflows usize"),
            BatchError::Reserve(_) => write!(f, "lane buffer reservation failed"),
        }
    }
}

impl std::error::Error for BatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BatchError::SizeOverflow => None,
            BatchError::Reserve(e) => Some(e),
        }
    }
}

// Lane count of `fork_many`: `lanes * ids`.
#[inline]
pub(crate) fn fork_many_lane_count(lanes: usize, ids: usize) -> Result<usize, BatchError> {
    lanes.checked_mul(ids).ok_or(BatchError::SizeOverflow)
}

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
    if cursor == 0 || cursor > 32 {
        return Err(SnapshotError::InvalidCursor);
    }
    let s0 = u128::from_le_bytes(bytes[8..24].try_into().unwrap());
    let s1 = u128::from_le_bytes(bytes[24..40].try_into().unwrap());
    let step = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
    Ok((s0, s1, step, cursor))
}

/// Serialized PRNG stream state.
/// Debug output is redacted; `s0`/`s1` are secret-derived state, same as `Seed256`.
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
/// Debug output is redacted; `s0`/`s1` are secret-derived state, same as `Seed256`.
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

    /// Deserializes a XOF snapshot from the 56-byte format.
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
#[cfg(feature = "ffi")]
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

    Ok(lane_count)
}

/// Serialized PRNG batch state.
/// Debug output is redacted; lanes hold secret-derived state, same as `Seed256`.
#[derive(Clone, PartialEq, Eq)]
pub struct LustroPrngBatchSnapshot {
    lanes: Vec<(u128, u128, u64)>,
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
        Self::from_le_bytes(bytes)
    }
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

    /// Serializes the batch snapshot to its variable-length format.
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

    /// Deserializes a PRNG batch snapshot.
    /// Rejects trailing or missing bytes.
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

/// Serialized XOF batch state.
/// Debug output is redacted; lanes hold secret-derived state, same as `Seed256`.
#[derive(Clone, PartialEq, Eq)]
pub struct LustroXofBatchSnapshot {
    lanes: Vec<(u128, u128, u64)>,
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
        Self::from_le_bytes(bytes)
    }
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

    /// Serializes the batch snapshot to its variable-length format.
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

    /// Deserializes a XOF batch snapshot.
    /// Rejects trailing or missing bytes.
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

// Writes `bytes` (at most 32) as hex through `Formatter::pad`, so width, fill,
// alignment and precision apply as for `str`. `prefix` adds "0x".
fn write_hex(
    f: &mut core::fmt::Formatter<'_>,
    bytes: &[u8],
    upper: bool,
    prefix: bool,
) -> core::fmt::Result {
    debug_assert!(bytes.len() <= 32);
    let digits: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut buf = [0u8; 66];
    let mut n = 0;
    if prefix {
        buf[..2].copy_from_slice(b"0x");
        n = 2;
    }
    for &b in bytes {
        buf[n] = digits[(b >> 4) as usize];
        buf[n + 1] = digits[(b & 0x0f) as usize];
        n += 2;
    }
    f.pad(core::str::from_utf8(&buf[..n]).map_err(|_| core::fmt::Error)?)
}

impl Hash128 {
    /// Returns the digest bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    // Constructs Hash128 from native state.
    #[inline]
    pub(crate) fn from_state(s0: u128) -> Self {
        Self(s0.to_le_bytes())
    }
}

impl AsRef<[u8; 16]> for Hash128 {
    #[inline]
    fn as_ref(&self) -> &[u8; 16] {
        &self.0
    }
}

impl AsRef<[u8]> for Hash128 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<&[u8]> for Hash128 {
    type Error = core::array::TryFromSliceError;

    // Constructs Hash128 from a byte slice of exactly 16 bytes.
    #[inline]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}

impl From<[u8; 16]> for Hash128 {
    #[inline]
    fn from(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

impl From<Hash128> for [u8; 16] {
    #[inline]
    fn from(hash: Hash128) -> Self {
        hash.0
    }
}

impl core::fmt::Display for Hash128 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(f, &self.0, false, false)
    }
}

impl core::fmt::LowerHex for Hash128 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(f, &self.0, false, f.alternate())
    }
}

impl core::fmt::UpperHex for Hash128 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(f, &self.0, true, f.alternate())
    }
}

impl Hash256 {
    /// Returns the digest bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

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
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl AsRef<[u8]> for Hash256 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<&[u8]> for Hash256 {
    type Error = core::array::TryFromSliceError;

    // Constructs Hash256 from a byte slice of exactly 32 bytes.
    #[inline]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}

impl From<[u8; 32]> for Hash256 {
    #[inline]
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<Hash256> for [u8; 32] {
    #[inline]
    fn from(hash: Hash256) -> Self {
        hash.0
    }
}

impl core::fmt::Display for Hash256 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(f, &self.0, false, false)
    }
}

impl core::fmt::LowerHex for Hash256 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(f, &self.0, false, f.alternate())
    }
}

impl core::fmt::UpperHex for Hash256 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(f, &self.0, true, f.alternate())
    }
}

impl Seed256 {
    /// Constructs Seed256 from a raw 32-byte array.
    #[inline]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the seed bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    // Splits the seed into native state halves.
    #[inline]
    pub(crate) fn to_state(self) -> (u128, u128) {
        (
            u128::from_le_bytes(self.0[..16].try_into().unwrap()),
            u128::from_le_bytes(self.0[16..].try_into().unwrap()),
        )
    }
}

impl AsRef<[u8; 32]> for Seed256 {
    #[inline]
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl AsRef<[u8]> for Seed256 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<&[u8]> for Seed256 {
    type Error = core::array::TryFromSliceError;

    // Constructs Seed256 from a byte slice of exactly 32 bytes.
    #[inline]
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}

impl From<[u8; 32]> for Seed256 {
    #[inline]
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}
