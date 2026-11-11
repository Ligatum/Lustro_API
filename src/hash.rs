//! Lustro V1 — pure Rust hash API.

use crate::api::{absorb_block, absorb_init, absorb_tail, absorb_with_domain};
use crate::constants::Domain;
use crate::types::{Hash128, Hash256, HashError};

// ==========================================
// RUST HASH API
// ==========================================

/// 256-bit digest of `data`.
pub fn hash256(data: &[u8]) -> Hash256 {
    let (s0, s1) = absorb_with_domain(data, Domain::Hash as u128);
    Hash256::from_state(s0, s1)
}

/// 128-bit digest of `data`: the first 16 bytes of `hash256(data)`.
pub fn hash128(data: &[u8]) -> Hash128 {
    let (s0, _) = absorb_with_domain(data, Domain::Hash as u128);
    Hash128::from_state(s0)
}

// ==========================================
// RUST INCREMENTAL HASH API
// ==========================================

/// `hash256` of a message given in pieces. The length of the whole message enters the
/// initial state, so it is passed to `new`; `update` takes the bytes in pieces of any
/// size and `finalize` returns `hash256` of their concatenation. For a 128-bit digest
/// use `Hash256::first_128()`.
///
/// Errors leave the hasher unchanged: `update` past the declared length rejects the
/// whole piece, and `finalize` before it is reached can be followed by more `update`.
/// `finalize` does not consume the hasher and gives the same digest on every call.
#[must_use]
#[derive(Clone)]
pub struct LustroHasher {
    s0: u128,
    s1: u128,
    total: u64,
    remaining: u64,
    buf: [u8; 32],
    buf_len: usize,
}

impl LustroHasher {
    /// Starts a hash of a message of exactly `total_len` bytes.
    pub fn new(total_len: u64) -> Self {
        Self::with_domain(total_len, Domain::Hash)
    }

    // Also used by `LustroXofAbsorber`: absorption differs only in the domain.
    pub(crate) fn with_domain(total_len: u64, domain: Domain) -> Self {
        let (s0, s1) = absorb_init(total_len, domain as u128);
        Self {
            s0,
            s1,
            total: total_len,
            remaining: total_len,
            buf: [0u8; 32],
            buf_len: 0,
        }
    }

    /// Absorbs the next `data.len()` bytes of the message. Returns
    /// `Err(HashError::TooMuchData)` and absorbs nothing if fewer than `data.len()`
    /// bytes are left.
    pub fn update(&mut self, data: &[u8]) -> Result<(), HashError> {
        if data.len() as u64 > self.remaining {
            return Err(HashError::TooMuchData);
        }
        self.absorb(data);
        Ok(())
    }

    /// `hash256` of the message, or `Err(HashError::TooLittleData)` while bytes are left.
    pub fn finalize(&self) -> Result<Hash256, HashError> {
        if self.remaining != 0 {
            return Err(HashError::TooLittleData);
        }
        Ok(self.digest())
    }

    /// Bytes still expected before `finalize` succeeds.
    #[must_use]
    #[inline]
    pub fn remaining(&self) -> u64 {
        self.remaining
    }

    fn digest(&self) -> Hash256 {
        let (s0, s1) = self.final_state();
        Hash256::from_state(s0, s1)
    }

    // All bytes absorbed: the terminator, when the message needs one, on a copy of the state.
    pub(crate) fn final_state(&self) -> (u128, u128) {
        absorb_tail(self.s0, self.s1, &self.buf[..self.buf_len], self.total == 0)
    }

    // `data.len()` must not exceed `remaining`. Full blocks are absorbed as soon as they
    // are complete: the last one needs no change when the length is a multiple of 32.
    fn absorb(&mut self, mut data: &[u8]) {
        self.remaining -= data.len() as u64;
        if self.buf_len != 0 {
            let take = (32 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len < 32 {
                return;
            }
            (self.s0, self.s1) = absorb_block(self.s0, self.s1, &self.buf);
            self.buf_len = 0;
        }
        let chunks = data.chunks_exact(32);
        let rest = chunks.remainder();
        let (mut s0, mut s1) = (self.s0, self.s1);
        for block in chunks {
            (s0, s1) = absorb_block(s0, s1, block.try_into().unwrap());
        }
        (self.s0, self.s1) = (s0, s1);
        self.buf[..rest.len()].copy_from_slice(rest);
        self.buf_len = rest.len();
    }
}

// Redacted: the state and the buffered bytes come from the message.
impl core::fmt::Debug for LustroHasher {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LustroHasher")
            .field("remaining", &self.remaining)
            .finish_non_exhaustive()
    }
}

/// `hash256` of the next `total_len` bytes of `reader`, read in pieces of up to 8 KiB.
/// Bytes after them are left in `reader` (pass `&mut reader` to keep using it).
/// Returns `ErrorKind::UnexpectedEof` if `reader` ends earlier, `ErrorKind::InvalidData` if
/// a `read` reports more bytes than it was given room for, and passes on other read errors;
/// `ErrorKind::Interrupted` is retried. With `total_len == 0` nothing is read.
pub fn hash256_reader<R: std::io::Read>(total_len: u64, reader: R) -> std::io::Result<Hash256> {
    let mut hasher = LustroHasher::new(total_len);
    hasher.read_rest(reader)?;
    Ok(hasher.digest())
}

impl LustroHasher {
    // Reads exactly `remaining` bytes; errors as documented on `hash256_reader`.
    pub(crate) fn read_rest<R: std::io::Read>(&mut self, mut reader: R) -> std::io::Result<()> {
        let mut buf = [0u8; 8192];
        while self.remaining != 0 {
            let want = self.remaining.min(buf.len() as u64) as usize;
            match reader.read(&mut buf[..want]) {
                Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                Ok(n) if n > want => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "read returned more bytes than requested",
                    ))
                }
                Ok(n) => self.absorb(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

// ==========================================
// RUST HASH BATCH API
// ==========================================

/// Hashes the `data.len() / message_len` messages of `message_len` bytes each, stored back to
/// back in `data`, and returns their 256-bit digests.
///
/// Panics if `message_len` is 0 (the message count is undefined, use
/// `hash256_many_fixed_into`) or if `data.len()` is not a multiple of `message_len`.
pub fn hash256_many_fixed(data: &[u8], message_len: usize) -> Vec<Hash256> {
    let mut out = vec![Hash256::default(); fixed_count("hash256_many_fixed", data, message_len)];
    hash256_many_fixed_into(data, message_len, &mut out);
    out
}

/// As `hash256_many_fixed`, returning 128-bit digests.
pub fn hash128_many_fixed(data: &[u8], message_len: usize) -> Vec<Hash128> {
    let mut out = vec![Hash128::default(); fixed_count("hash128_many_fixed", data, message_len)];
    hash128_many_fixed_into(data, message_len, &mut out);
    out
}

// Message count of `data` split into `message_len` pieces; panics as documented above.
fn fixed_count(name: &str, data: &[u8], message_len: usize) -> usize {
    assert!(
        message_len != 0,
        "{name}: message_len must not be 0, use the _into variant"
    );
    assert!(
        data.len().is_multiple_of(message_len),
        "{name}: data.len() must be a multiple of message_len"
    );
    data.len() / message_len
}

/// Hashes N independent messages of any lengths and returns N 256-bit digests.
pub fn hash256_many_var(messages: &[&[u8]]) -> Vec<Hash256> {
    let mut out = vec![Hash256::default(); messages.len()];
    hash256_many_var_into(messages, &mut out);
    out
}

/// Hashes N independent messages of any lengths and returns N 128-bit digests.
pub fn hash128_many_var(messages: &[&[u8]]) -> Vec<Hash128> {
    let mut out = vec![Hash128::default(); messages.len()];
    hash128_many_var_into(messages, &mut out);
    out
}

/// Hashes `out.len()` messages of `message_len` bytes each, stored back to back in
/// `data`, writing 256-bit digests directly into `out`. Batches of 4 or more messages
/// can use AVX2.
///
/// Panics if `data.len() != out.len() * message_len`.
pub fn hash256_many_fixed_into(data: &[u8], message_len: usize, out: &mut [Hash256]) {
    assert_eq!(
        Some(data.len()),
        out.len().checked_mul(message_len),
        "hash256_many_fixed_into: data.len() must equal out.len() * message_len"
    );
    // SAFETY: Hash256 is #[repr(transparent)] over [u8; 32], so it has identical layout and alignment.
    let out_blocks: &mut [[u8; 32]] =
        unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut [u8; 32], out.len()) };
    crate::api::absorb_hash256_fixed_into(data, message_len, Domain::Hash as u128, out_blocks);
}

/// As `hash256_many_fixed_into`, writing 128-bit digests.
pub fn hash128_many_fixed_into(data: &[u8], message_len: usize, out: &mut [Hash128]) {
    assert_eq!(
        Some(data.len()),
        out.len().checked_mul(message_len),
        "hash128_many_fixed_into: data.len() must equal out.len() * message_len"
    );
    // SAFETY: Hash128 is #[repr(transparent)] over [u8; 16], so it has identical layout and alignment.
    let out_blocks: &mut [[u8; 16]] =
        unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut [u8; 16], out.len()) };
    crate::api::absorb_hash128_fixed_into(data, message_len, Domain::Hash as u128, out_blocks);
}

/// Hashes N independent messages of any lengths, writing the 256-bit digests into `out`.
/// Batches of 4 or more messages can use AVX2.
/// Panics if `messages.len() != out.len()`.
pub fn hash256_many_var_into(messages: &[&[u8]], out: &mut [Hash256]) {
    assert_eq!(
        messages.len(),
        out.len(),
        "hash256_many_var_into: length mismatch"
    );
    // SAFETY: Hash256 is #[repr(transparent)] over [u8; 32], so it has identical layout and alignment.
    let out_blocks: &mut [[u8; 32]] =
        unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut [u8; 32], out.len()) };
    crate::api::absorb_hash256_var_into(messages, Domain::Hash as u128, out_blocks);
}

/// As `hash256_many_var_into`, writing 128-bit digests.
/// Panics if `messages.len() != out.len()`.
pub fn hash128_many_var_into(messages: &[&[u8]], out: &mut [Hash128]) {
    assert_eq!(
        messages.len(),
        out.len(),
        "hash128_many_var_into: length mismatch"
    );
    // SAFETY: Hash128 is #[repr(transparent)] over [u8; 16], so it has identical layout and alignment.
    let out_blocks: &mut [[u8; 16]] =
        unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut [u8; 16], out.len()) };
    crate::api::absorb_hash128_var_into(messages, Domain::Hash as u128, out_blocks);
}
