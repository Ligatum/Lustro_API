//! Lustro V1 FFI — buffer helpers for C consumers.

/// Safely reconstructs a &[u8] from a C pointer + length.
/// SAFETY: ptr must be valid for len bytes if len > 0.
#[inline]
pub(super) unsafe fn buf_in<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        Some(&[])
    } else if ptr.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(ptr, len))
    }
}

/// Safely reconstructs a &mut [u8] from a C pointer + length.
/// SAFETY: ptr must be valid for len bytes if len > 0.
#[inline]
pub(super) unsafe fn buf_out<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    if len == 0 {
        Some(&mut [])
    } else if ptr.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts_mut(ptr, len))
    }
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
