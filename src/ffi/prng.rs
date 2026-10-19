//! FFI bindings for Lustro PRNG.
//! Each instance is an independent stream.

use crate::errors::LustroError;
use crate::prng::LustroPrng;
use crate::prng::LustroPrngBatch;
use crate::types::{Seed256, StreamId};

use super::types::{buf_in, buf_out, bytes_in, fits_slice, ids_from_parts, slice_out};
use super::{batch_status, construct, derive_path_status, guarded};

// ==========================================
// FFI STREAM SINGLE API
// ==========================================

/// Creates a PRNG context from a 32-byte seed and 128-bit stream ID.
/// `stream_id` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `seed`.
///
/// # Safety
/// If `seed` is non-null, it must be valid for 32 readable bytes.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_new(
    seed: *const u8,
    stream_id_hi: u64,
    stream_id_lo: u64,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        let seed_bytes = buf_in(seed, 32).ok_or(LustroError::InvalidPointer)?;
        let stream_id = ((stream_id_hi as u128) << 64) | (stream_id_lo as u128);
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        Ok(LustroPrng::new(&seed256, StreamId(stream_id)))
    })
}

/// Frees a PRNG context. Passing null is safe (no-op).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context
/// allocated by this API. It must not have been freed already or be in use
/// concurrently.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_free(ctx: *mut LustroPrng) {
    guarded((), || {
        if !ctx.is_null() {
            drop(Box::from_raw(ctx));
        }
    })
}

/// Returns the next 32 random bits.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context and
/// be exclusively accessible for the duration of the call.
/// If `out` is non-null, it must be valid for 4 writable bytes and must not
/// overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_u32(ctx: *mut LustroPrng, out: *mut u32) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out.is_null() {
            return LustroError::InvalidPointer;
        }
        let prng = &mut *ctx;

        let val = prng.next_u32();
        core::ptr::write_unaligned(out, val);
        LustroError::Ok
    })
}

/// Returns the next 64 random bits.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context and
/// be exclusively accessible for the duration of the call.
/// If `out` is non-null, it must be valid for 8 writable bytes and must not
/// overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_u64(ctx: *mut LustroPrng, out: *mut u64) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out.is_null() {
            return LustroError::InvalidPointer;
        }
        let prng = &mut *ctx;

        let val = prng.next_u64();
        core::ptr::write_unaligned(out, val);
        LustroError::Ok
    })
}

/// Returns the next 128 random bits as 16 bytes (LE).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context and
/// be exclusively accessible for the duration of the call.
/// `out` must be valid for 16 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_u128(ctx: *mut LustroPrng, out: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 16) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let prng = &mut *ctx;

        let val = prng.next_u128();
        output.copy_from_slice(&val.to_le_bytes());
        LustroError::Ok
    })
}

/// Writes the next full 32-byte block. Unread bytes of the current block are discarded.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context and
/// be exclusively accessible for the duration of the call.
/// `out` must be valid for 32 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_block(ctx: *mut LustroPrng, out: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 32) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let prng = &mut *ctx;

        let val = prng.next_block();
        output.copy_from_slice(&val);
        LustroError::Ok
    })
}

/// Fills `out` with `out_len` random bytes.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context and
/// be exclusively accessible for the duration of the call.
/// If `out_len > 0`, `out` must be valid for `out_len` writable bytes and
/// must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fill(
    ctx: *mut LustroPrng,
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
        (*ctx).fill_bytes(output);
        LustroError::Ok
    })
}

/// Clones the PRNG context and returns a new independent instance.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_clone(
    ctx: *const LustroPrng,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        Ok((*ctx).clone())
    })
}

/// Derives a child PRNG from the current state and 128-bit identifier.
/// `id` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fork(
    ctx: *const LustroPrng,
    id_hi: u64,
    id_lo: u64,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let id = ((id_hi as u128) << 64) | (id_lo as u128);
        Ok((*ctx).fork(StreamId(id)))
    })
}

/// Derives a PRNG from a 32-byte seed along a path of `n` identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
/// its length is nonzero), `InvalidLength` for `n == 0` or an `n` too large to address.
///
/// # Safety
/// `seed` must be valid for 32 bytes.
/// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_derive_path(
    seed: *const u8,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        let seed_bytes = buf_in(seed, 32).ok_or(LustroError::InvalidPointer)?;
        if n == 0 {
            return Err(LustroError::InvalidLength);
        }
        let path = ids_from_parts(ids_hi, ids_lo, n)?;
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        Ok(LustroPrng::derive_path(&seed256, &path))
    })
}

/// Exports the current PRNG snapshot into `out`.
/// `out` must provide at least 56 writable bytes.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context that
/// is not mutably accessed or freed during the call.
/// `out` must be valid for 56 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_export_snapshot(
    ctx: *const LustroPrng,
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
        let prng = &*ctx;

        let snapshot = prng.export_snapshot();
        output.copy_from_slice(&snapshot.to_le_bytes());
        LustroError::Ok
    })
}

/// Restores a PRNG context from a 56-byte snapshot.
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
pub unsafe extern "C" fn lustro_prng_import_snapshot(
    bytes: *const u8,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        let snapshot_bytes = buf_in(bytes, 56).ok_or(LustroError::InvalidPointer)?;
        let array_ref: &[u8; 56] = snapshot_bytes.try_into().unwrap();
        let snapshot = crate::types::LustroPrngSnapshot::from_le_bytes(array_ref)?;
        Ok(LustroPrng::import_snapshot(snapshot))
    })
}

// ==========================================
// FFI STREAM BATCH API
// ==========================================

/// Creates a PRNG batch from `n` stream identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
/// its length is nonzero), `InvalidLength` for an `n` too large to address.
///
/// # Safety
/// `seed` must be valid for 32 readable bytes.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements
/// when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_new(
    seed: *const u8,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        let seed_bytes = buf_in(seed, 32).ok_or(LustroError::InvalidPointer)?;
        let stream_ids = ids_from_parts(ids_hi, ids_lo, n)?;
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        Ok(LustroPrngBatch::new(&seed256, &stream_ids))
    })
}

/// Creates `count` streams with sequential IDs starting at `first_stream_id`.
/// IDs wrap modulo 2^128. `first_stream_id` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `seed`, `OutOfMemory` if the lane buffer
/// cannot be reserved (allocator refusal or capacity overflow).
///
/// # Safety
/// `seed` must be valid for 32 readable bytes.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_new_range(
    seed: *const u8,
    first_hi: u64,
    first_lo: u64,
    count: usize,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        let seed_bytes = buf_in(seed, 32).ok_or(LustroError::InvalidPointer)?;
        let first_id = ((first_hi as u128) << 64) | (first_lo as u128);
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        LustroPrngBatch::try_new_range(&seed256, StreamId(first_id), count).map_err(batch_status)
    })
}

/// Frees a batch context. Passing null is safe (no-op).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// allocated by this API. It must not have been freed already or be in use
/// concurrently.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_free(ctx: *mut LustroPrngBatch) {
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
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_clone(
    ctx: *const LustroPrngBatch,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        (*ctx).try_clone().map_err(batch_status)
    })
}

/// Copies lane `index` of the batch as a single stream, positioned at the lane's next block.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`, `InvalidArgument` if `index >= len`.
/// The batch is not advanced and later calls on it do not affect the copy.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_lane(
    ctx: *const LustroPrngBatch,
    index: usize,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        (*ctx).lane(index).ok_or(LustroError::InvalidArgument)
    })
}

/// Returns the number of streams, or 0 for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_len(ctx: *const LustroPrngBatch) -> usize {
    if ctx.is_null() {
        return 0;
    }
    (*ctx).len()
}

/// Returns the suggested `steps` for `fill_blocks` at the batch length,
/// or 0 for null `ctx` or a caught panic. Speed hint only.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_suggested_steps(ctx: *const LustroPrngBatch) -> usize {
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
/// `ctx` must point to a live `LustroPrngBatch` context and be exclusively
/// accessible for the duration of the call.
/// When the expected output length is nonzero, `out` must be valid for
/// that many writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fill_blocks(
    ctx: *mut LustroPrngBatch,
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
        // SAFETY: `out_len` was validated for `n * steps * 32` bytes.
        let out_blocks: &mut [[u8; 32]] = match slice_out(out as *mut [u8; 32], n_blocks) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        batch.fill_blocks(out_blocks, steps);
        LustroError::Ok
    })
}

/// Derives one child PRNG per lane from `n` child identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// `n` must equal the batch length.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `n`/`k` is nonzero), `InvalidLength` if `n` differs from the batch length.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements
/// when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork(
    ctx: *const LustroPrngBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroPrngBatch,
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
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `k` readable elements
/// when `k > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork_many(
    ctx: *const LustroPrngBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    k: usize,
    out_ctx: *mut *mut LustroPrngBatch,
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

/// Derives one lane per root by walking each root along a path
/// of `n_path` identifiers.
/// Root and path IDs are passed as parallel `(hi, lo)` u64 arrays.
/// `n_path` must be nonzero; `n_roots` may be zero.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
/// its length is nonzero), `InvalidLength` for `n_path == 0` or a count too large to address,
/// `OutOfMemory` if the lane buffer cannot be reserved.
///
/// # Safety
/// `seed` must be valid for 32 bytes.
/// `roots_hi` and `roots_lo` must each be valid for `n_roots` elements when
/// `n_roots > 0`. `path_hi` and `path_lo` must each be valid for `n_path`
/// elements when `n_path > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_derive_path(
    seed: *const u8,
    roots_hi: *const u64,
    roots_lo: *const u64,
    n_roots: usize,
    path_hi: *const u64,
    path_lo: *const u64,
    n_path: usize,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        let seed_bytes = buf_in(seed, 32).ok_or(LustroError::InvalidPointer)?;
        if n_path == 0 {
            return Err(LustroError::InvalidLength);
        }
        let roots = ids_from_parts(roots_hi, roots_lo, n_roots)?;
        let path = ids_from_parts(path_hi, path_lo, n_path)?;
        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        LustroPrngBatch::try_derive_path(&seed256, &roots, &path).map_err(derive_path_status)
    })
}

/// Derives one child PRNG per lane with sequential IDs starting at `first`.
/// IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork_range(
    ctx: *const LustroPrngBatch,
    first_hi: u64,
    first_lo: u64,
    out_ctx: *mut *mut LustroPrngBatch,
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
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_snapshot_size(ctx: *const LustroPrngBatch) -> usize {
    if ctx.is_null() {
        return 0;
    }
    let batch = &*ctx;
    crate::types::batch_snapshot_encoded_len(batch.len()).unwrap_or(0)
}

/// Exports the current batch snapshot.
/// `out_len` must equal `lustro_prng_batch_snapshot_size(ctx)`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
/// `out` must be valid for `out_len` writable bytes and must not overlap
/// the context. `out_len` must equal the expected snapshot size.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_export_snapshot(
    ctx: *const LustroPrngBatch,
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

/// Restores a PRNG batch from `len` snapshot bytes.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`,
/// `UnsupportedVersion` or `InvalidKind` for a rejected snapshot.
///
/// # Safety
/// `bytes` must be valid for `len` bytes when `len > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_import_snapshot(
    bytes: *const u8,
    len: usize,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        let snapshot_bytes = bytes_in(bytes, len)?;
        let snapshot = crate::types::LustroPrngBatchSnapshot::from_le_bytes(snapshot_bytes)?;
        Ok(LustroPrngBatch::import_snapshot(snapshot))
    })
}
