//! Lustro V1 — shared constants: IV, domain IDs.

/// Initialisation vectors — derived from PHI_64
pub(crate) const IV_S0: u128 = 0x9E3779B97F4A7C159E3779B97F4A7C15_u128;
pub(crate) const IV_S1: u128 = 0x6C62272E07BB01426C62272E07BB0142_u128;

/// Domain separation tags
#[repr(u128)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    Hash  = 0x01,
    Prng  = 0x02,
    Xof   = 0x03,
}

// Stream-domain differentiation mask.
// Uses the PRINCE (reflection) alpha constant, repeated across both 64-bit halves.
pub(crate) const STREAM_INIT_MASK: u128 = 0xC0AC29B7C97C50DD_C0AC29B7C97C50DD_u128;