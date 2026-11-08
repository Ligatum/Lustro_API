//! FFI bindings for Lustro PRNG.
//! Each instance is an independent stream.

use crate::errors::LustroError;
use crate::prng::LustroPrng;
use crate::prng::LustroPrngBatch;
use crate::types::{Seed256, StreamId};

use super::types::{buf_in, buf_out, bytes_in, ctx_array, fits_slice, ids_from_parts, slice_out};
use super::{construct, guarded};

// ==========================================
// FFI STREAM SINGLE API
// ==========================================

/// Creates a PRNG context from a 32-byte seed and 128-bit stream ID.
/// `stream_id` is passed as `(hi, lo)` u64 values, `hi` being the upper 64 bits.
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

/// Fills `out_seed` with 32 bytes from the operating system's entropy source.
/// Returns `InvalidPointer` for a null `out_seed`. If the source is unavailable it returns
/// `EntropyUnavailable` and `out_seed` is not written.
///
/// # Safety
/// If `out_seed` is non-null, it must be valid for 32 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_seed_from_entropy(out_seed: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        let Some(out) = buf_out(out_seed, 32) else {
            return LustroError::InvalidPointer;
        };
        match Seed256::from_entropy() {
            Ok(seed) => {
                out.copy_from_slice(seed.as_ref());
                LustroError::Ok
            }
            Err(_) => LustroError::EntropyUnavailable,
        }
    })
}

/// As `lustro_prng_new`, with the seed taken from the operating system's entropy source.
/// Stores the new context in `*out_ctx` (null on failure). Returns `InvalidPointer` for a
/// null or misaligned `out_ctx`, `EntropyUnavailable` if the entropy source is unavailable.
///
/// # Safety
/// If `out_ctx` is non-null, it must be valid for writing one pointer.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_from_entropy(
    stream_id_hi: u64,
    stream_id_lo: u64,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        let stream_id = ((stream_id_hi as u128) << 64) | (stream_id_lo as u128);
        let seed = Seed256::from_entropy().map_err(|_| LustroError::EntropyUnavailable)?;
        Ok(LustroPrng::new(&seed, StreamId(stream_id)))
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

/// Writes the next 32 bits of the stream to `*out` (`out` need not be aligned).
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

/// Writes the next 64 bits of the stream to `*out` (`out` need not be aligned).
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

/// Writes the next 128 bits of the stream to `out` as 16 bytes (LE). `lustro_u128_to_parts` splits them
/// into `(hi, lo)`.
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

/// Skips `n` bytes: the same state as `lustro_prng_fill` of `n` bytes, without writing them.
/// Costs O(n); there is no jump-ahead.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context and
/// be exclusively accessible for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_advance_bytes(ctx: *mut LustroPrng, n: u64) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        (*ctx).advance_bytes(n);
        LustroError::Ok
    })
}

/// Skips `n` blocks: the same state as `n` calls of `lustro_prng_next_block`. Unread bytes of
/// the current block are discarded first (for `n > 0`); `n == 0` changes nothing.
/// Costs O(n); there is no jump-ahead.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context and
/// be exclusively accessible for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_advance_blocks(ctx: *mut LustroPrng, n: u64) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        (*ctx).advance_blocks(n);
        LustroError::Ok
    })
}

/// Writes the bytes taken from the start of the stream as 16 bytes (LE); see
/// `lustro_u128_to_parts`. Bytes discarded by `lustro_prng_next_block` and
/// `lustro_prng_advance_blocks` count as taken. A fresh stream and a fork start at 0;
/// an imported snapshot continues from its own position. The value is meaningless after
/// 2^64 blocks, when the block counter wraps.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context that
/// is not mutably accessed or freed during the call.
/// `out` must be valid for 16 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_position(ctx: *const LustroPrng, out: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 16) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        output.copy_from_slice(&(*ctx).position().to_le_bytes());
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
/// The read position inside the current block does not affect the child.
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

/// Derives one child PRNG per identifier, as a batch: lane `i` is `lustro_prng_fork` with
/// `(ids_hi[i], ids_lo[i])`. The parent is not changed.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `n > 0`), `InvalidLength` for an `n` too large to address, `OutOfMemory` if the identifiers or
/// the lane buffer cannot be allocated.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context that
/// is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fork_batch(
    ctx: *const LustroPrng,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let ids = ids_from_parts(ids_hi, ids_lo, n)?;
        (*ctx).try_fork_batch(&ids).map_err(LustroError::from)
    })
}

/// As `lustro_prng_fork_batch` with `count` sequential IDs starting at `first`, wrapping
/// modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
/// be reserved.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fork_batch_range(
    ctx: *const LustroPrng,
    first_hi: u64,
    first_lo: u64,
    count: usize,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let first = ((first_hi as u128) << 64) | (first_lo as u128);
        (*ctx)
            .try_fork_batch_range(StreamId(first), count)
            .map_err(LustroError::from)
    })
}

/// Derives a PRNG from the current state along a path of `n` identifiers: the same as
/// `lustro_prng_fork` for each ID in turn. IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `n > 0`), `InvalidLength` for `n == 0` or an `n` too large to address, `OutOfMemory` if the
/// identifier array cannot be allocated.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrng` context that
/// is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fork_path(
    ctx: *const LustroPrng,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroPrng,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        if n == 0 {
            return Err(LustroError::InvalidLength);
        }
        let path = ids_from_parts(ids_hi, ids_lo, n)?;
        Ok((*ctx).fork_path(&path))
    })
}

/// Derives a PRNG from a 32-byte seed along a path of `n` identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
/// its length is nonzero), `InvalidLength` for `n == 0` or an `n` too large to address,
/// `OutOfMemory` if the identifier array cannot be allocated.
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
/// The snapshot holds the full stream state: whoever has it can reproduce the rest of the
/// stream and its forks. Keep it as private as the seed.
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
/// its length is nonzero), `InvalidLength` for an `n` too large to address, `OutOfMemory` if the
/// identifiers or the lane buffer cannot be allocated.
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
        LustroPrngBatch::try_new(&seed256, &stream_ids).map_err(LustroError::from)
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
        LustroPrngBatch::try_new_range(&seed256, StreamId(first_id), count)
            .map_err(LustroError::from)
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
        (*ctx).try_clone().map_err(LustroError::from)
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

/// Creates a PRNG batch with one lane per stream, in order. Lane `i` continues
/// `streams[i]` at its next block; the streams are not changed.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null or misaligned `streams` array (when `n > 0`)
/// or element, `InvalidLength` for an `n` too large to address, `InvalidArgument` if a stream is
/// inside a block (part of its current block unread), `OutOfMemory` if the lane buffer cannot be
/// reserved.
///
/// # Safety
/// When `n > 0`, `streams` must be valid for `n` readable elements, each pointing to a live
/// `LustroPrng` context that is not mutably accessed or freed during the call. The same context
/// may appear more than once.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of the contexts.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_from_streams(
    streams: *const *const LustroPrng,
    n: usize,
    out_ctx: *mut *mut LustroPrngBatch,
) -> LustroError {
    construct(out_ctx, || {
        let streams = ctx_array(streams, n)?;
        LustroPrngBatch::try_from_stream_refs(streams.iter().map(|&p| &*p))
            .map_err(LustroError::from)
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

/// Writes the number of streams to `*out_len`. Unlike `lustro_prng_batch_len`, a null `ctx`
/// is reported (`InvalidPointer`, also for a null `out_len`) instead of reading as an empty
/// batch. Nothing is written on error.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_len` is non-null, it must be valid for writing one `uintptr_t` and must not
/// overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_get_len(
    ctx: *const LustroPrngBatch,
    out_len: *mut usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out_len.is_null() {
            return LustroError::InvalidPointer;
        }
        core::ptr::write_unaligned(out_len, (*ctx).len());
        LustroError::Ok
    })
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
/// If `ctx` is non-null, it must point to a live `LustroPrngBatch` context and be exclusively
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
/// `n` is nonzero), `InvalidLength` if `n` differs from the batch length, `OutOfMemory` if the
/// identifiers or the lane buffer cannot be allocated.
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
        batch.try_fork(&stream_ids).map_err(LustroError::from)
    })
}

/// Derives `k` children per lane. Output is parent-major:
/// child `j` of lane `i` is at index `i * k + j`.
/// IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `k` is nonzero), `InvalidLength` if `len() * k` overflows or `k` is too large to address,
/// `OutOfMemory` if the identifiers or the lane buffer cannot be reserved.
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
        batch.try_fork_many(&ids).map_err(LustroError::from)
    })
}

/// Derives one lane per root by walking each root along a path
/// of `n_path` identifiers.
/// Root and path IDs are passed as parallel `(hi, lo)` u64 arrays.
/// `n_path` must be nonzero; `n_roots` may be zero.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
/// its length is nonzero), `InvalidLength` for `n_path == 0` or a count too large to address,
/// `OutOfMemory` if the identifiers or the lane buffer cannot be reserved.
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
        LustroPrngBatch::try_derive_path(&seed256, &roots, &path).map_err(LustroError::from)
    })
}

/// Derives one child PRNG per lane with sequential IDs starting at `first`.
/// IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
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
        (*ctx)
            .try_fork_range(StreamId(first))
            .map_err(LustroError::from)
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
/// The snapshot holds every lane's state: whoever has it can reproduce the rest of each
/// stream and its forks. Keep it as private as the seed.
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
