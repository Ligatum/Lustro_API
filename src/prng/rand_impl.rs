use rand_core::{RngCore, SeedableRng, Error};
use super::LustroPrng;
use crate::types::{Seed256, StreamId};

impl RngCore for LustroPrng {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut buf = [0u8; 4];
        LustroPrng::fill_bytes(self, &mut buf);
        u32::from_le_bytes(buf)
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        LustroPrng::next_u64(self)
    }

    #[inline]
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        LustroPrng::fill_bytes(self, dest);
    }

    #[inline]
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        LustroPrng::fill_bytes(self, dest);
        Ok(())
    }
}

impl SeedableRng for LustroPrng {
    type Seed = [u8; 32];

    /// Creates stream 0 from a raw 32-byte seed.
    /// Use LustroPrng::new() directly when a non-zero StreamId is required.
    fn from_seed(seed: Self::Seed) -> Self {
        let seed256 = Seed256::from_bytes(seed);
        Self::new(&seed256, StreamId(0))
    }
}

impl From<Seed256> for LustroPrng {
    /// Creates stream 0 from a Seed256.
    /// Use LustroPrng::new() directly when a non-zero StreamId is required.
    fn from(seed: Seed256) -> Self {
        Self::new(&seed, StreamId(0))
    }
}

impl From<[u8; 32]> for LustroPrng {
    /// Creates stream 0 from a raw 32-byte array.
    /// Use LustroPrng::new() directly when a non-zero StreamId is required.
    fn from(seed: [u8; 32]) -> Self {
        Self::from_seed(seed)
    }
}