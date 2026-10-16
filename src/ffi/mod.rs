//! Lustro V1 FFI — C-compatible interface.
//! All functions are #[no_mangle] extern "C".
//! Stateful modules use opaque pointers managed by Rust allocator.

pub mod hash;
pub mod prng;
pub mod types;
pub mod xof;

use std::ffi::c_char;
use std::panic::{catch_unwind, UnwindSafe};

use crate::errors::LustroError;
use crate::types::{BatchError, DerivePathError};

// Panic guard for exported fns: returns `on_panic` if `f` unwinds.
// Nothing is rolled back.
pub(crate) fn guarded<R>(on_panic: R, f: impl FnOnce() -> R + UnwindSafe) -> R {
    catch_unwind(f).unwrap_or(on_panic)
}

// Guard for constructors: stores the boxed value in `*out_ctx` and returns the status.
// `*out_ctx` is null on failure. A null or misaligned `out_ctx` is `InvalidPointer`.
pub(crate) unsafe fn construct<T>(
    out_ctx: *mut *mut T,
    f: impl FnOnce() -> Result<T, LustroError> + UnwindSafe,
) -> LustroError {
    if out_ctx.is_null() || (out_ctx as usize) % std::mem::align_of::<*mut T>() != 0 {
        return LustroError::InvalidPointer;
    }
    *out_ctx = std::ptr::null_mut();
    match catch_unwind(f) {
        Ok(Ok(v)) => {
            *out_ctx = Box::into_raw(Box::new(v));
            LustroError::Ok
        }
        Ok(Err(e)) => e,
        Err(_) => LustroError::InternalPanic,
    }
}

pub(crate) fn batch_status(e: BatchError) -> LustroError {
    match e {
        BatchError::SizeOverflow => LustroError::InvalidLength,
        BatchError::Reserve(_) => LustroError::OutOfMemory,
    }
}

pub(crate) fn derive_path_status(e: DerivePathError) -> LustroError {
    match e {
        DerivePathError::EmptyPath => LustroError::InvalidLength,
        DerivePathError::Reserve(_) => LustroError::OutOfMemory,
    }
}

// `catch_unwind` cannot catch aborting panics.
#[cfg(panic = "abort")]
compile_error!("lustro FFI requires panic = \"unwind\"");

#[no_mangle]
pub extern "C" fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

/// Returns the configured number of threads for parallel batches, 0 if batches
/// run on the calling thread. The pool is not started by this call.
#[no_mangle]
pub extern "C" fn lustro_runtime_threads() -> usize {
    guarded(0, || crate::api::runtime_info().threads)
}

/// Returns the stream backend as a static NUL-terminated string ("scalar" or
/// "avx2"). Never NULL; do not free.
#[no_mangle]
pub extern "C" fn lustro_runtime_backend() -> *const c_char {
    guarded(c"unknown".as_ptr(), || {
        match crate::api::runtime_info().backend {
            "avx2" => c"avx2".as_ptr(),
            "scalar" => c"scalar".as_ptr(),
            _ => c"unknown".as_ptr(),
        }
    })
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
