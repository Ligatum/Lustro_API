//! FFI bindings for Lustro Hash.

use super::guarded;
use super::types::{buf_in, buf_out, fits_slice, ranges_overlap, slice_in, slice_out};
use crate::errors::LustroError;
use crate::hash::{hash128, hash256};

// ==========================================
// FFI HASH API
// ==========================================

// Computes a 256-bit hash into `out`.
// `out` must point to at least 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash256(
    data: *const u8,
    data_len: usize,
    out: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if !fits_slice::<u8>(data_len) {
            return LustroError::InvalidLength;
        }
        match ranges_overlap(data, data_len, out as *const u8, 32) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }
        let input = match buf_in(data, data_len) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let output = match buf_out(out, 32) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };

        let result = hash256(input);
        output.copy_from_slice(result.as_bytes());
        LustroError::Ok
    })
}

// Computes a 128-bit hash into `out`.
// `out` must point to at least 16 bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash128(
    data: *const u8,
    data_len: usize,
    out: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if !fits_slice::<u8>(data_len) {
            return LustroError::InvalidLength;
        }
        match ranges_overlap(data, data_len, out as *const u8, 16) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }
        let input = match buf_in(data, data_len) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let output = match buf_out(out, 16) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };

        let result = hash128(input);
        output.copy_from_slice(result.as_bytes());
        LustroError::Ok
    })
}

// ==========================================
// FFI HASH BATCH API
// ==========================================

// Hashes `n` fixed-length messages into `out_ptr`.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash256_many(
    data_ptr: *const u8,
    n: usize,
    message_len: usize,
    out_ptr: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if n == 0 {
            return LustroError::Ok;
        }

        let total_in = match n.checked_mul(message_len) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        let total_out = match n.checked_mul(32) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        if !fits_slice::<u8>(total_in) || !fits_slice::<u8>(total_out) {
            return LustroError::InvalidLength;
        }

        match ranges_overlap(data_ptr, total_in, out_ptr as *const u8, total_out) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }

        let data = match buf_in(data_ptr, total_in) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        if buf_out(out_ptr, total_out).is_none() {
            return LustroError::InvalidPointer;
        }

        let messages: Vec<&[u8]> = if message_len == 0 {
            vec![&[][..]; n]
        } else {
            data.chunks_exact(message_len).collect()
        };

        let out_blocks: &mut [[u8; 32]] = match slice_out(out_ptr as *mut [u8; 32], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        crate::api::absorb_hash256_batch_into(
            &messages,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}

// Hashes `n` fixed-length messages into 128-bit digests.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash128_many(
    data_ptr: *const u8,
    n: usize,
    message_len: usize,
    out_ptr: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if n == 0 {
            return LustroError::Ok;
        }

        let total_in = match n.checked_mul(message_len) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        let total_out = match n.checked_mul(16) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        if !fits_slice::<u8>(total_in) || !fits_slice::<u8>(total_out) {
            return LustroError::InvalidLength;
        }

        match ranges_overlap(data_ptr, total_in, out_ptr as *const u8, total_out) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }

        let data = match buf_in(data_ptr, total_in) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        if buf_out(out_ptr, total_out).is_none() {
            return LustroError::InvalidPointer;
        }

        let messages: Vec<&[u8]> = if message_len == 0 {
            vec![&[][..]; n]
        } else {
            data.chunks_exact(message_len).collect()
        };

        let out_blocks: &mut [[u8; 16]] = match slice_out(out_ptr as *mut [u8; 16], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        crate::api::absorb_hash128_batch_into(
            &messages,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}

/// Hashes `n` variable-length messages.
/// `message_ptrs[i]` must reference `message_lens[i]` bytes.
/// A null pointer is allowed when `message_lens[i] == 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash256_many_var(
    message_ptrs: *const *const u8,
    n: usize,
    message_lens: *const usize,
    out_ptr: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if n == 0 {
            return LustroError::Ok;
        }
        if message_ptrs.is_null() || message_lens.is_null() {
            return LustroError::InvalidPointer;
        }

        let total_out = match n.checked_mul(32) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        if !fits_slice::<*const u8>(n) || !fits_slice::<usize>(n) || !fits_slice::<u8>(total_out) {
            return LustroError::InvalidLength;
        }

        // Check overlap against the ptrs/lens tables before creating
        // a mutable slice over the output buffer.
        let ptrs_len = match n.checked_mul(std::mem::size_of::<*const u8>()) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        let lens_len = match n.checked_mul(std::mem::size_of::<usize>()) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        match ranges_overlap(
            message_ptrs as *const u8,
            ptrs_len,
            out_ptr as *const u8,
            total_out,
        ) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }
        match ranges_overlap(
            message_lens as *const u8,
            lens_len,
            out_ptr as *const u8,
            total_out,
        ) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }

        if buf_out(out_ptr, total_out).is_none() {
            return LustroError::InvalidPointer;
        }

        let ptrs = match slice_in(message_ptrs, n) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let lens = match slice_in(message_lens, n) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };

        let mut messages: Vec<&[u8]> = Vec::with_capacity(n);
        for i in 0..n {
            if !fits_slice::<u8>(lens[i]) {
                return LustroError::InvalidLength;
            }
            match ranges_overlap(ptrs[i], lens[i], out_ptr as *const u8, total_out) {
                Some(true) => return LustroError::InvalidPointer,
                Some(false) => {}
                None => return LustroError::InvalidLength,
            }
            match buf_in(ptrs[i], lens[i]) {
                Some(m) => messages.push(m),
                None => return LustroError::InvalidPointer,
            }
        }

        let out_blocks: &mut [[u8; 32]] = match slice_out(out_ptr as *mut [u8; 32], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        crate::api::absorb_hash256_batch_into(
            &messages,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}

/// Hashes `n` variable-length messages into 128-bit digests.
/// Same pointer conventions as `lustro_hash256_many_var`.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash128_many_var(
    message_ptrs: *const *const u8,
    n: usize,
    message_lens: *const usize,
    out_ptr: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if n == 0 {
            return LustroError::Ok;
        }
        if message_ptrs.is_null() || message_lens.is_null() {
            return LustroError::InvalidPointer;
        }

        let total_out = match n.checked_mul(16) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        if !fits_slice::<*const u8>(n) || !fits_slice::<usize>(n) || !fits_slice::<u8>(total_out) {
            return LustroError::InvalidLength;
        }

        // Check overlap against the ptrs/lens tables before creating
        // a mutable slice over the output buffer.
        let ptrs_len = match n.checked_mul(std::mem::size_of::<*const u8>()) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        let lens_len = match n.checked_mul(std::mem::size_of::<usize>()) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        match ranges_overlap(
            message_ptrs as *const u8,
            ptrs_len,
            out_ptr as *const u8,
            total_out,
        ) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }
        match ranges_overlap(
            message_lens as *const u8,
            lens_len,
            out_ptr as *const u8,
            total_out,
        ) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }

        if buf_out(out_ptr, total_out).is_none() {
            return LustroError::InvalidPointer;
        }

        let ptrs = match slice_in(message_ptrs, n) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let lens = match slice_in(message_lens, n) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };

        let mut messages: Vec<&[u8]> = Vec::with_capacity(n);
        for i in 0..n {
            if !fits_slice::<u8>(lens[i]) {
                return LustroError::InvalidLength;
            }
            match ranges_overlap(ptrs[i], lens[i], out_ptr as *const u8, total_out) {
                Some(true) => return LustroError::InvalidPointer,
                Some(false) => {}
                None => return LustroError::InvalidLength,
            }
            match buf_in(ptrs[i], lens[i]) {
                Some(m) => messages.push(m),
                None => return LustroError::InvalidPointer,
            }
        }

        let out_blocks: &mut [[u8; 16]] = match slice_out(out_ptr as *mut [u8; 16], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        crate::api::absorb_hash128_batch_into(
            &messages,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}
