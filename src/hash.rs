//! Lustro V1 — pure Rust hash API.

use crate::api::{absorb_with_domain};
use crate::constants::Domain;
use crate::types::{Hash128, Hash256};

// ==========================================
// RUST HASH API
// ==========================================

// CONVENIENCE RUST API.
pub fn hash256(data: &[u8]) -> Hash256 {
    let (s0, s1) = absorb_with_domain(data, Domain::Hash as u128);
    Hash256::from_state(s0, s1)
}

// CONVENIENCE RUST API.
pub fn hash128(data: &[u8]) -> Hash128 {
    let (s0, _) = absorb_with_domain(data, Domain::Hash as u128);
    Hash128::from_state(s0)
}

// ==========================================
// RUST HASH BATCH API
// ==========================================

// CONVENIENCE RUST API.
// Hashes N independent messages and returns N 256-bit digests.
pub fn hash256_many(messages: &[&[u8]]) -> Vec<Hash256> {
    let mut out = vec![Hash256::default(); messages.len()];
    hash256_many_into(messages, &mut out);
    out
}

// CONVENIENCE RUST API.
// Hashes N independent messages and returns N 128-bit digests.
pub fn hash128_many(messages: &[&[u8]]) -> Vec<Hash128> {
    let mut out = vec![Hash128::default(); messages.len()];
    hash128_many_into(messages, &mut out);
    out
}

// PERFORMANCE RUST API.
// Hashes N independent messages, writing 256-bit digests directly
// into `out` — avoids the intermediate Vec<(u128,u128)>.
pub fn hash256_many_into(messages: &[&[u8]], out: &mut [Hash256]) {
    assert_eq!(messages.len(), out.len(), "hash256_many_into: length mismatch");
    // SAFETY: Hash256 is #[repr(transparent)] over [u8; 32], so it has identical layout and alignment.
    let out_blocks: &mut [[u8; 32]] = unsafe {
        std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut [u8; 32], out.len())
    };
    crate::api::absorb_hash256_batch_into(messages, Domain::Hash as u128, out_blocks);
}

// PERFORMANCE RUST API.
// Hashes N independent messages, writing 128-bit digests directly
// into `out` — avoids the intermediate Vec<(u128,u128)>.
pub fn hash128_many_into(messages: &[&[u8]], out: &mut [Hash128]) {
    assert_eq!(messages.len(), out.len(), "hash128_many_into: length mismatch");
    // SAFETY: Hash128 is #[repr(transparent)] over [u8; 16], so it has identical layout and alignment.
    let out_blocks: &mut [[u8; 16]] = unsafe {
        std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut [u8; 16], out.len())
    };
    crate::api::absorb_hash128_batch_into(messages, Domain::Hash as u128, out_blocks);
}