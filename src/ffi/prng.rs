//! FFI bindings for Lustro PRNG.
//! Each instance is an independent stream.

use crate::errors::LustroError;
use crate::prng::LustroPrng;
use crate::prng::LustroPrngBatch;
use crate::types::{Seed256, StreamId};

use super::types::{buf_in, buf_out};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ==========================================
// FFI STREAM SINGLE API
// ==========================================

// Creates a PRNG context from a 32-byte seed and 128-bit stream ID.
// `stream_id` is passed as `(hi, lo)` u64 values.
// Returns null on invalid input.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_new(
    seed:          *const u8,
    stream_id_hi:  u64,
    stream_id_lo:  u64,
) -> *mut LustroPrng {
    let seed_bytes = match buf_in(seed, 32) {
        Some(s) if s.len() == 32 => s,
        _ => return std::ptr::null_mut(),
    };
    let stream_id = ((stream_id_hi as u128) << 64) | (stream_id_lo as u128);

    match catch_unwind(|| {
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        LustroPrng::new(&seed256, StreamId(stream_id))
    }) {
        Ok(prng) => Box::into_raw(Box::new(prng)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Frees a PRNG context. Passing null is safe (no-op).
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_free(ctx: *mut LustroPrng) {
    if !ctx.is_null() {
        let _ = catch_unwind(|| drop(Box::from_raw(ctx)));
    }
}

// Returns the next 64 random bits.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_u64(
    ctx: *mut LustroPrng,
    out: *mut u64,
) -> LustroError {
    if ctx.is_null() || out.is_null() {
        return LustroError::InvalidPointer;
    }
    let prng = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| prng.next_u64())) {
        Ok(val) => {
            core::ptr::write_unaligned(out, val);
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Returns the next 128 random bits as 16 bytes (LE).
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_u128(
    ctx: *mut LustroPrng,
    out: *mut u8,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, 16) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let prng = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| prng.next_u128())) {
        Ok(val) => {
            output.copy_from_slice(&val.to_le_bytes());
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Returns one full 32-byte engine block.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_block(
    ctx: *mut LustroPrng,
    out: *mut u8,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, 32) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let prng = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| prng.next_block())) {
        Ok(val) => {
            output.copy_from_slice(&val);
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Fills `out` with `out_len` random bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fill(
    ctx:     *mut LustroPrng,
    out:     *mut u8,
    out_len: usize,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, out_len) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let prng = &mut *ctx;

    match catch_unwind(AssertUnwindSafe(|| prng.fill_bytes(output))) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

// Clones the PRNG context and returns a new independent instance.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_clone(
    ctx: *const LustroPrng,
) -> *mut LustroPrng {
    if ctx.is_null() { return std::ptr::null_mut(); }
    let prng = &*ctx;

    match catch_unwind(|| prng.clone()) {
        Ok(cloned) => Box::into_raw(Box::new(cloned)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Derives a child PRNG from the current state and 128-bit identifier.
// `id` is passed as `(hi, lo)` u64 values.
// Returns null on null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fork(
    ctx:    *const LustroPrng,
    id_hi:  u64,
    id_lo:  u64,
) -> *mut LustroPrng {
    if ctx.is_null() { return std::ptr::null_mut(); }
    let prng = &*ctx;
    let id = ((id_hi as u128) << 64) | (id_lo as u128);

    match catch_unwind(|| prng.fork(StreamId(id))) {
        Ok(child) => Box::into_raw(Box::new(child)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Exports the current PRNG snapshot into `out`.
// `out` must provide at least 56 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_export_snapshot(
    ctx: *const LustroPrng,
    out: *mut u8,
) -> LustroError {
    if ctx.is_null() { return LustroError::InvalidPointer; }
    let output = match buf_out(out, 56) {
        Some(s) => s,
        None    => return LustroError::InvalidPointer,
    };
    let prng = &*ctx;

    match catch_unwind(AssertUnwindSafe(|| prng.export_snapshot())) {
        Ok(snapshot) => {
            output.copy_from_slice(&snapshot.to_le_bytes());
            LustroError::Ok
        }
        Err(_) => LustroError::InternalPanic,
    }
}

// Restores a PRNG context from a 56-byte snapshot.
// Returns null on invalid input or decoding failure.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_import_snapshot(
    bytes: *const u8,
) -> *mut LustroPrng {
    let snapshot_bytes = match buf_in(bytes, 56) {
        Some(s) if s.len() == 56 => s,
        _ => return std::ptr::null_mut(),
    };
    let array_ref: &[u8; 56] = snapshot_bytes.try_into().unwrap();

    match catch_unwind(|| {
        if let Ok(snapshot) = crate::types::LustroPrngSnapshot::from_le_bytes(array_ref) {
            Some(LustroPrng::import_snapshot(snapshot))
        } else {
            None
        }
    }) {
        Ok(Some(prng)) => Box::into_raw(Box::new(prng)),
        _ => std::ptr::null_mut(),
    }
}

// ==========================================
// FFI STREAM BATCH API
// ==========================================

// Creates a PRNG batch from `n` stream identifiers.
// IDs are passed as parallel `(hi, lo)` u64 arrays.
// Returns null on invalid input.
//
// # Safety
// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_new(
    seed:    *const u8,
    ids_hi:  *const u64,
    ids_lo:  *const u64,
    n:       usize,
) -> *mut LustroPrngBatch {
    let seed_bytes = match buf_in(seed, 32) {
        Some(s) if s.len() == 32 => s,
        _ => return std::ptr::null_mut(),
    };

    let stream_ids: Vec<StreamId> = if n == 0 {
        Vec::new()
    } else {
        if ids_hi.is_null() || ids_lo.is_null() {
            return std::ptr::null_mut();
        }
        let his = std::slice::from_raw_parts(ids_hi, n);
        let los = std::slice::from_raw_parts(ids_lo, n);
        his.iter().zip(los.iter())
            .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
            .collect()
    };

    match catch_unwind(|| {
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        LustroPrngBatch::new(&seed256, &stream_ids)
    }) {
        Ok(batch) => Box::into_raw(Box::new(batch)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Creates `count` streams with sequential IDs starting at `first_stream_id`.
// `first_stream_id` is passed as `(hi, lo)` u64 values.
// Returns null on invalid input.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_new_range(
    seed:                 *const u8,
    first_hi:   u64,
    first_lo:   u64,
    count:                usize,
) -> *mut LustroPrngBatch {
    let seed_bytes = match buf_in(seed, 32) {
        Some(s) if s.len() == 32 => s,
        _ => return std::ptr::null_mut(),
    };
    let first_id = ((first_hi as u128) << 64) | (first_lo as u128);

    match catch_unwind(|| {
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        LustroPrngBatch::new_range(&seed256, StreamId(first_id), count)
    }) {
        Ok(batch) => Box::into_raw(Box::new(batch)),
        Err(_) => std::ptr::null_mut(),
    }
}

// Frees a batch context. Passing null is safe (no-op).
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_free(ctx: *mut LustroPrngBatch) {
    if !ctx.is_null() {
        let _ = catch_unwind(|| drop(Box::from_raw(ctx)));
    }
}

// Returns the number of streams, or 0 for null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_len(ctx: *const LustroPrngBatch) -> usize {
    if ctx.is_null() { return 0; }
    (*ctx).len()
}

// Advances all streams by one step and writes one 32-byte block per stream.
// `out_len` must equal `batch_len * 32`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fill_blocks(
    ctx:     *mut LustroPrngBatch,
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

    // SAFETY: `out` is non-null and `out_len` was validated for `n * 32` bytes.
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
pub unsafe extern "C" fn lustro_prng_batch_fill_blocks_many(
    ctx:     *mut LustroPrngBatch,
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
    // SAFETY: `out` is non-null and `out_len` was validated for `n * steps * 32` bytes.
    let out_blocks: &mut [[u8; 32]] =
        std::slice::from_raw_parts_mut(out as *mut [u8; 32], n_blocks);

    match catch_unwind(AssertUnwindSafe(|| batch.fill_blocks_many(out_blocks, steps))) {
        Ok(()) => LustroError::Ok,
        Err(_) => LustroError::InternalPanic,
    }
}

// Derives one child PRNG per lane from `n` child identifiers.
// IDs are passed as parallel `(hi, lo)` u64 arrays.
// `n` must equal the batch length.
// Returns null on invalid input.
//
// # Safety
// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork(
    ctx:     *const LustroPrngBatch,
    ids_hi:  *const u64,
    ids_lo:  *const u64,
    n:       usize,
) -> *mut LustroPrngBatch {
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

// Derives one child PRNG per lane with sequential IDs starting at `first`.
// `first` is passed as `(hi, lo)` u64 values.
// Returns null on null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork_range(
    ctx:       *const LustroPrngBatch,
    first_hi:  u64,
    first_lo:  u64,
) -> *mut LustroPrngBatch {
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
pub unsafe extern "C" fn lustro_prng_batch_snapshot_size(ctx: *const LustroPrngBatch) -> usize {
    if ctx.is_null() { return 0; }
    let batch = &*ctx;
    crate::types::batch_snapshot_encoded_len(batch.len()).unwrap_or(0)
}

// Exports the current batch snapshot.
// `out_len` must equal `lustro_prng_batch_snapshot_size(ctx)`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_export_snapshot(
    ctx:     *const LustroPrngBatch,
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

// Restores a PRNG batch from `len` snapshot bytes.
// Returns null on invalid input or decoding failure.
//
// # Safety
// `bytes` must be valid for `len` bytes when `len > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_import_snapshot(
    bytes: *const u8,
    len:   usize,
) -> *mut LustroPrngBatch {
    let snapshot_bytes = match buf_in(bytes, len) {
        Some(s) => s,
        None    => return std::ptr::null_mut(),
    };

    match catch_unwind(|| {
        crate::types::LustroPrngBatchSnapshot::from_le_bytes(snapshot_bytes)
            .ok()
            .map(LustroPrngBatch::import_snapshot)
    }) {
        Ok(Some(batch)) => Box::into_raw(Box::new(batch)),
        _ => std::ptr::null_mut(),
    }
}