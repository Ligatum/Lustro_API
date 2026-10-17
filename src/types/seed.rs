//! Lustro V1 — seed material.

/// 256-bit seed or key material.
/// Debug output is redacted; `Copy` is intentional.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Seed256(pub [u8; 32]);

impl core::fmt::Debug for Seed256 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Seed256").field(&"[redacted]").finish()
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

/// The system entropy source could not provide a seed.
///
/// The cause reported by the OS is available through [`std::error::Error::source`].
#[cfg(feature = "entropy")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntropyError(getrandom::Error);

#[cfg(feature = "entropy")]
impl core::fmt::Display for EntropyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "system entropy source unavailable")
    }
}

#[cfg(feature = "entropy")]
impl std::error::Error for EntropyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[cfg(feature = "entropy")]
impl Seed256 {
    /// Fills a seed from the operating system's entropy source.
    ///
    /// Returns an error when the source is unavailable (`rand_core`'s
    /// `SeedableRng::from_entropy` panics instead); no partially filled seed is returned.
    /// `LustroPrng` does not implement `CryptoRng`.
    pub fn from_entropy() -> Result<Self, EntropyError> {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(EntropyError)?;
        Ok(Self(bytes))
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
