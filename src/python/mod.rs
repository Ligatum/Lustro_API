//! Lustro V1 PYTHON interface.

use pyo3::prelude::*;
use std::borrow::Cow;
use std::sync::OnceLock;

pub mod hash;
pub mod prng;
pub mod xof;

mod arrays;
mod buffers;
mod messages;

#[cfg(target_endian = "little")]
pub(crate) use arrays::words_as_blocks;
pub(crate) use arrays::*;
pub(crate) use buffers::*;
pub(crate) use messages::*;

// PyO3 turns panics into exceptions only when unwinding.
#[cfg(panic = "abort")]
compile_error!("lustro Python bindings require panic = \"unwind\"");

// ==========================================
// MODULE FUNCTIONS
// ==========================================
// Functions exported to Python from the module itself.

/// Version of the native API (integer), independent of `__version__`.
#[pyfunction]
pub fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

/// Execution settings of this process as a dict: `backend` ("scalar" or "avx2", as
/// selected for stream and hash batches) and `threads` (configured pool size, not a count of
/// active workers; 0 = calling thread only). The pool is not started by this call and may
/// fail to start; batches then run on the calling thread. Also `fixed`, `parallel_built`,
/// `simd_available`, `pool_usable` (the pool is built and this process can use it; False
/// also before the first parallel batch and in a child forked after the parent started the
/// pool) and `logical_cpus`. This call fixes nothing: if the settings are not fixed yet,
/// `fixed` is False and the values come from the environment as it is now; they can differ
/// from the ones fixed later. Another thread can fix them meanwhile, so the result can be
/// stale on return.
/// See `LUSTRO_NUM_THREADS`, `LUSTRO_DISABLE_MT`, `LUSTRO_DISABLE_SIMD`.
#[pyfunction]
pub fn runtime_info(py: Python<'_>) -> PyResult<Bound<'_, pyo3::types::PyDict>> {
    info_dict(py, crate::api::runtime_info())
}

fn info_dict(py: Python<'_>, info: crate::RuntimeInfo) -> PyResult<Bound<'_, pyo3::types::PyDict>> {
    let d = pyo3::types::PyDict::new(py);
    d.set_item("backend", info.backend)?;
    d.set_item("threads", info.threads)?;
    d.set_item("fixed", info.fixed)?;
    d.set_item("parallel_built", info.parallel_built)?;
    d.set_item("simd_available", info.simd_available)?;
    d.set_item("pool_usable", info.pool_usable)?;
    d.set_item("logical_cpus", info.logical_cpus)?;
    Ok(d)
}

/// Sets threads and SIMD for this process, before first use. `threads` >= 1, capped
/// at the logical CPU count, 1 = no pool. `allow_simd=False` = scalar only. None
/// follows the environment, a given value replaces it.
///
/// Fixed once, by an earlier call or a batch call: a hash batch, `fill_blocks`/`blocks`, or
/// `LustroXofBatch(...)` with at least one message. A later call returns
/// None if the arguments give the same settings as the fixed ones, else raises
/// `ConfigError` (a subclass of `RuntimeError`).
///
/// The settings belong to the whole process. An application sets them early; a library that
/// uses Lustro leaves this call to the application and reads them with `runtime_info()`.
#[pyfunction]
#[pyo3(signature = (threads=None, allow_simd=None))]
pub fn configure(threads: Option<i64>, allow_simd: Option<bool>) -> PyResult<()> {
    let mut options = crate::RuntimeOptions::new();
    if let Some(n) = threads {
        if n < 1 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "threads must be at least 1",
            ));
        }
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        options =
            options.threads(std::num::NonZeroUsize::new(n).unwrap_or(std::num::NonZeroUsize::MIN));
    }
    if let Some(allow) = allow_simd {
        options = options.allow_simd(allow);
    }
    crate::configure(options).map_err(|e| ConfigError::new_err(e.to_string()))
}

/// Kind of exported snapshot bytes: "prng", "xof", "prng_batch" or "xof_batch".
/// Reads only the version and kind bytes, so a result does not mean `import_snapshot` will
/// succeed. Raises SnapshotError for fewer than 2 bytes or an unknown version or kind.
#[pyfunction]
pub fn snapshot_kind(
    #[pyo3(from_py_with = bytes_arg)] bytes: Cow<'_, [u8]>,
) -> PyResult<&'static str> {
    use crate::types::SnapshotKind as K;
    Ok(match K::detect(&bytes).map_err(snapshot_error)? {
        K::Prng => "prng",
        K::Xof => "xof",
        K::PrngBatch => "prng_batch",
        K::XofBatch => "xof_batch",
    })
}

// ==========================================
// ERRORS
// ==========================================
// Exception types and the mapping of internal errors to Python exceptions.

// Separate module: the macro expands to a `gil-refs` cfg that rustc does not know.
#[allow(unexpected_cfgs)]
mod exceptions {
    pyo3::create_exception!(
        lustro,
        SnapshotError,
        pyo3::exceptions::PyValueError,
        "A snapshot could not be imported."
    );
    pyo3::create_exception!(
        lustro,
        ConfigError,
        pyo3::exceptions::PyRuntimeError,
        "The runtime settings are already fixed with another effect."
    );
}
pub use exceptions::{ConfigError, SnapshotError};

// Maps SnapshotError to the Python SnapshotError (message from Display).
pub(crate) fn snapshot_error(err: crate::types::SnapshotError) -> pyo3::PyErr {
    match err {
        crate::types::SnapshotError::Reserve => alloc_error(),
        _ => SnapshotError::new_err(err.to_string()),
    }
}

// Maps an allocation failure to MemoryError.
pub(crate) fn alloc_error() -> pyo3::PyErr {
    pyo3::exceptions::PyMemoryError::new_err("allocation failed")
}

// `bytes` copy of `data`. `PyBytes::new` panics when Python cannot allocate; this raises MemoryError.
pub(crate) fn try_bytes<'py>(
    py: Python<'py>,
    data: &[u8],
) -> PyResult<Bound<'py, pyo3::types::PyBytes>> {
    try_bytes_with(py, data.len(), |out| out.copy_from_slice(data))
}

// As `try_bytes`, with the content produced by `fill` after the allocation succeeded, so a
// failed allocation leaves whatever `fill` would advance untouched.
pub(crate) fn try_bytes_with<'py>(
    py: Python<'py>,
    len: usize,
    fill: impl FnOnce(&mut [u8]),
) -> PyResult<Bound<'py, pyo3::types::PyBytes>> {
    pyo3::types::PyBytes::new_with(py, len, |out| {
        fill(out);
        Ok(())
    })
    .map_err(|e| {
        if e.is_instance_of::<pyo3::exceptions::PyMemoryError>(py) {
            alloc_error()
        } else {
            e
        }
    })
}

// Index argument of `batch[i]`: anything with `__index__`. As for a list, an integer that
// does not fit `isize` is out of range (IndexError), not an overflow; other types stay TypeError.
pub(crate) fn item_index(index: &Bound<'_, PyAny>) -> PyResult<isize> {
    index.extract::<isize>().map_err(|e| {
        if e.is_instance_of::<pyo3::exceptions::PyOverflowError>(index.py()) {
            pyo3::exceptions::PyIndexError::new_err("lane index out of range")
        } else {
            e
        }
    })
}

// Lane index as in a list: a negative value counts from the end.
pub(crate) fn lane_index(index: isize, len: usize) -> pyo3::PyResult<usize> {
    let i = if index < 0 {
        len.checked_sub(index.unsigned_abs())
    } else {
        Some(index as usize)
    };
    i.filter(|&i| i < len)
        .ok_or_else(|| pyo3::exceptions::PyIndexError::new_err("lane index out of range"))
}

// Error of a batch `try_derive_path`. The callers check `path` first, so `EmptyPath`
// is not expected here.
pub(crate) fn derive_path_error(e: crate::types::DerivePathError) -> pyo3::PyErr {
    match e {
        crate::types::DerivePathError::EmptyPath => {
            pyo3::exceptions::PyValueError::new_err("path must not be empty")
        }
        crate::types::DerivePathError::Reserve(_) => alloc_error(),
    }
}

// BatchError as a Python exception. Every variant is listed, so a new one needs a choice here.
pub(crate) fn batch_error(e: crate::types::BatchError) -> pyo3::PyErr {
    use crate::types::BatchError as B;
    match e {
        B::Reserve(_) => alloc_error(),
        B::SizeOverflow => pyo3::exceptions::PyOverflowError::new_err(e.to_string()),
        B::LengthMismatch { .. } | B::PartialBlock { .. } | B::InvalidMessageLen { .. } => {
            pyo3::exceptions::PyValueError::new_err(e.to_string())
        }
    }
}

// Wrong type, dtype or ndim of an array argument.
pub(crate) fn array_type_error(name: &str, expected: &str) -> pyo3::PyErr {
    pyo3::exceptions::PyTypeError::new_err(format!("{name} must be {expected}"))
}

// ==========================================
// ARGUMENT CONVERSION
// ==========================================
// Integer arguments that PyO3 does not convert with the error types the API promises.

// u128 argument (`from_py_with`). A negative int is `OverflowError` on every Python
// version, PyO3 gives `ValueError` for it on 3.13.
pub(crate) fn u128_arg(obj: &Bound<'_, PyAny>) -> PyResult<u128> {
    obj.extract::<u128>().map_err(|e| {
        let py = obj.py();
        if e.is_instance_of::<pyo3::exceptions::PyValueError>(py) && obj.lt(0).unwrap_or(false) {
            pyo3::exceptions::PyOverflowError::new_err("can't convert negative int to unsigned")
        } else {
            e
        }
    })
}

// List of u128 ids (`from_py_with`), same rules per item as `u128_arg`.
pub(crate) fn ids_arg(obj: &Bound<'_, PyAny>) -> PyResult<Vec<u128>> {
    let items = seq_items(obj)?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(items.len())
        .map_err(|_| alloc_error())?;
    for item in &items {
        ids.push(u128_arg(item)?);
    }
    Ok(ids)
}

// Items of a sequence argument, accepted as by PyO3's `Vec` extraction (`PySequence_Check`,
// a `str` is TypeError) but read one by one. PyO3 reserves `len()` items first, so a wrong
// or huge `__len__` ends in a panic (capacity overflow) or an abort (failed allocation).
// A plain loop, not `collect`: the iterator's `size_hint` calls `__length_hint__`/`__len__`.
// Room is reserved with `try_reserve`, so a failed allocation is a MemoryError.
pub(crate) fn seq_items<'py>(obj: &Bound<'py, PyAny>) -> PyResult<Vec<Bound<'py, PyAny>>> {
    if obj.is_instance_of::<pyo3::types::PyString>() {
        return Err(pyo3::exceptions::PyTypeError::new_err(
            "Can't extract `str` to `Vec`",
        ));
    }
    // SAFETY: `obj` is a valid object pointer and the GIL is held.
    if unsafe { pyo3::ffi::PySequence_Check(obj.as_ptr()) } == 0 {
        let sequence = <pyo3::types::PySequence as pyo3::PyTypeInfo>::type_object(obj.py());
        return Err(pyo3::CastError::new(obj.as_borrowed(), sequence.into_any()).into());
    }
    let mut items = Vec::new();
    for item in obj.try_iter()? {
        items.try_reserve(1).map_err(|_| alloc_error())?;
        items.push(item?);
    }
    Ok(items)
}

// ==========================================
// GIL STATE
// ==========================================

// Whether the GIL is enabled (False on a free-threaded build run without it). Read once, at
// module init (`init_gil_flag`), so the later calls run no Python code. No `sys._is_gil_enabled`
// (Python before 3.13) means a build with the GIL; any other failure counts as disabled.
fn gil_enabled(py: Python<'_>) -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    if let Some(&on) = CACHE.get() {
        return on;
    }
    // Queried outside `get_or_init`: the call may run user code that re-enters this function.
    let on = match py
        .import("sys")
        .and_then(|sys| sys.getattr("_is_gil_enabled"))
    {
        Ok(f) => f.call0().and_then(|r| r.is_truthy()).unwrap_or(false),
        Err(e) => e.is_instance_of::<pyo3::exceptions::PyAttributeError>(py),
    };
    *CACHE.get_or_init(|| on)
}

// Fills the GIL flag; called from the module init.
pub(crate) fn init_gil_flag(py: Python<'_>) {
    gil_enabled(py);
}
