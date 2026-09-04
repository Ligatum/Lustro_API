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