//! Lustro V1 FFI — C-compatible interface.
//! All functions are #[no_mangle] extern "C".
//! Stateful modules use opaque pointers managed by Rust allocator.

pub mod hash;
pub mod prng;
pub mod types;
pub mod xof;

use std::ffi::c_char;
use std::panic::{catch_unwind, UnwindSafe};

// Panic guard for exported fns: returns `on_panic` if `f` unwinds.
// Nothing is rolled back.
pub(crate) fn guarded<R>(on_panic: R, f: impl FnOnce() -> R + UnwindSafe) -> R {
    catch_unwind(f).unwrap_or(on_panic)
}

// `catch_unwind` cannot catch aborting panics.
#[cfg(panic = "abort")]
compile_error!("lustro FFI requires panic = \"unwind\"");

#[no_mangle]
pub extern "C" fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

/// Returns a static NUL-terminated description of an error code.
/// Unknown codes give "unknown error code". Never NULL; do not free.
#[no_mangle]
pub extern "C" fn lustro_strerror(code: i32) -> *const c_char {
    match crate::errors::LustroError::from_code(code) {
        Some(e) => e.as_nul_str().as_ptr() as *const c_char,
        None => b"unknown error code\0".as_ptr() as *const c_char,
    }
}
