//! FFI bindings for Lustro Hash.

use super::types::{buf_in, buf_out, fits_slice, ranges_overlap, slice_in, slice_out};
use super::{construct, guarded};
use crate::dispatch::{map_ranges, RawMessages};
use crate::errors::LustroError;
use crate::hash::{hash128, hash256, LustroHasher};

// ==========================================
// FFI HASH API
// ==========================================

// Pointer table shared with the checking threads; it is only read.
struct PtrTable<'a>(&'a [*const u8]);

// SAFETY: the table is only read.
unsafe impl Sync for PtrTable<'_> {}

impl PtrTable<'_> {
    #[inline]
    fn get(&self, i: usize) -> *const u8 {
        self.0[i]
    }
}

// From this many messages the table check runs on the thread pool.
const CHECK_PAR_MIN: usize = 8192;

// Checks every message: length limit, no overlap with the output range, pointer rules.
// Returns the saturating sum of the lengths. The error is the one of the lowest index.
// SAFETY: as `buf_in` for each `ptrs[i]`, `lens[i]`; `ptrs` and `lens` have equal length.
unsafe fn check_var_table(
    ptrs: &[*const u8],
    lens: &[usize],
    out: *const u8,
    total_out: usize,
) -> Result<usize, LustroError> {
    let table = PtrTable(ptrs);
    let out_addr = out as usize;
    let parts = map_ranges(ptrs.len(), CHECK_PAR_MIN, |range| {
        let mut sum = 0usize;
        for i in range {
            let (p, l) = (table.get(i), lens[i]);
            if !fits_slice::<u8>(l) {
                return Err(LustroError::InvalidLength);
            }
            match ranges_overlap(p, l, out_addr as *const u8, total_out) {
                Some(true) => return Err(LustroError::InvalidPointer),
                Some(false) => {}
                None => return Err(LustroError::InvalidLength),
            }
            if buf_in(p, l).is_none() {
                return Err(LustroError::InvalidPointer);
            }
            sum = sum.saturating_add(l);
        }
        Ok(sum)
    });
    let mut total = 0usize;
    for part in parts {
        total = total.saturating_add(part?);
    }
    Ok(total)
}

/// Computes a 256-bit hash into `out`.
///
/// # Safety
/// If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
/// `out` must be valid for 32 writable bytes. The input and output regions
/// must not overlap.
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

/// Computes a 128-bit hash into `out`.
///
/// # Safety
/// If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
/// `out` must be valid for 16 writable bytes. The input and output regions
/// must not overlap.
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
// FFI INCREMENTAL HASH API
// ==========================================

/// Starts a `lustro_hash256` of a message of exactly `total_len` bytes, given in pieces
/// to `lustro_hasher_update`. Stores the new context in `*out_ctx` (null on failure) and
/// returns the status: `InvalidPointer` for a null or misaligned `out_ctx`.
///
/// # Safety
/// If `out_ctx` is non-null, it must be valid for writing one pointer.
#[no_mangle]
pub unsafe extern "C" fn lustro_hasher_new(
    total_len: u64,
    out_ctx: *mut *mut LustroHasher,
) -> LustroError {
    construct(out_ctx, || Ok(LustroHasher::new(total_len)))
}

/// Frees a hasher context. Passing null is safe (no-op).
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroHasher` context
/// allocated by this API. It must not have been freed already or be in use
/// concurrently.
#[no_mangle]
pub unsafe extern "C" fn lustro_hasher_free(ctx: *mut LustroHasher) {
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
/// If `ctx` is non-null, it must point to a live `LustroHasher` context and
/// be exclusively accessible for the duration of the call.
/// If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_hasher_update(
    ctx: *mut LustroHasher,
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
        let ctx_len = std::mem::size_of::<LustroHasher>();
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

/// Writes the 32-byte `lustro_hash256` of the message to `out`. Returns `InvalidLength`
/// while bytes are left (`out` is not written; more `lustro_hasher_update` calls can
/// follow) and `InvalidPointer` for a null `ctx` or `out`, or `out` overlapping the
/// context. The context is not changed: every call writes the same digest.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroHasher` context that
/// is not mutably accessed or freed during the call.
/// If `out` is non-null, it must be valid for 32 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_hasher_finalize256(
    ctx: *const LustroHasher,
    out: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let ctx_len = std::mem::size_of::<LustroHasher>();
        if ranges_overlap(out as *const u8, 32, ctx as *const u8, ctx_len) != Some(false) {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 32) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        match (*ctx).finalize() {
            Ok(d) => {
                output.copy_from_slice(d.as_bytes());
                LustroError::Ok
            }
            Err(e) => e.into(),
        }
    })
}

/// Writes the number of bytes still expected before `lustro_hasher_finalize256` succeeds.
/// Returns `InvalidPointer` for a null `ctx` or `out_remaining`, or `out_remaining`
/// overlapping the context, writing nothing. `out_remaining` need not be aligned.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroHasher` context that
/// is not mutably accessed or freed during the call.
/// If `out_remaining` is non-null, it must be valid for writing one `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn lustro_hasher_remaining(
    ctx: *const LustroHasher,
    out_remaining: *mut u64,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out_remaining.is_null() {
            return LustroError::InvalidPointer;
        }
        let ctx_len = std::mem::size_of::<LustroHasher>();
        if ranges_overlap(out_remaining as *const u8, 8, ctx as *const u8, ctx_len) != Some(false) {
            return LustroError::InvalidPointer;
        }
        core::ptr::write_unaligned(out_remaining, (*ctx).remaining());
        LustroError::Ok
    })
}

/// Clones the hasher context: the copy holds the same absorbed bytes and continues
/// independently. Stores the new context in `*out_ctx` (null on failure) and returns the
/// status: `InvalidPointer` for a null or misaligned `out_ctx`, or for null `ctx`. An
/// `out_ctx` overlapping the context is `InvalidPointer` too, and then `*out_ctx` is not
/// written.
///
/// # Safety
/// If `ctx` is non-null, it must point to a live `LustroHasher` context that
/// is not mutably accessed or freed during the call.
/// If `out_ctx` is non-null, it must be valid for writing one pointer.
#[no_mangle]
pub unsafe extern "C" fn lustro_hasher_clone(
    ctx: *const LustroHasher,
    out_ctx: *mut *mut LustroHasher,
) -> LustroError {
    // `construct` writes NULL to `*out_ctx` first, which would overwrite part of the source.
    if !ctx.is_null() {
        let ctx_len = std::mem::size_of::<LustroHasher>();
        let slot = std::mem::size_of::<*mut LustroHasher>();
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
// FFI HASH BATCH API
// ==========================================

/// Hashes `n` fixed-length messages into `out_ptr`.
///
/// # Safety
/// If `n * message_len` is nonzero and representable, `data_ptr` must be
/// valid for `n * message_len` readable bytes.
/// If `n > 0` and the size calculation succeeds, `out_ptr` must be valid
/// for `n * 32` writable bytes.
/// The input and output regions must not overlap.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash256_many_fixed(
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

        let out_blocks: &mut [[u8; 32]] = match slice_out(out_ptr as *mut [u8; 32], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        crate::api::absorb_hash256_fixed_into(
            data,
            message_len,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}

/// Hashes `n` fixed-length messages into 128-bit digests.
///
/// # Safety
/// If `n * message_len` is nonzero and representable, `data_ptr` must be
/// valid for `n * message_len` readable bytes.
/// If `n > 0` and the size calculation succeeds, `out_ptr` must be valid
/// for `n * 16` writable bytes.
/// The input and output regions must not overlap.
#[no_mangle]
pub unsafe extern "C" fn lustro_hash128_many_fixed(
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

        let out_blocks: &mut [[u8; 16]] = match slice_out(out_ptr as *mut [u8; 16], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        crate::api::absorb_hash128_fixed_into(
            data,
            message_len,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}

/// Hashes `n` variable-length messages.
/// `message_ptrs[i]` must reference `message_lens[i]` bytes.
/// A null pointer is allowed when `message_lens[i] == 0`.
///
/// # Safety
/// When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
/// `n` readable elements.
/// Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
/// when `message_lens[i] > 0`.
/// `out_ptr` must be valid for `n * 32` writable bytes when that size is
/// representable.
/// The output region must not overlap either table or any nonempty message.
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

        let total_bytes =
            match unsafe { check_var_table(ptrs, lens, out_ptr as *const u8, total_out) } {
                Ok(v) => v,
                Err(e) => return e,
            };

        let out_blocks: &mut [[u8; 32]] = match slice_out(out_ptr as *mut [u8; 32], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        // SAFETY: tables and messages were validated above and do not overlap `out`.
        let messages = unsafe { RawMessages::new(ptrs, lens) };
        crate::api::absorb_hash256_var_raw_into(
            messages,
            total_bytes,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}

/// Hashes `n` variable-length messages into 128-bit digests.
/// Same pointer conventions as `lustro_hash256_many_var`.
///
/// # Safety
/// When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
/// `n` readable elements.
/// Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
/// when `message_lens[i] > 0`.
/// `out_ptr` must be valid for `n * 16` writable bytes when that size is
/// representable.
/// The output region must not overlap either table or any nonempty message.
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

        let total_bytes =
            match unsafe { check_var_table(ptrs, lens, out_ptr as *const u8, total_out) } {
                Ok(v) => v,
                Err(e) => return e,
            };

        let out_blocks: &mut [[u8; 16]] = match slice_out(out_ptr as *mut [u8; 16], n) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        // SAFETY: tables and messages were validated above and do not overlap `out`.
        let messages = unsafe { RawMessages::new(ptrs, lens) };
        crate::api::absorb_hash128_var_raw_into(
            messages,
            total_bytes,
            crate::constants::Domain::Hash as u128,
            out_blocks,
        );
        LustroError::Ok
    })
}
