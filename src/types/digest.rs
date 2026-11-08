//! Lustro V1 — digest types.

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

/// Why a hex string was rejected by `Hash128::from_str` or `Hash256::from_str`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseHashError {
    /// The input does not hold exactly `expected` hex digits (32 for `Hash128`,
    /// 64 for `Hash256`). `actual` counts the bytes after an optional `0x`/`0X` prefix.
    InvalidLength { expected: usize, actual: usize },
    /// Byte at `index` is not a hex digit. `index` is the byte offset in the whole
    /// input, prefix included. The length is checked first.
    InvalidDigit { index: usize },
}

impl core::fmt::Display for ParseHashError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ParseHashError::InvalidLength { expected, actual } => {
                write!(f, "expected {expected} hex digits, got {actual} bytes")
            }
            ParseHashError::InvalidDigit { index } => {
                write!(f, "invalid hex digit at byte {index}")
            }
        }
    }
}

impl std::error::Error for ParseHashError {}

/// Why `update` or `finalize` of `LustroHasher` or `LustroXofAbsorber` failed. Nothing is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HashError {
    /// `update` was given more bytes than are left of the declared length.
    TooMuchData,
    /// `finalize` was called before the declared length was reached.
    TooLittleData,
}

impl core::fmt::Display for HashError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HashError::TooMuchData => f.write_str("more data than the declared length"),
            HashError::TooLittleData => f.write_str("less data than the declared length"),
        }
    }
}

impl std::error::Error for HashError {}

#[inline]
fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// Parses 2 * N hex digits after an optional "0x"/"0X". No whitespace is accepted.
// A digest without prefix never starts with "0x" ('x' is not a hex digit).
fn parse_hex<const N: usize>(s: &str) -> Result<[u8; N], ParseHashError> {
    let bytes = s.as_bytes();
    let skip = match bytes {
        [b'0', b'x' | b'X', ..] => 2,
        _ => 0,
    };
    let digits = &bytes[skip..];
    if digits.len() != 2 * N {
        return Err(ParseHashError::InvalidLength {
            expected: 2 * N,
            actual: digits.len(),
        });
    }
    let mut out = [0u8; N];
    for (i, pair) in digits.chunks_exact(2).enumerate() {
        let hi = hex_value(pair[0]).ok_or(ParseHashError::InvalidDigit {
            index: skip + 2 * i,
        })?;
        let lo = hex_value(pair[1]).ok_or(ParseHashError::InvalidDigit {
            index: skip + 2 * i + 1,
        })?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
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

impl core::str::FromStr for Hash128 {
    type Err = ParseHashError;

    /// Parses 32 hex digits, either case, with an optional `0x`/`0X` prefix.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex::<16>(s).map(Self)
    }
}

impl Hash256 {
    /// Returns the digest bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the first 16 bytes as `Hash128`.
    /// For the same input, `hash128(m) == hash256(m).first_128()`.
    #[inline]
    pub fn first_128(&self) -> Hash128 {
        let mut out = [0u8; 16];
        out.copy_from_slice(&self.0[..16]);
        Hash128(out)
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

impl core::str::FromStr for Hash256 {
    type Err = ParseHashError;

    /// Parses 64 hex digits, either case, with an optional `0x`/`0X` prefix.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex::<32>(s).map(Self)
    }
}
