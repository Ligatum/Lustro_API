//! Lustro V1 PYTHON interface.

pub mod hash;
pub mod prng;
pub mod xof;

use numpy::{PyArray1, PyArray3, PyArrayMethods};
use pyo3::buffer::PyBuffer;
use pyo3::prelude::*;
use pyo3::sync::GILOnceCell;
use pyo3::types::{PyBytes, PySequence, PyString};
use std::borrow::Cow;

// PyO3 turns panics into exceptions only when unwinding.
#[cfg(panic = "abort")]
compile_error!("lustro Python bindings require panic = \"unwind\"");

/// Version of the native API (integer), independent of `__version__`.
#[pyfunction]
pub fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

/// Execution settings of this process as a dict: `backend` ("scalar" or "avx2")
/// and `threads` (configured pool size, 0 = calling thread only). The pool is not
/// started by this call; see `LUSTRO_NUM_THREADS`, `LUSTRO_DISABLE_MT`,
/// `LUSTRO_DISABLE_SIMD`.
#[pyfunction]
pub fn runtime_info(py: Python<'_>) -> PyResult<Bound<'_, pyo3::types::PyDict>> {
    let info = crate::api::runtime_info();
    let d = pyo3::types::PyDict::new_bound(py);
    d.set_item("backend", info.backend)?;
    d.set_item("threads", info.threads)?;
    Ok(d)
}

/// Sets threads and SIMD for this process, before first use. `threads` >= 1, capped
/// at the logical CPU count, 1 = no pool. `allow_simd=False` = scalar only. None
/// follows the environment, a given value replaces it.
///
/// Fixed once, by an earlier call, a batch call, `suggested_steps()` or
/// `runtime_info()`. Later calls raise `RuntimeError`, also with identical arguments.
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
    crate::configure(options).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}

// Separate module: the macro expands to a `gil-refs` cfg that rustc does not know.
#[allow(unexpected_cfgs)]
mod exceptions {
    pyo3::create_exception!(
        lustro,
        SnapshotError,
        pyo3::exceptions::PyValueError,
        "A snapshot could not be imported."
    );
}
pub use exceptions::SnapshotError;

// Maps SnapshotError to the Python SnapshotError (message from Display).
pub(crate) fn snapshot_error(err: crate::types::SnapshotError) -> pyo3::PyErr {
    SnapshotError::new_err(err.to_string())
}

// Maps an allocation failure to MemoryError.
pub(crate) fn alloc_error() -> pyo3::PyErr {
    pyo3::exceptions::PyMemoryError::new_err("allocation failed")
}

// Wrong type, dtype or ndim of an array argument.
pub(crate) fn array_type_error(name: &str, expected: &str) -> pyo3::PyErr {
    pyo3::exceptions::PyTypeError::new_err(format!("{name} must be {expected}"))
}

// Writes into a writable, C-contiguous byte buffer; the GIL stays held throughout.
pub(crate) fn fill_buffer(
    buf: &Bound<'_, pyo3::PyAny>,
    fill: impl FnOnce(&mut [u8]),
) -> PyResult<()> {
    let bad = || {
        pyo3::exceptions::PyTypeError::new_err(
            "buf must be a writable, C-contiguous buffer of bytes \
             (bytearray, memoryview, numpy uint8 array)",
        )
    };
    let view = pyo3::buffer::PyBuffer::<u8>::get_bound(buf).map_err(|_| bad())?;
    if view.readonly() || !view.is_c_contiguous() {
        return Err(bad());
    }
    let len = view.len_bytes();
    if len == 0 {
        return Ok(());
    }
    // SAFETY: `view` is writable and C-contiguous, so `buf_ptr()` covers `len`
    // consecutive bytes. `view` keeps the exporting object alive until `fill` returns.
    let slice = unsafe { std::slice::from_raw_parts_mut(view.buf_ptr() as *mut u8, len) };
    fill(slice);
    Ok(())
}

const BYTES_EXPECTED: &str = "expected bytes or a C-contiguous byte buffer";

// Copy of a byte buffer (bytearray, memoryview, array('B'), NumPy uint8, ...), taken with
// the GIL held. TypeError if there is no buffer, its items are not bytes or it is not
// C-contiguous; MemoryError if the copy cannot be reserved. TypeError and BufferError
// from the exporter are folded into that TypeError, other exception types are passed on.
// The copy protects against other Python threads. Native code that writes to the buffer
// without the GIL is not covered.
fn copy_buffer(obj: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    let bad = || pyo3::exceptions::PyTypeError::new_err(BYTES_EXPECTED);
    // PyO3 reports a missing buffer and a wrong item format as TypeError/BufferError, which
    // cannot be told apart from the same types raised by an exporter.
    let view = PyBuffer::<u8>::get_bound(obj).map_err(|e| {
        let py = obj.py();
        if e.is_instance_of::<pyo3::exceptions::PyTypeError>(py)
            || e.is_instance_of::<pyo3::exceptions::PyBufferError>(py)
        {
            bad()
        } else {
            e
        }
    })?;
    if !view.is_c_contiguous() {
        return Err(bad());
    }
    let len = view.len_bytes();
    let mut copy = Vec::new();
    copy.try_reserve_exact(len).map_err(|_| alloc_error())?;
    if len != 0 {
        // SAFETY: `view` is C-contiguous, so `buf_ptr()` covers `len` consecutive bytes,
        // and it keeps the exporter alive and the buffer in place until it is dropped.
        let src = unsafe { std::slice::from_raw_parts(view.buf_ptr() as *const u8, len) };
        copy.extend_from_slice(src);
    }
    Ok(copy)
}

// Bytes-like argument (`from_py_with`). `bytes` is borrowed, any other byte buffer is
// copied, so the data stays valid and unchanged while the GIL is released.
pub(crate) fn bytes_arg<'a>(obj: &'a Bound<'_, PyAny>) -> PyResult<Cow<'a, [u8]>> {
    if let Ok(bytes) = obj.downcast::<PyBytes>() {
        return Ok(Cow::Borrowed(bytes.as_bytes()));
    }
    copy_buffer(obj).map(Cow::Owned)
}

// One message of a batch: `bytes` stays referenced, other buffers are copies.
enum Message<'py> {
    Bytes(Bound<'py, PyBytes>),
    Copy(Vec<u8>),
}

// Messages of a batch argument, ready to be used while the GIL is released.
pub struct Messages<'py>(Vec<Message<'py>>);

impl Messages<'_> {
    pub(crate) fn slices(&self) -> Vec<&[u8]> {
        self.0
            .iter()
            .map(|m| match m {
                Message::Bytes(b) => b.as_bytes(),
                Message::Copy(v) => v.as_slice(),
            })
            .collect()
    }
}

// Batch argument (`from_py_with`): only the container is checked here, the items are read
// by `load()`, so that other argument checks can come first. A single string or buffer is
// rejected as a whole, even though `memoryview` and `array` are sequences of integers.
pub struct MessageSeq<'py>(Bound<'py, PySequence>);

pub(crate) fn messages_arg<'py>(obj: &Bound<'py, PyAny>) -> PyResult<MessageSeq<'py>> {
    let type_error = |msg: &'static str| pyo3::exceptions::PyTypeError::new_err(msg);
    // SAFETY: `obj` is a valid object pointer and the GIL is held.
    if obj.is_instance_of::<PyString>()
        || unsafe { pyo3::ffi::PyObject_CheckBuffer(obj.as_ptr()) } != 0
    {
        return Err(type_error(
            "expected a sequence of bytes-like objects, not a single string or buffer",
        ));
    }
    let seq = obj
        .downcast::<PySequence>()
        .map_err(|_| type_error("expected a sequence of bytes-like objects"))?;
    Ok(MessageSeq(seq.clone()))
}

impl<'py> MessageSeq<'py> {
    // TypeError names the first item that is not a byte buffer.
    pub(crate) fn load(&self) -> PyResult<Messages<'py>> {
        let mut items = Vec::new();
        items
            .try_reserve_exact(self.0.len()?)
            .map_err(|_| alloc_error())?;
        for (i, item) in self.0.iter()?.enumerate() {
            let message = match item?.downcast_into::<PyBytes>() {
                Ok(bytes) => Message::Bytes(bytes),
                Err(e) => Message::Copy(copy_buffer(&e.into_inner()).map_err(|err| {
                    if err.is_instance_of::<pyo3::exceptions::PyTypeError>(self.0.py()) {
                        pyo3::exceptions::PyTypeError::new_err(format!(
                            "messages[{i}]: {BYTES_EXPECTED}"
                        ))
                    } else {
                        err
                    }
                })?),
            };
            items.push(message);
        }
        Ok(Messages(items))
    }
}

// Step-major blocks -> little-endian u64 words, 4 per block.
// `words.len()` must equal `blocks.len() * 4`.
pub(crate) fn blocks_to_words(blocks: &[[u8; 32]], words: &mut [u64]) {
    debug_assert_eq!(words.len(), blocks.len() * 4);
    #[cfg(target_endian = "little")]
    {
        // SAFETY: `words` is a valid `&mut [u64]` and the byte view covers exactly the
        // same memory; any bit pattern is a valid u64.
        let dst = unsafe {
            std::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut u8, words.len() * 8)
        };
        dst.copy_from_slice(blocks.as_flattened());
    }
    #[cfg(not(target_endian = "little"))]
    for (block, w) in blocks.iter().zip(words.chunks_exact_mut(4)) {
        w[0] = u64::from_le_bytes(block[0..8].try_into().unwrap());
        w[1] = u64::from_le_bytes(block[8..16].try_into().unwrap());
        w[2] = u64::from_le_bytes(block[16..24].try_into().unwrap());
        w[3] = u64::from_le_bytes(block[24..32].try_into().unwrap());
    }
}

// Without NumPy, using an array panics. Importing the module up front turns that into
// an ImportError, and for stream calls it happens before the state has moved.
pub(crate) fn require_numpy(py: Python<'_>) -> PyResult<()> {
    static IMPORTED: GILOnceCell<()> = GILOnceCell::new();
    IMPORTED
        .get_or_try_init(py, || numpy::get_array_module(py).map(|_| ()))
        .map(|_| ())
}

// Zeroed words for a new array; MemoryError if they cannot be reserved.
fn new_words(len: usize) -> PyResult<Vec<u64>> {
    let mut words = Vec::new();
    words.try_reserve_exact(len).map_err(|_| alloc_error())?;
    words.resize(len, 0);
    Ok(words)
}

// Stream bytes are written into the words' memory as is; this turns them into values.
fn words_from_le(words: &mut [u64]) {
    #[cfg(target_endian = "big")]
    for w in words {
        *w = u64::from_le(*w);
    }
    #[cfg(target_endian = "little")]
    let _ = words;
}

// New uint64 array of shape `(n,)` with the next `n` stream words, as `next_u64()` returns them.
// The size check, the NumPy check and the buffer allocation come before `fill`, so their
// failures leave the state untouched. Array construction happens after `fill`.
// The GIL is released while `fill` runs.
pub(crate) fn u64_array<'py>(
    py: Python<'py>,
    n: usize,
    fill: impl FnOnce(&mut [u8]) + Send,
) -> PyResult<Bound<'py, PyArray1<u64>>> {
    if n.checked_mul(8).is_none_or(|b| b > isize::MAX as usize) {
        return Err(pyo3::exceptions::PyOverflowError::new_err(
            "size exceeds isize::MAX",
        ));
    }
    require_numpy(py)?;
    let mut words = new_words(n)?;
    if n != 0 {
        // SAFETY: the byte view covers exactly the memory of `words`; any bit pattern
        // is a valid u8 and u64.
        let bytes = unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut u8, n * 8) };
        py.allow_threads(|| fill(bytes));
        words_from_le(&mut words);
    }
    Ok(PyArray1::from_vec_bound(py, words))
}

// New uint64 array of shape `(steps, lanes, 4)` laid out as `fill_blocks()` writes it.
// The size checks, the NumPy check and the buffer allocation come before `fill`, so their
// failures leave the state untouched. Array construction and reshape happen after `fill`.
// The GIL is released while `fill` runs.
pub(crate) fn blocks_array<'py>(
    py: Python<'py>,
    lanes: usize,
    steps: usize,
    fill: impl FnOnce(&mut [[u8; 32]]) + Send,
) -> PyResult<Bound<'py, PyArray3<u64>>> {
    let too_big =
        || pyo3::exceptions::PyValueError::new_err("output size exceeds isize::MAX bytes");
    if steps > isize::MAX as usize {
        return Err(too_big());
    }
    let blocks = lanes.checked_mul(steps).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err("n_streams * steps overflows usize")
    })?;
    if blocks
        .checked_mul(32)
        .is_none_or(|b| b > isize::MAX as usize)
    {
        return Err(too_big());
    }
    require_numpy(py)?;
    let mut words = new_words(blocks * 4)?;
    if blocks != 0 {
        // SAFETY: `[u8; 32]` has alignment 1 and the view covers exactly the memory of
        // `words` (4 words per block); any bit pattern is valid.
        let view =
            unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut [u8; 32], blocks) };
        py.allow_threads(|| fill(view));
        words_from_le(&mut words);
    }
    PyArray1::from_vec_bound(py, words).reshape([steps, lanes, 4])
}
