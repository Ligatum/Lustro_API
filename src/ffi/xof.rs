//! FFI bindings for Lustro XOF.

use crate::errors::LustroError;
use crate::types::StreamId;
use crate::xof::LustroXof;
use crate::xof::LustroXofBatch;

use super::guarded;
use super::types::{buf_in, buf_out, fits_slice, slice_in, slice_out};

// ==========================================
// FFI XOF SINGLE API
// ==========================================

/// Creates an XOF context by absorbing a message.
/// Returns null on invalid input.
///
/// # Safety
/// If `message_len > 0`, `message` must be valid for `message_len` readable
/// bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_new(message: *const u8, message_len: usize) -> *mut LustroXof {
    guarded(std::ptr::null_mut(), || {
        let message = match buf_in(message, message_len) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(LustroXof::new(message)))
    })
}

/// Frees an XOF context. Passing null is safe (no-op).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context
/// allocated by this API. It must not have been freed already or be in use
/// concurrently.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_free(ctx: *mut LustroXof) {
    guarded((), || {
        if !ctx.is_null() {
            drop(Box::from_raw(ctx));
        }
    })
}

/// Returns the next 32 output bits.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context and
/// be exclusively accessible for the duration of the call.
/// If `out` is non-null, it must be valid for 4 writable bytes and must not
/// overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_next_u32(ctx: *mut LustroXof, out: *mut u32) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out.is_null() {
            return LustroError::InvalidPointer;
        }
        let xof = &mut *ctx;

        let val = xof.next_u32();
        core::ptr::write_unaligned(out, val);
        LustroError::Ok
    })
}

/// Returns the next 64 output bits.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context and
/// be exclusively accessible for the duration of the call.
/// If `out` is non-null, it must be valid for 8 writable bytes and must not
/// overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_next_u64(ctx: *mut LustroXof, out: *mut u64) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out.is_null() {
            return LustroError::InvalidPointer;
        }
        let xof = &mut *ctx;

        let val = xof.next_u64();
        core::ptr::write_unaligned(out, val);
        LustroError::Ok
    })
}

/// Returns the next 128 output bits as 16 bytes (LE).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context and
/// be exclusively accessible for the duration of the call.
/// `out` must be valid for 16 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_next_u128(ctx: *mut LustroXof, out: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 16) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let xof = &mut *ctx;

        let val = xof.next_u128();
        output.copy_from_slice(&val.to_le_bytes());
        LustroError::Ok
    })
}

/// Writes the next full 32-byte block. Unread bytes of the current block are discarded.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context and
/// be exclusively accessible for the duration of the call.
/// `out` must be valid for 32 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_next_block(ctx: *mut LustroXof, out: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 32) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let xof = &mut *ctx;

        let val = xof.next_block();
        output.copy_from_slice(&val);
        LustroError::Ok
    })
}

/// Fills `out` with `out_len` output bytes.
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context and
/// be exclusively accessible for the duration of the call.
/// If `out_len > 0`, `out` must be valid for `out_len` writable bytes and
/// must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fill(
    ctx: *mut LustroXof,
    out: *mut u8,
    out_len: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        if !fits_slice::<u8>(out_len) {
            return LustroError::InvalidLength;
        }
        let output = match buf_out(out, out_len) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let xof = &mut *ctx;

        xof.fill_bytes(output);
        LustroError::Ok
    })
}

/// Clones the XOF context and returns a new independent instance.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_clone(ctx: *const LustroXof) -> *mut LustroXof {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let xof = &*ctx;

        Box::into_raw(Box::new(xof.clone()))
    })
}

/// Derives a child XOF from the current state and 128-bit identifier.
/// `id` is passed as `(hi, lo)` u64 values.
/// Returns null on null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fork(
    ctx: *const LustroXof,
    id_hi: u64,
    id_lo: u64,
) -> *mut LustroXof {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let xof = &*ctx;
        let id = ((id_hi as u128) << 64) | (id_lo as u128);

        Box::into_raw(Box::new(xof.fork(StreamId(id))))
    })
}

/// Derives an XOF from a message along a path of `n` identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Returns null on invalid input, including `n == 0`.
///
/// # Safety
/// `message` must be valid for `message_len` bytes when `message_len > 0`.
/// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_derive_path(
    message: *const u8,
    message_len: usize,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
) -> *mut LustroXof {
    guarded(std::ptr::null_mut(), || {
        let message = match buf_in(message, message_len) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        if n == 0 || ids_hi.is_null() || ids_lo.is_null() {
            return std::ptr::null_mut();
        }
        let his = match slice_in(ids_hi, n) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let los = match slice_in(ids_lo, n) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let path: Vec<StreamId> = his
            .iter()
            .zip(los.iter())
            .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
            .collect();

        Box::into_raw(Box::new(LustroXof::derive_path(message, &path)))
    })
}

/// Exports the current XOF snapshot into `out`.
/// `out` must provide at least 56 writable bytes.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// `out` must be valid for 56 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_export_snapshot(
    ctx: *const LustroXof,
    out: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 56) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let xof = &*ctx;

        let snapshot = xof.export_snapshot();
        output.copy_from_slice(&snapshot.to_le_bytes());
        LustroError::Ok
    })
}

/// Restores an XOF context from a 56-byte snapshot.
/// Returns null on invalid input or decoding failure.
///
/// # Safety
/// If `bytes` is non-null, it must be valid for 56 readable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_import_snapshot(bytes: *const u8) -> *mut LustroXof {
    guarded(std::ptr::null_mut(), || {
        let snapshot_bytes = match buf_in(bytes, 56) {
            Some(s) if s.len() == 56 => s,
            _ => return std::ptr::null_mut(),
        };
        let array_ref: &[u8; 56] = snapshot_bytes.try_into().unwrap();

        if let Ok(snapshot) = crate::types::LustroXofSnapshot::from_le_bytes(array_ref) {
            Box::into_raw(Box::new(LustroXof::import_snapshot(snapshot)))
        } else {
            std::ptr::null_mut()
        }
    })
}

// ==========================================
// FFI XOF BATCH API
// ==========================================

/// Creates an XOF batch from `n` messages.
/// Messages are passed as parallel pointer/length arrays.
/// Returns null on invalid input.
///
/// # Safety
/// When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
/// `n` readable elements.
/// Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
/// when `message_lens[i] > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_new(
    message_ptrs: *const *const u8,
    message_lens: *const usize,
    n: usize,
) -> *mut LustroXofBatch {
    guarded(std::ptr::null_mut(), || {
        if n == 0 {
            return Box::into_raw(Box::new(LustroXofBatch::new(&[])));
        }
        if message_ptrs.is_null() || message_lens.is_null() {
            return std::ptr::null_mut();
        }

        let ptrs = match slice_in(message_ptrs, n) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let lens = match slice_in(message_lens, n) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        let mut messages: Vec<&[u8]> = Vec::with_capacity(n);
        for i in 0..n {
            match buf_in(ptrs[i], lens[i]) {
                Some(s) => messages.push(s),
                None => return std::ptr::null_mut(),
            }
        }

        Box::into_raw(Box::new(LustroXofBatch::new(&messages)))
    })
}

/// Frees a batch context. Passing null is safe (no-op).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// allocated by this API. It must not have been freed already or be in use
/// concurrently.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_free(ctx: *mut LustroXofBatch) {
    guarded((), || {
        if !ctx.is_null() {
            drop(Box::from_raw(ctx));
        }
    })
}

/// Returns the number of streams, or 0 for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_len(ctx: *const LustroXofBatch) -> usize {
    if ctx.is_null() {
        return 0;
    }
    (*ctx).len()
}

/// Returns the suggested `steps` for `fill_blocks` at the batch length,
/// or 0 for null `ctx` or a caught panic. Speed hint only.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_suggested_steps(ctx: *const LustroXofBatch) -> usize {
    guarded(0, || {
        if ctx.is_null() {
            return 0;
        }
        (*ctx).suggested_steps()
    })
}

/// Advances all streams by `steps` stream steps.
/// `out_len` must equal `batch_len * steps * 32`.
/// Output is step-major: the block of `lane` at `step` starts at
/// byte offset `(step * batch_len + lane) * 32`.
///
/// # Safety
/// `ctx` must point to a live `LustroXofBatch` context and be exclusively
/// accessible for the duration of the call.
/// When the expected output length is nonzero, `out` must be valid for
/// that many writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fill_blocks(
    ctx: *mut LustroXofBatch,
    out: *mut u8,
    out_len: usize,
    steps: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
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

        if !fits_slice::<u8>(out_len) {
            return LustroError::InvalidLength;
        }

        let n_blocks = n * steps;
        // SAFETY: `out_len` was validated for `n_blocks * 32` bytes.
        let out_blocks: &mut [[u8; 32]] = match slice_out(out as *mut [u8; 32], n_blocks) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        batch.fill_blocks(out_blocks, steps);
        LustroError::Ok
    })
}

/// Derives one child XOF per lane from `n` child identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// `n` must equal the batch length.
/// Returns null on invalid input.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements
/// when `n > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork(
    ctx: *const LustroXofBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
) -> *mut LustroXofBatch {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let batch = &*ctx;
        if n != batch.len() {
            return std::ptr::null_mut();
        }

        let stream_ids: Vec<StreamId> = if n == 0 {
            Vec::new()
        } else {
            if ids_hi.is_null() || ids_lo.is_null() {
                return std::ptr::null_mut();
            }
            let his = match slice_in(ids_hi, n) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            let los = match slice_in(ids_lo, n) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            his.iter()
                .zip(los.iter())
                .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
                .collect()
        };

        Box::into_raw(Box::new(batch.fork(&stream_ids)))
    })
}

/// Derives `k` children per lane. Output is parent-major:
/// child `j` of lane `i` is at index `i * k + j`.
/// IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
/// Returns null on invalid input, `len() * k` overflow, or allocation failure.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `k` readable elements
/// when `k > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork_many(
    ctx: *const LustroXofBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    k: usize,
) -> *mut LustroXofBatch {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let batch = &*ctx;
        if batch.len().checked_mul(k).is_none() {
            return std::ptr::null_mut();
        }

        let ids: Vec<StreamId> = if k == 0 {
            Vec::new()
        } else {
            if ids_hi.is_null() || ids_lo.is_null() {
                return std::ptr::null_mut();
            }
            let his = match slice_in(ids_hi, k) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            let los = match slice_in(ids_lo, k) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            his.iter()
                .zip(los.iter())
                .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
                .collect()
        };

        match batch.try_fork_many(&ids) {
            Ok(child) => Box::into_raw(Box::new(child)),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// Derives one lane per message by walking each stream along a path
/// of `n_path` identifiers. Messages are passed as parallel pointer/length
/// arrays; path IDs as parallel `(hi, lo)` u64 arrays.
/// `n_path` must be nonzero; `n_messages` may be zero.
/// Returns null on invalid input, including an empty path.
///
/// # Safety
/// `message_ptrs` and `message_lens` must each be valid for `n_messages`
/// elements when `n_messages > 0`. Each `message_ptrs[i]` must be valid for
/// `message_lens[i]` bytes if `message_lens[i] > 0`.
/// `path_hi` and `path_lo` must each be valid for `n_path` elements when
/// `n_path > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_derive_path(
    message_ptrs: *const *const u8,
    message_lens: *const usize,
    n_messages: usize,
    path_hi: *const u64,
    path_lo: *const u64,
    n_path: usize,
) -> *mut LustroXofBatch {
    guarded(std::ptr::null_mut(), || {
        if n_path == 0 || path_hi.is_null() || path_lo.is_null() {
            return std::ptr::null_mut();
        }
        if n_messages > 0 && (message_ptrs.is_null() || message_lens.is_null()) {
            return std::ptr::null_mut();
        }

        let mut messages: Vec<&[u8]> = Vec::with_capacity(n_messages);
        if n_messages > 0 {
            let ptrs = match slice_in(message_ptrs, n_messages) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            let lens = match slice_in(message_lens, n_messages) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            for i in 0..n_messages {
                match buf_in(ptrs[i], lens[i]) {
                    Some(s) => messages.push(s),
                    None => return std::ptr::null_mut(),
                }
            }
        }

        let his = match slice_in(path_hi, n_path) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let los = match slice_in(path_lo, n_path) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let path: Vec<StreamId> = his
            .iter()
            .zip(los.iter())
            .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
            .collect();

        Box::into_raw(Box::new(LustroXofBatch::derive_path(&messages, &path)))
    })
}

/// Derives one child XOF per lane with sequential IDs starting at `first`.
/// IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
/// Returns null on null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork_range(
    ctx: *const LustroXofBatch,
    first_hi: u64,
    first_lo: u64,
) -> *mut LustroXofBatch {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let batch = &*ctx;
        let first = ((first_hi as u128) << 64) | (first_lo as u128);

        Box::into_raw(Box::new(batch.fork_range(StreamId(first))))
    })
}

/// Returns the snapshot size in bytes, or 0 for null `ctx`.
/// Size: `16 + batch_len * 48`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_snapshot_size(ctx: *const LustroXofBatch) -> usize {
    if ctx.is_null() {
        return 0;
    }
    let batch = &*ctx;
    crate::types::batch_snapshot_encoded_len(batch.len()).unwrap_or(0)
}

/// Exports the current batch snapshot.
/// `out_len` must equal `lustro_xof_batch_snapshot_size(ctx)`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `out` must be valid for `out_len` writable bytes and must not overlap
/// the context. `out_len` must equal the expected snapshot size.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_export_snapshot(
    ctx: *const LustroXofBatch,
    out: *mut u8,
    out_len: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
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
            None => return LustroError::InvalidPointer,
        };

        let snapshot = batch.export_snapshot();
        output.copy_from_slice(&snapshot.to_le_bytes());
        LustroError::Ok
    })
}

/// Restores an XOF batch from `len` snapshot bytes.
/// Returns null on invalid input or decoding failure.
///
/// # Safety
/// `bytes` must be valid for `len` bytes if len > 0.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_import_snapshot(
    bytes: *const u8,
    len: usize,
) -> *mut LustroXofBatch {
    guarded(std::ptr::null_mut(), || {
        let snapshot_bytes = match buf_in(bytes, len) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        if let Ok(snapshot) = crate::types::LustroXofBatchSnapshot::from_le_bytes(snapshot_bytes) {
            Box::into_raw(Box::new(LustroXofBatch::import_snapshot(snapshot)))
        } else {
            std::ptr::null_mut()
        }
    })
}
