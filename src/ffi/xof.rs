//! FFI bindings for Lustro XOF.

use crate::errors::LustroError;
use crate::types::StreamId;
use crate::xof::LustroXof;
use crate::xof::LustroXofBatch;

use super::types::{
    buf_in, buf_out, bytes_in, fits_slice, ids_from_parts, messages_from_parts, slice_out,
};
use super::{batch_status, construct, derive_path_status, guarded};

// ==========================================
// FFI XOF SINGLE API
// ==========================================

/// Creates an XOF context by absorbing a message.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `message` when `message_len > 0`,
/// `InvalidLength` for a `message_len` too large to address.
///
/// # Safety
/// If `message_len > 0`, `message` must be valid for `message_len` readable
/// bytes.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_new(
    message: *const u8,
    message_len: usize,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    construct(out_ctx, || {
        let message = bytes_in(message, message_len)?;
        Ok(LustroXof::new(message))
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
///
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
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_clone(
    ctx: *const LustroXof,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        Ok((*ctx).clone())
    })
}

/// Derives a child XOF from the current state and 128-bit identifier.
/// `id` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fork(
    ctx: *const LustroXof,
    id_hi: u64,
    id_lo: u64,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let id = ((id_hi as u128) << 64) | (id_lo as u128);
        Ok((*ctx).fork(StreamId(id)))
    })
}

/// Derives an XOF from a message along a path of `n` identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `message` when `message_len > 0` or a null or
/// misaligned ID array (when `n > 0`), `InvalidLength` for `n == 0` or a length too large to
/// address.
///
/// # Safety
/// `message` must be valid for `message_len` bytes when `message_len > 0`.
/// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_derive_path(
    message: *const u8,
    message_len: usize,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    construct(out_ctx, || {
        let message = bytes_in(message, message_len)?;
        if n == 0 {
            return Err(LustroError::InvalidLength);
        }
        let path = ids_from_parts(ids_hi, ids_lo, n)?;
        Ok(LustroXof::derive_path(message, &path))
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
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `bytes`; `UnsupportedVersion`, `InvalidKind`
/// or `InvalidCursor` for a rejected snapshot. The length is fixed at 56 bytes, so `InvalidLength`
/// is not reported.
///
/// # Safety
/// If `bytes` is non-null, it must be valid for 56 readable bytes.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_import_snapshot(
    bytes: *const u8,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    construct(out_ctx, || {
        let snapshot_bytes = buf_in(bytes, 56).ok_or(LustroError::InvalidPointer)?;
        let array_ref: &[u8; 56] = snapshot_bytes.try_into().unwrap();
        let snapshot = crate::types::LustroXofSnapshot::from_le_bytes(array_ref)?;
        Ok(LustroXof::import_snapshot(snapshot))
    })
}

// ==========================================
// FFI XOF BATCH API
// ==========================================

/// Creates an XOF batch from `n` messages.
/// Messages are passed as parallel pointer/length arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null or misaligned pointer/length array (when
/// `n > 0`) or a null message pointer with a nonzero length, `InvalidLength` for a length too large
/// to address.
///
/// # Safety
/// When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
/// `n` readable elements.
/// Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
/// when `message_lens[i] > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_new(
    message_ptrs: *const *const u8,
    message_lens: *const usize,
    n: usize,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        let messages = messages_from_parts(message_ptrs, message_lens, n)?;
        Ok(LustroXofBatch::new(&messages))
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

/// Clones the batch context and returns a new independent instance.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
/// be reserved.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_clone(
    ctx: *const LustroXofBatch,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        (*ctx).try_clone().map_err(batch_status)
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
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `n`/`k` is nonzero), `InvalidLength` if `n` differs from the batch length.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements
/// when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork(
    ctx: *const LustroXofBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let batch = &*ctx;
        if n != batch.len() {
            return Err(LustroError::InvalidLength);
        }
        let stream_ids = ids_from_parts(ids_hi, ids_lo, n)?;
        Ok(batch.fork(&stream_ids))
    })
}

/// Derives `k` children per lane. Output is parent-major:
/// child `j` of lane `i` is at index `i * k + j`.
/// IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `n`/`k` is nonzero), `InvalidLength` if `len() * k` overflows or `k` is too large to address,
/// `OutOfMemory` if the lane buffer cannot be reserved.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `k` readable elements
/// when `k > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork_many(
    ctx: *const LustroXofBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    k: usize,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let batch = &*ctx;
        if batch.len().checked_mul(k).is_none() {
            return Err(LustroError::InvalidLength);
        }
        let ids = ids_from_parts(ids_hi, ids_lo, k)?;
        batch.try_fork_many(&ids).map_err(batch_status)
    })
}

/// Derives one lane per message by walking each stream along a path
/// of `n_path` identifiers. Messages are passed as parallel pointer/length
/// arrays; path IDs as parallel `(hi, lo)` u64 arrays.
/// `n_path` must be nonzero; `n_messages` may be zero.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null or misaligned argument array (when its length
/// is nonzero) or a null message pointer with a nonzero length, `InvalidLength` for `n_path == 0`
/// or a length too large to address, `OutOfMemory` if the lane buffer cannot be reserved.
///
/// # Safety
/// `message_ptrs` and `message_lens` must each be valid for `n_messages`
/// elements when `n_messages > 0`. Each `message_ptrs[i]` must be valid for
/// `message_lens[i]` bytes if `message_lens[i] > 0`.
/// `path_hi` and `path_lo` must each be valid for `n_path` elements when
/// `n_path > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_derive_path(
    message_ptrs: *const *const u8,
    message_lens: *const usize,
    n_messages: usize,
    path_hi: *const u64,
    path_lo: *const u64,
    n_path: usize,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        if n_path == 0 {
            return Err(LustroError::InvalidLength);
        }
        let messages = messages_from_parts(message_ptrs, message_lens, n_messages)?;
        let path = ids_from_parts(path_hi, path_lo, n_path)?;
        LustroXofBatch::try_derive_path(&messages, &path).map_err(derive_path_status)
    })
}

/// Derives one child XOF per lane with sequential IDs starting at `first`.
/// IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_fork_range(
    ctx: *const LustroXofBatch,
    first_hi: u64,
    first_lo: u64,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let first = ((first_hi as u128) << 64) | (first_lo as u128);
        Ok((*ctx).fork_range(StreamId(first)))
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
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`,
/// `UnsupportedVersion` or `InvalidKind` for a rejected snapshot.
///
/// # Safety
/// `bytes` must be valid for `len` bytes if len > 0.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_import_snapshot(
    bytes: *const u8,
    len: usize,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        let snapshot_bytes = bytes_in(bytes, len)?;
        let snapshot = crate::types::LustroXofBatchSnapshot::from_le_bytes(snapshot_bytes)?;
        Ok(LustroXofBatch::import_snapshot(snapshot))
    })
}
