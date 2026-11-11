//! FFI bindings for Lustro XOF.

use crate::errors::LustroError;
use crate::types::{StreamId, LUSTRO_SNAPSHOT_LEN};
use crate::xof::LustroXof;
use crate::xof::LustroXofAbsorber;
use crate::xof::LustroXofBatch;

use super::types::{
    buf_in, buf_out, bytes_in, ctx_array, fits_slice, ids_from_parts, messages_from_parts,
    ranges_overlap, slice_out,
};
use super::{construct, guarded};

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

/// Writes the next 32 bits of the stream to `*out` (`out` need not be aligned).
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

/// Writes the next 64 bits of the stream to `*out` (`out` need not be aligned).
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

/// Writes the next 128 bits of the stream to `out` as 16 bytes (LE). `lustro_u128_to_parts` splits them
/// into `(hi, lo)`.
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

/// Fills `out` with the next `out_len` bytes of the stream.
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

/// Skips `n` bytes: the same state as `lustro_xof_fill` of `n` bytes, without writing them.
/// Costs O(n); there is no jump-ahead.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context and
/// be exclusively accessible for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_advance_bytes(ctx: *mut LustroXof, n: u64) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        (*ctx).advance_bytes(n);
        LustroError::Ok
    })
}

/// Skips `n` blocks: the same state as `n` calls of `lustro_xof_next_block`. Unread bytes of
/// the current block are discarded first (for `n > 0`); `n == 0` changes nothing.
/// Costs O(n); there is no jump-ahead.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context and
/// be exclusively accessible for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_advance_blocks(ctx: *mut LustroXof, n: u64) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        (*ctx).advance_blocks(n);
        LustroError::Ok
    })
}

/// Writes the bytes taken from the start of the stream as 16 bytes (LE); see
/// `lustro_u128_to_parts`. Bytes discarded by `lustro_xof_next_block` and
/// `lustro_xof_advance_blocks` count as taken. A fresh stream and a fork start at 0;
/// an imported snapshot continues from its own position. The value is meaningless after
/// 2^64 blocks, when the block counter wraps.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// `out` must be valid for 16 writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_position(ctx: *const LustroXof, out: *mut u8) -> LustroError {
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

/// Clones the XOF context and returns a new independent instance.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
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
/// The read position inside the current block does not affect the child.
/// `id` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
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

/// Derives one child XOF per identifier, as a batch: lane `i` is `lustro_xof_fork` with
/// `(ids_hi[i], ids_lo[i])`. The parent is not changed.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `n > 0`), `InvalidLength` for an `n` too large to address, `OutOfMemory` if the identifiers or
/// the lane buffer cannot be allocated.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fork_batch(
    ctx: *const LustroXof,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        let ids = ids_from_parts(ids_hi, ids_lo, n)?;
        (*ctx).try_fork_batch(&ids).map_err(LustroError::from)
    })
}

/// As `lustro_xof_fork_batch` with `count` sequential IDs starting at `first`, wrapping
/// modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
/// be reserved.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fork_batch_range(
    ctx: *const LustroXof,
    first_hi: u64,
    first_lo: u64,
    count: usize,
    out_ctx: *mut *mut LustroXofBatch,
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

/// Derives an XOF from the current state along a path of `n` identifiers: the same as
/// `lustro_xof_fork` for each ID in turn. IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
/// `n > 0`), `InvalidLength` for `n == 0` or an `n` too large to address, `OutOfMemory` if the
/// identifier array cannot be allocated.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_fork_path(
    ctx: *const LustroXof,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
    out_ctx: *mut *mut LustroXof,
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

/// Derives an XOF from a message along a path of `n` identifiers.
/// IDs are passed as parallel `(hi, lo)` u64 arrays.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `message` when `message_len > 0` or a null or
/// misaligned ID array (when `n > 0`), `InvalidLength` for `n == 0` or a length too large to
/// address, `OutOfMemory` if the identifier array cannot be allocated.
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
/// `out_len` must be 56 (`LUSTRO_SNAPSHOT_LEN`), otherwise `InvalidLength`.
/// The snapshot holds the full stream state: whoever has it can reproduce the rest of the
/// stream and its forks. Keep it as private as the message.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXof` context that
/// is not mutably accessed or freed during the call.
/// `out` must be valid for `out_len` writable bytes and must not overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_export_snapshot(
    ctx: *const LustroXof,
    out: *mut u8,
    out_len: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        if out_len != LUSTRO_SNAPSHOT_LEN {
            return LustroError::InvalidLength;
        }
        let output = match buf_out(out, LUSTRO_SNAPSHOT_LEN) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let xof = &*ctx;

        let snapshot = xof.export_snapshot();
        output.copy_from_slice(&snapshot.to_le_bytes());
        LustroError::Ok
    })
}

/// Restores an XOF context from `len` snapshot bytes.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`
/// when `len` is not 56 (`LUSTRO_SNAPSHOT_LEN`); `UnsupportedVersion`, `InvalidKind`,
/// `InvalidReserved` or `InvalidCursor` for a rejected snapshot.
///
/// # Safety
/// `bytes` must be valid for `len` readable bytes when `len > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_import_snapshot(
    bytes: *const u8,
    len: usize,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    construct(out_ctx, || {
        let snapshot_bytes = bytes_in(bytes, len)?;
        let snapshot = crate::types::LustroXofSnapshot::try_from(snapshot_bytes)?;
        Ok(LustroXof::import_snapshot(snapshot))
    })
}

// ==========================================
// FFI INCREMENTAL XOF API
// ==========================================

/// Starts a `lustro_xof_new` of a message of exactly `total_len` bytes, given in pieces
/// to `lustro_xof_absorber_update`. Stores the new context in `*out_ctx` (null on failure)
/// and returns the status: `InvalidPointer` for a null or misaligned `out_ctx`.
///
/// # Safety
/// If `out_ctx` is non-null, it must be valid for writing one pointer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_absorber_new(
    total_len: u64,
    out_ctx: *mut *mut LustroXofAbsorber,
) -> LustroError {
    construct(out_ctx, || Ok(LustroXofAbsorber::new(total_len)))
}

/// Frees an absorber context. Passing null is safe (no-op).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context
/// allocated by this API. It must not have been freed already or be in use
/// concurrently.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_absorber_free(ctx: *mut LustroXofAbsorber) {
    guarded((), || {
        if !ctx.is_null() {
            drop(Box::from_raw(ctx));
        }
    })
}

/// Absorbs the next `data_len` bytes of the message. Returns `InvalidLength` and absorbs
/// nothing if fewer than `data_len` bytes are left (or `data_len` exceeds isize::MAX);
/// `InvalidPointer` for a null `ctx`, a null `data` with `data_len > 0`, or `data`
/// overlapping the context.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context and
/// be exclusively accessible for the duration of the call.
/// If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_absorber_update(
    ctx: *mut LustroXofAbsorber,
    data: *const u8,
    data_len: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        if !fits_slice::<u8>(data_len) {
            return LustroError::InvalidLength;
        }
        let ctx_len = std::mem::size_of::<LustroXofAbsorber>();
        match ranges_overlap(data, data_len, ctx as *const u8, ctx_len) {
            Some(true) => return LustroError::InvalidPointer,
            Some(false) => {}
            None => return LustroError::InvalidLength,
        }
        let input = match buf_in(data, data_len) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        match (*ctx).update(input) {
            Ok(()) => LustroError::Ok,
            Err(e) => e.into(),
        }
    })
}

/// Creates the XOF context `lustro_xof_new` would give for the whole message, at the start
/// of the stream; free it with `lustro_xof_free`. Stores it in `*out_ctx` (null on failure)
/// and returns the status: `InvalidLength` while bytes are left (more
/// `lustro_xof_absorber_update` calls can follow), `InvalidPointer` for a null or
/// misaligned `out_ctx`, or for null `ctx`. An `out_ctx` overlapping the context is
/// `InvalidPointer` too, and then `*out_ctx` is not written. The absorber is not changed:
/// every call gives the same stream.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_absorber_finalize(
    ctx: *const LustroXofAbsorber,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    // `construct` writes NULL to `*out_ctx` first, which would overwrite part of the absorber.
    if !ctx.is_null() {
        let ctx_len = std::mem::size_of::<LustroXofAbsorber>();
        let slot = std::mem::size_of::<*mut LustroXof>();
        if ranges_overlap(out_ctx as *const u8, slot, ctx as *const u8, ctx_len) != Some(false) {
            return LustroError::InvalidPointer;
        }
    }
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        (*ctx).finalize().map_err(LustroError::from)
    })
}

/// Writes the number of bytes still expected before `lustro_xof_absorber_finalize`
/// succeeds. Returns `InvalidPointer` for a null `ctx` or `out_remaining`, or
/// `out_remaining` overlapping the context, writing nothing. `out_remaining` need not be
/// aligned.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context that
/// is not mutably accessed or freed during the call.
/// If `out_remaining` is non-null, it must be valid for writing one `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_absorber_remaining(
    ctx: *const LustroXofAbsorber,
    out_remaining: *mut u64,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out_remaining.is_null() {
            return LustroError::InvalidPointer;
        }
        let ctx_len = std::mem::size_of::<LustroXofAbsorber>();
        if ranges_overlap(out_remaining as *const u8, 8, ctx as *const u8, ctx_len) != Some(false) {
            return LustroError::InvalidPointer;
        }
        core::ptr::write_unaligned(out_remaining, (*ctx).remaining());
        LustroError::Ok
    })
}

/// Clones the absorber context: the copy holds the same absorbed bytes and continues
/// independently. Stores the new context in `*out_ctx` (null on failure) and returns the
/// status: `InvalidPointer` for a null or misaligned `out_ctx`, or for null `ctx`. An
/// `out_ctx` overlapping the context is `InvalidPointer` too, and then `*out_ctx` is not
/// written.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_absorber_clone(
    ctx: *const LustroXofAbsorber,
    out_ctx: *mut *mut LustroXofAbsorber,
) -> LustroError {
    // `construct` writes NULL to `*out_ctx` first, which would overwrite part of the source.
    if !ctx.is_null() {
        let ctx_len = std::mem::size_of::<LustroXofAbsorber>();
        let slot = std::mem::size_of::<*mut LustroXofAbsorber>();
        if ranges_overlap(out_ctx as *const u8, slot, ctx as *const u8, ctx_len) != Some(false) {
            return LustroError::InvalidPointer;
        }
    }
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        Ok((*ctx).clone())
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
/// to address, `OutOfMemory` if the message table, the lane buffer or the digest buffer cannot be
/// allocated.
/// Fixes the runtime settings (see `lustro_configure`) when `n > 0`.
///
/// # Safety
/// When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
/// `n` readable elements.
/// Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
/// when `message_lens[i] > 0`.
/// The tables and the message bytes must stay valid and unmodified by any thread until the
/// function returns; they are read on the thread pool.
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
        LustroXofBatch::try_new(&messages).map_err(LustroError::from)
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
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_clone(
    ctx: *const LustroXofBatch,
    out_ctx: *mut *mut LustroXofBatch,
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
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_lane(
    ctx: *const LustroXofBatch,
    index: usize,
    out_ctx: *mut *mut LustroXof,
) -> LustroError {
    construct(out_ctx, || {
        if ctx.is_null() {
            return Err(LustroError::InvalidPointer);
        }
        (*ctx).lane(index).ok_or(LustroError::InvalidArgument)
    })
}

/// Creates an XOF batch with one lane per stream, in order. Lane `i` continues
/// `streams[i]` at its next block; the streams are not changed.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null or misaligned `streams` array (when `n > 0`)
/// or element, `InvalidLength` for an `n` too large to address, `InvalidArgument` if a stream is
/// inside a block (part of its current block unread), `OutOfMemory` if the lane buffer cannot be
/// reserved.
///
/// # Safety
/// When `n > 0`, `streams` must be valid for `n` readable elements, each pointing to a live
/// `LustroXof` context that is not mutably accessed or freed during the call. The same context
/// may appear more than once.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of the contexts.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_from_streams(
    streams: *const *const LustroXof,
    n: usize,
    out_ctx: *mut *mut LustroXofBatch,
) -> LustroError {
    construct(out_ctx, || {
        let streams = ctx_array(streams, n)?;
        LustroXofBatch::try_from_stream_refs(streams.iter().map(|&p| &*p))
            .map_err(LustroError::from)
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

/// Writes the number of streams to `*out_len`. Unlike `lustro_xof_batch_len`, a null `ctx`
/// is reported (`InvalidPointer`, also for a null `out_len`) instead of reading as an empty
/// batch. Nothing is written on error.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_len` is non-null, it must be valid for writing one `uintptr_t` and must not
/// overlap the context.
#[no_mangle]
pub unsafe extern "C" fn lustro_xof_batch_get_len(
    ctx: *const LustroXofBatch,
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
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context and be exclusively
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
/// `n` is nonzero), `InvalidLength` if `n` differs from the batch length, `OutOfMemory` if the
/// identifiers or the lane buffer cannot be allocated.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `n` readable elements
/// when `n > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
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
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `ids_hi` and `ids_lo` must each be valid for `k` readable elements
/// when `k > 0`.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
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
        batch.try_fork_many(&ids).map_err(LustroError::from)
    })
}

/// Derives one lane per message by walking each stream along a path
/// of `n_path` identifiers. Messages are passed as parallel pointer/length
/// arrays; path IDs as parallel `(hi, lo)` u64 arrays.
/// `n_path` must be nonzero; `n_messages` may be zero.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null or misaligned argument array (when its length
/// is nonzero) or a null message pointer with a nonzero length, `InvalidLength` for `n_path == 0`
/// or a length too large to address, `OutOfMemory` if the input tables or the lane buffer cannot
/// be reserved.
///
/// # Safety
/// `message_ptrs` and `message_lens` must each be valid for `n_messages`
/// elements when `n_messages > 0`. Each `message_ptrs[i]` must be valid for
/// `message_lens[i]` bytes if `message_lens[i] > 0`.
/// `path_hi` and `path_lo` must each be valid for `n_path` elements when
/// `n_path > 0`.
/// The tables and the message bytes must stay valid and unmodified by any thread until the
/// function returns.
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
        LustroXofBatch::try_derive_path(&messages, &path).map_err(LustroError::from)
    })
}

/// Derives one child XOF per lane with sequential IDs starting at `first`.
/// IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
/// be reserved.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer
/// and must not overlap any input buffer or the memory of `ctx`.
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
        (*ctx)
            .try_fork_range(StreamId(first))
            .map_err(LustroError::from)
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
/// `out_len` must equal `lustro_xof_batch_snapshot_size(ctx)`, otherwise `InvalidLength`.
/// Writes straight into `out` and allocates nothing, so there is no `OutOfMemory`.
/// The snapshot holds every lane's state: whoever has it can reproduce the rest of each
/// stream and its forks. Keep it as private as the messages.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroXofBatch` context
/// that is not mutably accessed or freed during the call.
/// `out` must be valid for `out_len` writable bytes and must not overlap
/// the context.
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

        batch.write_snapshot_into(output);
        LustroError::Ok
    })
}

/// Restores an XOF batch from `len` snapshot bytes.
/// Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
/// for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`,
/// `UnsupportedVersion`, `InvalidKind` or `InvalidReserved` for a rejected snapshot;
/// `OutOfMemory` if the lane buffer cannot be allocated.
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
        let snapshot = crate::types::LustroXofBatchSnapshot::try_from_le_bytes(snapshot_bytes)?;
        Ok(LustroXofBatch::import_snapshot(snapshot))
    })
}
