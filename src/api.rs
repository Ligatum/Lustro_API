use crate::core::{idm_scalar_ref, erd_round_scalar_ref};
use crate::constants::STREAM_INIT_MASK;

pub const LUSTRO_API_VERSION: u32 = 1;

#[inline]
pub fn lustro_api_version() -> u32 {
    LUSTRO_API_VERSION
}

// ==========================================
// INTERNAL CORE API
// ==========================================

// PRNG step: IDM + ERD with counter.
#[inline(always)]
pub(crate) fn stream_step(
    s0: u128,
    s1: u128,
    step: u64,
) -> (u128, u128) {
    let (s0, s1) = idm_scalar_ref(s0, s1);
    erd_round_scalar_ref(s0, s1, step)
}

// Stateless IDM + ERD pipeline.
#[inline(always)]
pub(crate) fn evaluate_scalar(
    s0_in: u128,
    s1_in: u128,
) -> (u128, u128) {
    let (s0, s1) = idm_scalar_ref(s0_in, s1_in);
    erd_round_scalar_ref(s0, s1, 0)
}

// ==========================================
// INTERNAL BRANCH DERIVATION API
// ==========================================
// Shared pipeline for stream and fork derivation:
// prepare_base -> derive_branch_stream.

// Stage 1: applies domain and stream initialization constants.
// Result can be reused for sibling derivations.
#[inline(always)]
pub(crate) fn prepare_base(s0: u128, s1: u128, domain: u128) -> (u128, u128) {
    (s0 ^ domain, s1 ^ STREAM_INIT_MASK)
}

// Stage 2: derives a branch state by mixing the branch identifier.
#[inline(always)]
pub(crate) fn derive_branch_stream(base_s0: u128, base_s1: u128, id: u128) -> (u128, u128) {
    evaluate_scalar(base_s0, base_s1 ^ id)
}

// Derives a forked lane without requiring StreamState.
#[inline(always)]
pub(crate) fn fork_lane(s0: u128, s1: u128, domain: u128, id: u128) -> (u128, u128) {
    let (base_s0, base_s1) = prepare_base(s0, s1, domain);
    derive_branch_stream(base_s0, base_s1, id)
}

// ==========================================
// INTERNAL HASH API
// ==========================================

// Absorbs 32-byte blocks with domain and bit-length separation.
// Partial input is terminated with 0x80 and zero-padded.
#[inline]
pub(crate) fn absorb_with_domain(data: &[u8], domain: u128) -> (u128, u128) {
    use crate::constants::{IV_S0, IV_S1};
    let mut s0 = IV_S0;
    let mut s1 = IV_S1;
    s0 ^= domain;
    let len64 = (data.len() as u64).wrapping_mul(8);
    s1 ^= ((len64.rotate_left(17) as u128) << 64) | (len64 as u128);

    let chunks = data.chunks_exact(32);
    let remainder = chunks.remainder();

    for block in chunks {
        let b0 = u128::from_le_bytes(block[..16].try_into().unwrap());
        let b1 = u128::from_le_bytes(block[16..].try_into().unwrap());
        s0 ^= b0;
        s1 ^= b1;
        (s0, s1) = evaluate_scalar(s0, s1);
    }

    // Length is already committed; no terminator round is needed.
    if remainder.is_empty() && !data.is_empty() {
        return (s0, s1);
    }

    finalize_terminator(s0, s1, remainder)
}

// Absorbs the final partial block with 0x80 termination and zero-padding.
fn finalize_terminator(mut s0: u128, mut s1: u128, remainder: &[u8]) -> (u128, u128) {
    debug_assert!(remainder.len() < 32, "finalize_terminator: remainder must be a partial block");
    let mut buf = [0u8; 32];
    buf[..remainder.len()].copy_from_slice(remainder);
    buf[remainder.len()] = 0x80;
    let b0 = u128::from_le_bytes(buf[..16].try_into().unwrap());
    let b1 = u128::from_le_bytes(buf[16..].try_into().unwrap());
    s0 ^= b0;
    s1 ^= b1;
    if remainder.len() < 16 {
        s1 ^= b0;
    }
    evaluate_scalar(s0, s1)
}

// Hashes independent messages using scalar or parallel dispatch.
pub(crate) fn absorb_hash256_batch_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 32]]) {
    crate::dispatch::dispatch_hash256_batch_into(messages, domain, out);
}

pub(crate) fn absorb_hash128_batch_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 16]]) {
    crate::dispatch::dispatch_hash128_batch_into(messages, domain, out);
}

// ==========================================
// INTERNAL PRNG API
// ==========================================

// Shared PRNG stream state and buffer management.
#[derive(Clone)]
pub(crate) struct StreamState {
    s0:     u128,
    s1:     u128,
    step:   u64,
    cursor: u8,
}

impl StreamState {
    // Creates a stream with an empty output buffer.
    pub(crate) fn new(s0: u128, s1: u128) -> Self {
        Self {
            s0,
            s1,
            step: 0,
            cursor: 32,
        }
    }

    #[inline]
    fn refill(&mut self) {
        (self.s0, self.s1) = stream_step(self.s0, self.s1, self.step);
        self.step = self.step.wrapping_add(1);
        self.cursor = 0;
    }

    pub(crate) fn fill_bytes(&mut self, out: &mut [u8]) {
        let mut written = 0;

        // Drain the current partial block first.
        if self.cursor > 0 && self.cursor < 32 && written < out.len() {
            let pos       = self.cursor as usize;
            let available = 32 - pos;
            let chunk     = available.min(out.len());

            if pos < 16 {
                let s0 = self.s0.to_le_bytes();
                let from_s0 = (16 - pos).min(chunk);
                out[..from_s0].copy_from_slice(&s0[pos..pos + from_s0]);
                let remaining = chunk - from_s0;
                if remaining > 0 {
                    out[from_s0..chunk].copy_from_slice(&self.s1.to_le_bytes()[..remaining]);
                }
            } else {
                let s1_pos = pos - 16;
                out[..chunk].copy_from_slice(&self.s1.to_le_bytes()[s1_pos..s1_pos + chunk]);
            }

            self.cursor += chunk as u8;
            written = chunk;
        }

        // Bulk path: each iteration consumes a fresh 32-byte block.
        // Preserve a partial cursor when the loop does not run.
        let mut bulk_ran = false;
        while out.len() - written >= 32 {
            bulk_ran = true;
            self.refill();
            let s0 = self.s0.to_le_bytes();
            let s1 = self.s1.to_le_bytes();
            out[written..written + 16].copy_from_slice(&s0);
            out[written + 16..written + 32].copy_from_slice(&s1);
            written += 32;
        }
        if bulk_ran {
            self.cursor = 32;
        }

        // Partial tail starts from a fresh block.
        if written < out.len() {
            self.refill();
            let s0 = self.s0.to_le_bytes();
            let chunk = out.len() - written;
            let from_s0 = chunk.min(16);
            out[written..written + from_s0].copy_from_slice(&s0[..from_s0]);
            if chunk > 16 {
                out[written + 16..written + chunk].copy_from_slice(&self.s1.to_le_bytes()[..chunk - 16]);
            }
            self.cursor = chunk as u8;
        }
    }

    // Reads exactly N bytes.
    #[inline]
    pub(crate) fn read_bytes<const N: usize>(&mut self) -> [u8; N] {
        debug_assert!(N <= 32, "read_bytes: N={} exceeds buffer size 32", N);
        let mut out = [0u8; N];
        self.fill_bytes(&mut out);
        out
    }

    // Returns the next 32-byte output block.
    #[inline]
    pub(crate) fn read_full_block(&mut self) -> [u8; 32] {
        self.refill();
        self.cursor = 32;
        let mut out = [0u8; 32];
        out[..16].copy_from_slice(&self.s0.to_le_bytes());
        out[16..].copy_from_slice(&self.s1.to_le_bytes());
        out
    }

    // Derives a stream from the current state and identifier.
    // Cursor is not part of the derivation and does not affect the result.
    #[inline]
    pub(crate) fn fork(&self, domain: u128, id: u128) -> Self {
        let (s0, s1) = fork_lane(self.s0, self.s1, domain, id);
        Self::new(s0, s1)
    }

    // Returns all internal state components.
    #[inline]
    pub(crate) fn to_parts(&self) -> (u128, u128, u64, u8) {
        (self.s0, self.s1, self.step, self.cursor)
    }

    // Reconstructs StreamState from internal state components.
    // Panics if `cursor > 32`.
    #[inline]
    pub(crate) fn from_parts(s0: u128, s1: u128, step: u64, cursor: u8) -> Self {
        assert!(cursor <= 32, "StreamState::from_parts: cursor cannot exceed 32");
        Self { s0, s1, step, cursor }
    }
}

impl core::fmt::Debug for StreamState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StreamState")
            .field("step", &self.step)
            .field("s0", &"[redacted]")
            .field("s1", &"[redacted]")
            .finish()
    }
}
