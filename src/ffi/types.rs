//! Lustro V1 FFI — buffer helpers for C consumers.

use crate::errors::LustroError;
use crate::types::StreamId;

/// Whether `len` elements of `T` span at most isize::MAX bytes
/// (the `slice::from_raw_parts` limit). False also on usize overflow.
#[inline]
pub(super) fn fits_slice<T>(len: usize) -> bool {
    match len.checked_mul(std::mem::size_of::<T>()) {
        Some(bytes) => bytes <= isize::MAX as usize,
        None => false,
    }
}

/// Reconstructs a &[T] from a C pointer + element count.
/// None if ptr is null (len > 0), misaligned, or len * size_of::<T>() > isize::MAX.
/// SAFETY: ptr must be valid for len elements if len > 0.
#[inline]
pub(super) unsafe fn slice_in<'a, T>(ptr: *const T, len: usize) -> Option<&'a [T]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() || !(ptr as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return None;
    }
    if !fits_slice::<T>(len) {
        return None;
    }
    Some(std::slice::from_raw_parts(ptr, len))
}

/// Reconstructs a &mut [T] from a C pointer + element count.
/// Same checks as `slice_in`.
/// SAFETY: ptr must be valid for len elements if len > 0.
#[inline]
pub(super) unsafe fn slice_out<'a, T>(ptr: *mut T, len: usize) -> Option<&'a mut [T]> {
    if len == 0 {
        return Some(&mut []);
    }
    if ptr.is_null() || !(ptr as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return None;
    }
    if !fits_slice::<T>(len) {
        return None;
    }
    Some(std::slice::from_raw_parts_mut(ptr, len))
}

/// `slice_in` for bytes.
/// SAFETY: ptr must be valid for len bytes if len > 0.
#[inline]
pub(super) unsafe fn buf_in<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    slice_in(ptr, len)
}

/// `slice_out` for bytes.
/// SAFETY: ptr must be valid for len bytes if len > 0.
#[inline]
pub(super) unsafe fn buf_out<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    slice_out(ptr, len)
}

/// As `buf_in`, but a length above isize::MAX is `InvalidLength`, as in the hash functions.
/// SAFETY: ptr must be valid for len bytes if len > 0.
#[inline]
pub(super) unsafe fn bytes_in<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], LustroError> {
    if !fits_slice::<u8>(len) {
        return Err(LustroError::InvalidLength);
    }
    buf_in(ptr, len).ok_or(LustroError::InvalidPointer)
}

/// Joins parallel `(hi, lo)` arrays into stream IDs.
/// `InvalidLength` if `n` is not addressable; `InvalidPointer` if a pointer is null (n > 0) or misaligned;
/// `OutOfMemory` if the vector cannot be reserved.
/// SAFETY: both pointers must be valid for `n` elements if n > 0.
pub(super) unsafe fn ids_from_parts(
    hi: *const u64,
    lo: *const u64,
    n: usize,
) -> Result<Vec<StreamId>, LustroError> {
    if !fits_slice::<StreamId>(n) {
        return Err(LustroError::InvalidLength);
    }
    let his = slice_in(hi, n).ok_or(LustroError::InvalidPointer)?;
    let los = slice_in(lo, n).ok_or(LustroError::InvalidPointer)?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(n)
        .map_err(|_| LustroError::OutOfMemory)?;
    ids.extend(
        his.iter()
            .zip(los)
            .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128))),
    );
    Ok(ids)
}

/// Reconstructs `n` messages from parallel pointer/length arrays.
/// `InvalidLength` if `n` or a length is not addressable; `InvalidPointer` on a null or misaligned
/// array, or on a null message pointer with a nonzero length; `OutOfMemory` if the vector
/// cannot be reserved.
/// SAFETY: arrays must be valid for `n` elements if n > 0; each pointer for its length.
pub(super) unsafe fn messages_from_parts<'a>(
    ptrs: *const *const u8,
    lens: *const usize,
    n: usize,
) -> Result<Vec<&'a [u8]>, LustroError> {
    if !fits_slice::<&[u8]>(n) {
        return Err(LustroError::InvalidLength);
    }
    let ptrs = slice_in(ptrs, n).ok_or(LustroError::InvalidPointer)?;
    let lens = slice_in(lens, n).ok_or(LustroError::InvalidPointer)?;
    let mut messages = Vec::new();
    messages
        .try_reserve_exact(n)
        .map_err(|_| LustroError::OutOfMemory)?;
    for (&p, &len) in ptrs.iter().zip(lens) {
        messages.push(bytes_in(p, len)?);
    }
    Ok(messages)
}

/// Checks an array of `n` context pointers.
/// `InvalidLength` if `n` is not addressable; `InvalidPointer` on a null or misaligned array
/// (n > 0) or on a null or misaligned element.
/// SAFETY: the array must be valid for `n` elements if n > 0.
pub(super) unsafe fn ctx_array<'a, T>(
    ptrs: *const *const T,
    n: usize,
) -> Result<&'a [*const T], LustroError> {
    if !fits_slice::<*const T>(n) {
        return Err(LustroError::InvalidLength);
    }
    let ptrs = slice_in(ptrs, n).ok_or(LustroError::InvalidPointer)?;
    if ptrs
        .iter()
        .any(|&p| p.is_null() || !(p as usize).is_multiple_of(std::mem::align_of::<T>()))
    {
        return Err(LustroError::InvalidPointer);
    }
    Ok(ptrs)
}

/// Whether byte ranges [a, a+a_len) and [b, b+b_len) overlap.
/// Some(false)/Some(true) is the answer; None means a range's end overflowed
/// usize: the caller cannot address it, so it is invalid input.
/// Zero-length ranges never overlap.
#[inline]
pub(super) fn ranges_overlap(
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
) -> Option<bool> {
    if a_len == 0 || b_len == 0 {
        return Some(false);
    }
    let a_start = a as usize;
    let b_start = b as usize;
    let a_end = a_start.checked_add(a_len)?;
    let b_end = b_start.checked_add(b_len)?;
    Some(a_start < b_end && b_start < a_end)
}

/// Whether `[out, out + out_len)` is disjoint from the context object `*ctx` itself.
/// Memory that the context owns on the heap (batch lanes) is not covered. Pointers are not
/// validated: callers reject NULL separately.
#[inline]
pub(super) fn disjoint_from_ctx<T>(out: *const u8, out_len: usize, ctx: *const T) -> bool {
    ranges_overlap(out, out_len, ctx as *const u8, std::mem::size_of::<T>()) == Some(false)
}
