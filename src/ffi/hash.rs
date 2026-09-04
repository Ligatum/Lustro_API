//! FFI bindings for Lustro Hash.

use crate::errors::LustroError;
use crate::hash::{hash128, hash256};
use super::types::{buf_in, buf_out};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ==========================================
// FFI HASH API
// ==========================================

// Computes a 256-bit hash into `out`.
// `out` must point to at least 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash256(
    data:     *const u8,
    data_len: usize,
    out:      *mut u8,
) -> LustroError {
    let input = match buf_in(data, data_len) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let output = match buf_out(out, 32) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    match catch_unwind(|| hash256(input)) {
        Ok(result) => {
            output.copy_from_slice(result.as_bytes());
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Computes a 128-bit hash into `out`.
// `out` must point to at least 16 bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash128(
    data:     *const u8,
    data_len: usize,
    out:      *mut u8,
) -> LustroError {
    let input = match buf_in(data, data_len) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let output = match buf_out(out, 16) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    match catch_unwind(|| hash128(input)) {
        Ok(result) => {
            output.copy_from_slice(result.as_bytes());
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// ==========================================
// FFI HASH BATCH API
// ==========================================

// Hashes `n` fixed-length messages into `out_ptr`.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash256_many(
    data_ptr: *const u8,
    n:        usize,
    message_len:  usize,
    out_ptr:  *mut u8,
) -> LustroError {
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

    let data = match buf_in(data_ptr, total_in) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    if buf_out(out_ptr, total_out).is_none() {
        return LustroError::InvalidPointer;
    }

    let messages: Vec<&[u8]> = if message_len == 0 {
        vec![&[][..]; n]
    } else {
        data.chunks_exact(message_len).collect()
    };

    let out_blocks: &mut [[u8; 32]] =
        std::slice::from_raw_parts_mut(out_ptr as *mut [u8; 32], n);

    match catch_unwind(AssertUnwindSafe(move || {
        crate::api::absorb_hash256_batch_into(&messages, crate::constants::Domain::Hash as u128, out_blocks)
    })) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

// Hashes `n` fixed-length messages into 128-bit digests.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash128_many(
    data_ptr: *const u8,
    n:        usize,
    message_len:  usize,
    out_ptr:  *mut u8,
) -> LustroError {
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

    let data = match buf_in(data_ptr, total_in) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    if buf_out(out_ptr, total_out).is_none() {
        return LustroError::InvalidPointer;
    }

    let messages: Vec<&[u8]> = if message_len == 0 {
        vec![&[][..]; n]
    } else {
        data.chunks_exact(message_len).collect()
    };

    let out_blocks: &mut [[u8; 16]] =
        std::slice::from_raw_parts_mut(out_ptr as *mut [u8; 16], n);

    match catch_unwind(AssertUnwindSafe(move || {
        crate::api::absorb_hash128_batch_into(&messages, crate::constants::Domain::Hash as u128, out_blocks)
    })) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

/// Hashes `n` variable-length messages.
/// `message_ptrs[i]` must reference `message_lens[i]` bytes.
/// A null pointer is allowed when `message_lens[i] == 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash256_many_var(
    message_ptrs: *const *const u8,
    n:            usize,
    message_lens: *const usize,
    out_ptr:      *mut u8,
) -> LustroError {
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
    if buf_out(out_ptr, total_out).is_none() {
        return LustroError::InvalidPointer;
    }

    let ptrs = std::slice::from_raw_parts(message_ptrs, n);
    let lens = std::slice::from_raw_parts(message_lens, n);

    let mut messages: Vec<&[u8]> = Vec::with_capacity(n);
    for i in 0..n {
        match buf_in(ptrs[i], lens[i]) {
            Some(m) => messages.push(m),
            None => return LustroError::InvalidPointer,
        }
    }

    let out_blocks: &mut [[u8; 32]] =
        std::slice::from_raw_parts_mut(out_ptr as *mut [u8; 32], n);

    match catch_unwind(AssertUnwindSafe(move || {
        crate::api::absorb_hash256_batch_into(&messages, crate::constants::Domain::Hash as u128, out_blocks)
    })) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

/// Hashes `n` variable-length messages into 128-bit digests.
/// Same pointer conventions as `lustro_hash256_many_var`.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash128_many_var(
    message_ptrs: *const *const u8,
    n:            usize,
    message_lens: *const usize,
    out_ptr:      *mut u8,
) -> LustroError {
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
    if buf_out(out_ptr, total_out).is_none() {
        return LustroError::InvalidPointer;
    }

    let ptrs = std::slice::from_raw_parts(message_ptrs, n);
    let lens = std::slice::from_raw_parts(message_lens, n);

    let mut messages: Vec<&[u8]> = Vec::with_capacity(n);
    for i in 0..n {
        match buf_in(ptrs[i], lens[i]) {
            Some(m) => messages.push(m),
            None => return LustroError::InvalidPointer,
        }
    }

    let out_blocks: &mut [[u8; 16]] =
        std::slice::from_raw_parts_mut(out_ptr as *mut [u8; 16], n);

    match catch_unwind(AssertUnwindSafe(move || {
        crate::api::absorb_hash128_batch_into(&messages, crate::constants::Domain::Hash as u128, out_blocks)
    })) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}
