//! Lustro V1 FFI — buffer helpers for C consumers.

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
    if ptr.is_null() || (ptr as usize) % std::mem::align_of::<T>() != 0 {
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
    if ptr.is_null() || (ptr as usize) % std::mem::align_of::<T>() != 0 {
        return None;
    }
    if !fits_slice::<T>(len) {
        return None;
    }
    Some(std::slice::from_raw_parts_mut(ptr, len))
}

/// Safely reconstructs a &[u8] from a C pointer + length.
/// SAFETY: ptr must be valid for len bytes if len > 0.
#[inline]
pub(super) unsafe fn buf_in<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    slice_in(ptr, len)
}

/// Safely reconstructs a &mut [u8] from a C pointer + length.
/// SAFETY: ptr must be valid for len bytes if len > 0.
#[inline]
pub(super) unsafe fn buf_out<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    slice_out(ptr, len)
}

/// Whether byte ranges [a, a+a_len) and [b, b+b_len) overlap.
/// Some(false)/Some(true) is the answer; None means a range's end overflowed
/// usize, so the caller can't even address it — treat as invalid input.
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
