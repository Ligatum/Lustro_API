//! Lustro V1 FFI — C-compatible interface.
//! Exported functions are #[no_mangle] extern "C"; contexts are opaque pointers to values
//! allocated by Rust.

pub mod hash;
pub mod prng;
pub mod types;
pub mod xof;

use std::ffi::c_char;
use std::num::NonZeroUsize;
use std::panic::{catch_unwind, UnwindSafe};

use crate::errors::LustroError;
use crate::types::RuntimeOptions;

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
    if out_ctx.is_null() || !(out_ctx as usize).is_multiple_of(std::mem::align_of::<*mut T>()) {
        return LustroError::InvalidPointer;
    }
    // `*out_ctx` is written only after `f` has finished reading its inputs, so an `out_ctx`
    // that overlaps one of them cannot change what `f` sees.
    match catch_unwind(f) {
        Ok(Ok(v)) => {
            *out_ctx = Box::into_raw(Box::new(v));
            LustroError::Ok
        }
        Ok(Err(e)) => {
            *out_ctx = std::ptr::null_mut();
            e
        }
        Err(_) => {
            *out_ctx = std::ptr::null_mut();
            LustroError::InternalPanic
        }
    }
}

// `construct` for a constructor with a source context. An `out_ctx` overlapping `*ctx` is
// `InvalidPointer` and `*out_ctx` is not written. A null `ctx` is left to `f`.
pub(crate) unsafe fn construct_from<S, T>(
    ctx: *const S,
    out_ctx: *mut *mut T,
    f: impl FnOnce() -> Result<T, LustroError> + UnwindSafe,
) -> LustroError {
    if !ctx.is_null() && !slot_clear_of(out_ctx, ctx) {
        return LustroError::InvalidPointer;
    }
    construct(out_ctx, f)
}

// As `construct_from` for `*_batch_from_streams`: every context in `streams` is checked.
// A bad table is left to `f`.
pub(crate) unsafe fn construct_from_streams<S, T>(
    streams: *const *const S,
    n: usize,
    out_ctx: *mut *mut T,
    f: impl FnOnce() -> Result<T, LustroError> + UnwindSafe,
) -> LustroError {
    if let Some(table) = types::slice_in(streams, n) {
        if table
            .iter()
            .any(|&p| !p.is_null() && !slot_clear_of(out_ctx, p))
        {
            return LustroError::InvalidPointer;
        }
    }
    construct(out_ctx, f)
}

// Whether the `out_ctx` slot is disjoint from the context object `*ctx` itself.
unsafe fn slot_clear_of<S, T>(out_ctx: *mut *mut T, ctx: *const S) -> bool {
    types::ranges_overlap(
        out_ctx as *const u8,
        std::mem::size_of::<*mut T>(),
        ctx as *const u8,
        std::mem::size_of::<S>(),
    ) == Some(false)
}

// `catch_unwind` cannot catch aborting panics.
#[cfg(panic = "abort")]
compile_error!("lustro FFI requires panic = \"unwind\"");

/// Returns the native API version. It does not follow the crate version or the snapshot
/// format version.
#[no_mangle]
pub extern "C" fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

/// Returns the configured pool size for parallel batches, not the number of active workers.
/// `0` means parallelism is disabled by settings or the crate was built without `parallel`.
/// This call does not start the pool and fixes nothing, like `lustro_runtime_info`. If
/// initialization fails, or this process inherited a pool-owner PID from its parent
/// through `fork`, batches fall back to the calling thread. Returns `0` also after a
/// caught internal panic.
#[no_mangle]
pub extern "C" fn lustro_runtime_threads() -> usize {
    guarded(0, || crate::api::runtime_info().threads)
}

/// Returns the backend for batches of streams and hashes as a static NUL-terminated
/// string ("scalar" or "avx2"). Never NULL; do not free. Fixes nothing, like
/// `lustro_runtime_info`. Returns "unknown" after a caught internal panic.
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

/// Execution settings of this process, as returned by `lustro_runtime_info`. Same fields as
/// `RuntimeInfo` in Rust. Flags are 0 or 1. The caller passes `sizeof` of this struct to
/// that function, so that fields added later do not overflow a struct built from an older
/// header.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LustroRuntimeInfo {
    /// Configured pool size, not a count of active workers. 0 = batches run on the
    /// calling thread.
    pub threads: usize,
    /// Logical CPUs available to the process, detected at the call.
    pub logical_cpus: usize,
    /// Backend for batches: "scalar" or "avx2". Static NUL-terminated string, never NULL,
    /// do not free.
    pub backend: *const c_char,
    /// 1 if the settings are fixed (by `lustro_configure` or a batch call); 0 before that.
    pub fixed: u8,
    /// 1 if built with the `parallel` feature.
    pub parallel_built: u8,
    /// 1 if the CPU has the SIMD instructions the crate uses (AVX2 today).
    pub simd_available: u8,
    /// 1 if the pool is built and this process can use it.
    pub pool_usable: u8,
}

// End of the last field of the first layout: the least `out_size` that is accepted.
const RUNTIME_INFO_MIN_SIZE: usize = std::mem::offset_of!(LustroRuntimeInfo, pool_usable) + 1;

fn info_to_c(info: crate::types::RuntimeInfo) -> LustroRuntimeInfo {
    LustroRuntimeInfo {
        threads: info.threads,
        logical_cpus: info.logical_cpus,
        backend: match info.backend {
            "avx2" => c"avx2".as_ptr(),
            "scalar" => c"scalar".as_ptr(),
            _ => c"unknown".as_ptr(),
        },
        fixed: info.fixed as u8,
        parallel_built: info.parallel_built as u8,
        simd_available: info.simd_available as u8,
        pool_usable: info.pool_usable as u8,
    }
}

// Writes the fields that fit in `out_size` through a checked pointer; nothing is written on
// error. Every field of the first layout fits once `out_size >= RUNTIME_INFO_MIN_SIZE`.
unsafe fn write_info(
    out: *mut LustroRuntimeInfo,
    out_size: usize,
    get: fn() -> crate::types::RuntimeInfo,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if out.is_null()
            || !(out as usize).is_multiple_of(std::mem::align_of::<LustroRuntimeInfo>())
        {
            return LustroError::InvalidPointer;
        }
        if out_size < RUNTIME_INFO_MIN_SIZE {
            return LustroError::InvalidLength;
        }
        let info = info_to_c(get());
        macro_rules! put {
            ($f:ident) => {
                if std::mem::offset_of!(LustroRuntimeInfo, $f) + std::mem::size_of_val(&info.$f)
                    <= out_size
                {
                    std::ptr::addr_of_mut!((*out).$f).write(info.$f);
                }
            };
        }
        put!(threads);
        put!(logical_cpus);
        put!(backend);
        put!(fixed);
        put!(parallel_built);
        put!(simd_available);
        put!(pool_usable);
        LustroError::Ok
    })
}

/// Stores the execution settings of this process in `*out`. Like `runtime_info()` in Rust,
/// this fixes nothing: settings not yet fixed come from the environment as it is now
/// (`fixed` is 0) and may differ from the ones fixed later (see `lustro_configure`). Another
/// thread can fix them meanwhile, so the result can be stale on return. Does not start the pool.
///
/// `out_size` is `sizeof(LustroRuntimeInfo)`. Only the fields that fit in it are written,
/// so a library newer than the header leaves the extra fields alone. Returns
/// `InvalidPointer` for a null or misaligned `out`, `InvalidLength` if `out_size` is
/// smaller than the first layout of the struct.
///
/// # Safety
/// If `out` is non-null, it must be valid for writing `out_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_runtime_info(
    out: *mut LustroRuntimeInfo,
    out_size: usize,
) -> LustroError {
    write_info(out, out_size, crate::api::runtime_info)
}

/// `simd` for `lustro_configure`: follow `LUSTRO_DISABLE_SIMD`.
pub const LUSTRO_SIMD_KEEP: i32 = 0;
/// `simd` for `lustro_configure`: allow AVX2.
pub const LUSTRO_SIMD_AUTO: i32 = 1;
/// `simd` for `lustro_configure`: scalar only.
pub const LUSTRO_SIMD_SCALAR: i32 = 2;

// `threads == 0` and `LUSTRO_SIMD_KEEP` leave the setting to the environment.
pub(crate) fn options_from_c(threads: usize, simd: i32) -> Result<RuntimeOptions, LustroError> {
    let mut options = RuntimeOptions::new();
    if let Some(n) = NonZeroUsize::new(threads) {
        options = options.threads(n);
    }
    match simd {
        LUSTRO_SIMD_KEEP => {}
        LUSTRO_SIMD_AUTO => options = options.allow_simd(true),
        LUSTRO_SIMD_SCALAR => options = options.allow_simd(false),
        _ => return Err(LustroError::InvalidArgument),
    }
    Ok(options)
}

/// Sets threads and SIMD for this process, before first use. `threads`: 0 = not set,
/// 1 = no pool, larger values are capped at the logical CPU count. `simd`: one of
/// `LUSTRO_SIMD_*`. A value given here replaces the matching environment variables.
///
/// Fixed once, by an earlier call or a batch call: a hash batch, `*_batch_fill_blocks`, or
/// `lustro_xof_batch_new` with `n > 0`. A later call returns `Ok` if the
/// arguments give the same settings as the fixed ones, else `AlreadyConfigured`. Unknown
/// `simd` returns `InvalidArgument` and changes nothing.
///
/// The settings belong to the whole process. An application sets them early; a library that
/// uses Lustro leaves this call to the application and reads them with `lustro_runtime_info`.
#[no_mangle]
pub extern "C" fn lustro_configure(threads: usize, simd: i32) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        match options_from_c(threads, simd) {
            Ok(options) => match crate::api::configure(options) {
                Ok(()) => LustroError::Ok,
                Err(e) => e.into(),
            },
            Err(e) => e,
        }
    })
}

/// `*out_kind` of `lustro_snapshot_kind`: single PRNG stream.
pub const LUSTRO_SNAPSHOT_KIND_PRNG: u8 = 1;
/// `*out_kind` of `lustro_snapshot_kind`: single XOF stream.
pub const LUSTRO_SNAPSHOT_KIND_XOF: u8 = 2;
/// `*out_kind` of `lustro_snapshot_kind`: PRNG batch.
pub const LUSTRO_SNAPSHOT_KIND_PRNG_BATCH: u8 = 3;
/// `*out_kind` of `lustro_snapshot_kind`: XOF batch.
pub const LUSTRO_SNAPSHOT_KIND_XOF_BATCH: u8 = 4;

/// Stores the kind of exported snapshot bytes in `*out_kind` (`LUSTRO_SNAPSHOT_KIND_*`).
/// Reads only the version and kind bytes, so `Ok` does not mean the import will succeed.
/// Returns `InvalidPointer` for a null `out_kind` or a null `bytes` when `len > 0`,
/// `InvalidLength` if `len < 2`, `UnsupportedVersion` or `InvalidKind` for an unknown byte.
/// Nothing is written on error.
///
/// # Safety
/// `bytes` must be valid for `len` bytes when `len > 0`. If `out_kind` is non-null, it must
/// be valid for writing one byte and must not overlap `bytes`.
#[no_mangle]
pub unsafe extern "C" fn lustro_snapshot_kind(
    bytes: *const u8,
    len: usize,
    out_kind: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if out_kind.is_null() {
            return LustroError::InvalidPointer;
        }
        let bytes = match types::bytes_in(bytes, len) {
            Ok(b) => b,
            Err(e) => return e,
        };
        use crate::types::SnapshotKind as K;
        match K::detect(bytes) {
            Ok(kind) => {
                *out_kind = match kind {
                    K::Prng => LUSTRO_SNAPSHOT_KIND_PRNG,
                    K::Xof => LUSTRO_SNAPSHOT_KIND_XOF,
                    K::PrngBatch => LUSTRO_SNAPSHOT_KIND_PRNG_BATCH,
                    K::XofBatch => LUSTRO_SNAPSHOT_KIND_XOF_BATCH,
                };
                LustroError::Ok
            }
            Err(e) => e.into(),
        }
    })
}

/// Returns a static NUL-terminated description of an error code.
/// Unknown codes give "unknown error code". Never NULL; do not free.
#[no_mangle]
pub extern "C" fn lustro_strerror(code: i32) -> *const c_char {
    match crate::errors::LustroError::from_code(code) {
        Some(e) => e.as_nul_str().as_ptr() as *const c_char,
        None => c"unknown error code".as_ptr(),
    }
}
