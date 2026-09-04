//! FFI bindings for Lustro XOF.

use crate::errors::LustroError;
use crate::xof::LustroXof;
use crate::xof::LustroXofBatch;
use crate::types::StreamId;

use super::types::{buf_in, buf_out};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ==========================================
// FFI XOF SINGLE API
// ==========================================

// Creates an XOF context by absorbing a message.
// Returns null on invalid input.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_new(
    message:     *const u8,
    message_len: usize,
) -> *mut LustroXof {
    let message = match buf_in(message, message_len) {
        Some(s) => s,
        None    => return std::ptr::null_mut(),
    };

    match catch_unwind(|| LustroXof::new(message)) {
        Ok(xof) => Box::into_raw(Box::new(xof)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Frees an XOF context. Passing null is safe (no-op).
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_free(ctx: *mut LustroXof) {
    if !ctx.is_null() {
        let _ = catch_unwind(|| drop(Box::from_raw(ctx)));
    }
}

// Returns the next 64 output bits.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_next_u64(
    ctx: *mut LustroXof,
    out: *mut u64,
) -> LustroError {
    if ctx.is_null() || out.is_null() {
        return LustroError::InvalidPointer;
    }
    let xof = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| xof.next_u64())) {
        Ok(val) => {
            core::ptr::write_unaligned(out, val);
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Returns the next 128 output bits as 16 bytes (LE).
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_next_u128(
    ctx: *mut LustroXof,
    out: *mut u8,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, 16) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let xof = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| xof.next_u128())) {
        Ok(val) => {
            output.copy_from_slice(&val.to_le_bytes());
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Returns one full 32-byte engine block.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_next_block(
    ctx: *mut LustroXof,
    out: *mut u8,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, 32) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let xof = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| xof.next_block())) {
        Ok(val) => {
            output.copy_from_slice(&val);
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Fills `out` with `out_len` output bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fill(
    ctx:     *mut LustroXof,
    out:     *mut u8,
    out_len: usize,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, out_len) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let xof = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| xof.fill_bytes(output))) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

// Clones the XOF context and returns a new independent instance.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_clone(
    ctx: *const LustroXof,
) -> *mut LustroXof {
    if ctx.is_null() { return std::ptr::null_mut(); }
    let xof = &*ctx;

    match catch_unwind(|| xof.clone()) {
        Ok(cloned) => Box::into_raw(Box::new(cloned)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Derives a child XOF from the current state and 128-bit identifier.
// `id` is passed as `(hi, lo)` u64 values.
// Returns null on null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fork(
    ctx:    *const LustroXof,
    id_hi:  u64,
    id_lo:  u64,
) -> *mut LustroXof {
    if ctx.is_null() { return std::ptr::null_mut(); }
    let xof = &*ctx;
    let id = ((id_hi as u128) << 64) | (id_lo as u128);

    match catch_unwind(|| xof.fork(StreamId(id))) {
        Ok(child) => Box::into_raw(Box::new(child)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Exports the current XOF snapshot into `out`.
// `out` must provide at least 56 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_export_snapshot(
    ctx: *const LustroXof,
    out: *mut u8,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, 56) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let xof = &*ctx;

    match catch_unwind(AssertUnwindSafe(|| xof.export_snapshot())) {
        Ok(snapshot) => {
            output.copy_from_slice(&snapshot.to_le_bytes());
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Restores an XOF context from a 56-byte snapshot.
// Returns null on invalid input or decoding failure.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_import_snapshot(
    bytes: *const u8,
) -> *mut LustroXof {
    let snapshot_bytes = match buf_in(bytes, 56) {
        Some(s) if s.len() == 56 => s,
        _ => return std::ptr::null_mut(),
    };
    let array_ref: &[u8; 56] = snapshot_bytes.try_into().unwrap();

    match catch_unwind(|| {
        if let Ok(snapshot) = crate::types::LustroXofSnapshot::from_le_bytes(array_ref) {
            Some(LustroXof::import_snapshot(snapshot))
        } else {
            None
        }
    }) {
        Ok(Some(xof)) => Box::into_raw(Box::new(xof)),
        _ => std::ptr::null_mut(),
    }
}

// ==========================================
// FFI XOF BATCH API
// ==========================================

// Creates an XOF batch from `n` messages.
// Messages are passed as parallel pointer/length arrays.
// Returns null on invalid input.
//
// # Safety
// `message_ptrs` and `message_lens` must each be valid for `n` elements.
// Each `message_ptrs[i]` must be valid for `message_lens[i]` bytes if
// `message_lens[i] > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_new(
    message_ptrs: *const *const u8,
    message_lens: *const usize,
    n: usize,
) -> *mut LustroXofBatch {
    if n == 0 {
        return Box::into_raw(Box::new(LustroXofBatch::new(&[])));
    }
    if message_ptrs.is_null() || message_lens.is_null() {
        return std::ptr::null_mut();
    }

    let ptrs = std::slice::from_raw_parts(message_ptrs, n);
    let lens = std::slice::from_raw_parts(message_lens, n);

    let mut messages: Vec<&[u8]> = Vec::with_capacity(n);
    for i in 0..n {
        match buf_in(ptrs[i], lens[i]) {
            Some(s) => messages.push(s),
            None => return std::ptr::null_mut(),
        }
    }

    match catch_unwind(AssertUnwindSafe(|| LustroXofBatch::new(&messages))) {
        Ok(batch) => Box::into_raw(Box::new(batch)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Frees a batch context. Passing null is safe (no-op).
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_free(ctx: *mut LustroXofBatch) {
    if !ctx.is_null() {
        let _ = catch_unwind(|| drop(Box::from_raw(ctx)));
    }
}

// Returns the number of streams, or 0 for null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_len(ctx: *const LustroXofBatch) -> usize {
    if ctx.is_null() { return 0; }
    (*ctx).len()
}

// Advances all streams by one step and writes one 32-byte block per stream.
// `out_len` must equal `batch_len * 32`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fill_blocks(
    ctx:     *mut LustroXofBatch,
    out:     *mut u8,
    out_len: usize,
) -> LustroError {
    if ctx.is_null() {
        return LustroError::InvalidPointer;
    }
    let batch = &mut *ctx;
    let n = batch.len();

    let expected_len = match n.checked_mul(32) {
        Some(v) => v,
        None => return LustroError::InvalidLength,
    };
    if out_len != expected_len {
        return LustroError::InvalidLength;
    }
    if n == 0 {
        return LustroError::Ok;
    }
    if out.is_null() {
        return LustroError::InvalidPointer;
    }
    // SAFETY: `out` is non-null and `out_len` was validated for `n_blocks * 32` bytes.
    let out_blocks: &mut [[u8; 32]] =
        std::slice::from_raw_parts_mut(out as *mut [u8; 32], n);

    match catch_unwind(AssertUnwindSafe(|| batch.fill_blocks(out_blocks))) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

// Advances all streams by `steps` rounds.
// `out_len` must equal `batch_len * steps * 32`.
// Output is stream-major.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fill_blocks_many(
    ctx:     *mut LustroXofBatch,
    out:     *mut u8,
    out_len: usize,
    steps:   usize,
) -> LustroError {
    if ctx.is_null() {
        return LustroError::InvalidPointer;
    }
    let batch = &mut *ctx;
    let n = batch.len();

    let expected_len = match n.checked_mul(steps).and_then(|v| v.checked_mul(32)) {
        Some(v) => v,
        None => return LustroError::InvalidLength,
    };
    if out_len != expected_len {
        return LustroError::InvalidLength;
    }
    if n == 0 || steps == 0 {
        return LustroError::Ok;
    }
    if out.is_null() {
        return LustroError::InvalidPointer;
    }

    let n_blocks = n * steps;
    // SAFETY: `out` is non-null and `out_len` was validated for `n_blocks * 32` bytes.
    let out_blocks: &mut [[u8; 32]] =
        std::slice::from_raw_parts_mut(out as *mut [u8; 32], n_blocks);

    match catch_unwind(AssertUnwindSafe(|| batch.fill_blocks_many(out_blocks, steps))) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

// Derives one child XOF per lane from `n` child identifiers.
// IDs are passed as parallel `(hi, lo)` u64 arrays.
// `n` must equal the batch length.
// Returns null on invalid input.
//
// # Safety
// `ids_hi` and `ids_lo` must each be valid for `n` elements when n > 0.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork(
    ctx:     *const LustroXofBatch,
    ids_hi:  *const u64,
    ids_lo:  *const u64,
    n:       usize,
) -> *mut LustroXofBatch {
    if ctx.is_null() { return std::ptr::null_mut(); }
    let batch = &*ctx;
    if n != batch.len() { return std::ptr::null_mut(); }

    let stream_ids: Vec<StreamId> = if n == 0 {
        Vec::new()
    } else {
        if ids_hi.is_null() || ids_lo.is_null() { return std::ptr::null_mut(); }
        let his = std::slice::from_raw_parts(ids_hi, n);
        let los = std::slice::from_raw_parts(ids_lo, n);
        his.iter().zip(los.iter())
            .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
            .collect()
    };

    match catch_unwind(AssertUnwindSafe(|| batch.fork(&stream_ids))) {
        Ok(child) => Box::into_raw(Box::new(child)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Derives one child XOF per lane with sequential IDs starting at `first`.
// `first` is passed as `(hi, lo)` u64 values.
// Returns null on null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork_range(
    ctx:       *const LustroXofBatch,
    first_hi:  u64,
    first_lo:  u64,
) -> *mut LustroXofBatch {
    if ctx.is_null() { return std::ptr::null_mut(); }
    let batch = &*ctx;
    let first = ((first_hi as u128) << 64) | (first_lo as u128);

    match catch_unwind(AssertUnwindSafe(|| batch.fork_range(StreamId(first)))) {
        Ok(child) => Box::into_raw(Box::new(child)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Returns the snapshot size in bytes, or 0 for null `ctx`.
// Size: `16 + batch_len * 48`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_snapshot_size(ctx: *const LustroXofBatch) -> usize {
    if ctx.is_null() { return 0; }
    let batch = &*ctx;
    crate::types::batch_snapshot_encoded_len(batch.len()).unwrap_or(0)
}

// Exports the current batch snapshot.
// `out_len` must equal `lustro_xof_batch_snapshot_size(ctx)`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_export_snapshot(
    ctx:     *const LustroXofBatch,
    out:     *mut u8,
    out_len: usize,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let batch = &*ctx;

    let expected_len = match crate::types::batch_snapshot_encoded_len(batch.len()) {
        Some(v) => v,
        None => return LustroError::InvalidLength,
    };
    if out_len != expected_len {
        return LustroError::InvalidLength;
    }
    let output = match buf_out(out, expected_len) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };

    match catch_unwind(AssertUnwindSafe(|| batch.export_snapshot())) {
        Ok(snapshot) => {
            output.copy_from_slice(&snapshot.to_le_bytes());
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Restores an XOF batch from `len` snapshot bytes.
// Returns null on invalid input or decoding failure.
//
// # Safety
// `bytes` must be valid for `len` bytes if len > 0.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_import_snapshot(
    bytes: *const u8,
    len:   usize,
) -> *mut LustroXofBatch {
    let snapshot_bytes = match buf_in(bytes, len) {
        Some(s) => s,
        None    => return std::ptr::null_mut(),
    };

    match catch_unwind(|| {
        crate::types::LustroXofBatchSnapshot::from_le_bytes(snapshot_bytes)
            .ok()
            .map(LustroXofBatch::import_snapshot)
    }) {
        Ok(Some(batch)) => Box::into_raw(Box::new(batch)),
        _ => std::ptr::null_mut(),
    }
}