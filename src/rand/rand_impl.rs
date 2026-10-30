// rand_core 0.10 integration (feature `rand`).
// LustroPrng implements TryRng (and so Rng) and SeedableRng.
// LustroXof implements TryRng (and so Rng) only.
// CryptoRng is not implemented.

use super::LustroPrng;
use crate::types::{Seed256, StreamId};
use crate::xof::LustroXof;
use core::convert::Infallible;
use rand_core::{SeedableRng, TryRng};

// ==========================================
// LUSTRO PRNG
// ==========================================

// `Rng` (next_u32, next_u64, fill_bytes) follows from `Error = Infallible`.
impl TryRng for LustroPrng {
    type Error = Infallible;

    #[inline]
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(LustroPrng::next_u32(self))
    }

    #[inline]
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(LustroPrng::next_u64(self))
    }

    #[inline]
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        LustroPrng::fill_bytes(self, dst);
        Ok(())
    }
}

// `seed_from_u64` uses rand_core's default to expand the `u64` into the 32-byte
// seed; the expansion is defined by rand_core, not by Lustro.
// `SeedableRng::fork(&mut self)` shares its name with the inherent `fork(&self, StreamId)`.
// On `LustroPrng` the inherent one is called; the trait one needs `SeedableRng::fork(&mut rng)`.
impl SeedableRng for LustroPrng {
    type Seed = [u8; 32];

    /// Creates stream 0 from a raw 32-byte seed.
    /// Use LustroPrng::new() directly when a non-zero StreamId is required.
    fn from_seed(seed: Self::Seed) -> Self {
        let seed256 = Seed256::from_bytes(seed);
        Self::new(&seed256, StreamId(0))
    }
}

// ==========================================
// LUSTRO XOF
// ==========================================

// Same stream reads as the inherent methods. No `SeedableRng`: `LustroXof::new` takes a
// message of any length, not a fixed-size seed. Build the stream with `LustroXof::new`.
impl TryRng for LustroXof {
    type Error = Infallible;

    #[inline]
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(LustroXof::next_u32(self))
    }

    #[inline]
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(LustroXof::next_u64(self))
    }

    #[inline]
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        LustroXof::fill_bytes(self, dst);
        Ok(())
    }
}
