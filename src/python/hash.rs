//! Python bindings for Lustro Hash.

use numpy::ndarray::ArrayView2;
use numpy::{
    PyArray2, PyArrayDescrMethods, PyArrayMethods, PyReadwriteArray2, PyUntypedArrayMethods,
};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::borrow::Cow;
use std::cell::RefCell;

use crate::hash::hash128 as hash128_impl;
use crate::hash::hash128_many_fixed_into;
use crate::hash::hash128_many_var_into;
use crate::hash::hash256 as hash256_impl;
use crate::hash::hash256_many_fixed_into;
use crate::hash::hash256_many_var_into;
use crate::hash::{hash128_many_var_blocks, hash256_many_var_blocks};
use crate::python::{bytes_arg, messages_arg, require_numpy, ListMessages, MessageSeq, Messages};
use crate::types::{Hash128, Hash256};

// ==========================================
// INPUT HELPERS
// ==========================================

// Largest input kept in the per-thread scratch buffer between calls. A bigger input gets a
// buffer that is freed when the call ends.
const SCRATCH_KEEP: usize = 32 << 20;

thread_local! {
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

// Rows copied into one buffer: `n` messages of `len` bytes, back to back. The buffer comes
// from the scratch of the calling thread and goes back to it on drop.
struct FlatRows {
    buf: Vec<u8>,
    n: usize,
    len: usize,
}

impl Drop for FlatRows {
    fn drop(&mut self) {
        if self.buf.capacity() <= SCRATCH_KEEP {
            let buf = std::mem::take(&mut self.buf);
            // Err only while the thread is exiting; the buffer is freed then.
            let _ = SCRATCH.try_with(|s| *s.borrow_mut() = buf);
        }
    }
}

// Own copy — can't hold a &[u8] into NumPy's buffer across detach().
// Row-contiguous views (a[::2], a[:, :16]) are accepted, ValueError if any row is not.
fn copy_rows(view: ArrayView2<u8>) -> PyResult<FlatRows> {
    let (n, len) = view.dim();
    let buf = SCRATCH
        .try_with(|s| std::mem::take(&mut *s.borrow_mut()))
        .unwrap_or_default();
    let mut flat = FlatRows { buf, n, len };
    flat.buf.clear();
    flat.buf
        .try_reserve_exact(view.len())
        .map_err(|_| crate::python::alloc_error())?;
    match view.as_slice() {
        Some(s) => flat.buf.extend_from_slice(s),
        None => {
            for r in view.rows() {
                flat.buf
                    .extend_from_slice(r.to_slice().ok_or_else(non_contiguous_error)?);
            }
        }
    }
    Ok(flat)
}

fn non_contiguous_error() -> PyErr {
    pyo3::exceptions::PyValueError::new_err(
        "data must have contiguous rows (got a view with non-contiguous rows, \
         e.g. from slicing or transposing — call np.ascontiguousarray() first)",
    )
}

// ==========================================
// OUTPUT HELPERS
// ==========================================

// Digests go straight into the output array buffer.
fn rows_array<'a, 'py>(data: &'a Bound<'py, PyAny>) -> PyResult<&'a Bound<'py, PyArray2<u8>>> {
    data.cast::<PyArray2<u8>>()
        .map_err(|_| crate::python::array_type_error("data", "a 2D numpy.ndarray of dtype uint8"))
}

// Digests go straight into `out_arr`, a fresh (n, 32) array made by the caller.
fn digests256<'py>(
    py: Python<'py>,
    out_arr: Bound<'py, PyArray2<u8>>,
    release_gil: bool,
    run: impl FnOnce(&mut [Hash256]) + Send,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let n = out_arr.shape()[0];
    {
        let mut out_rw = out_arr.readwrite();
        let out_slice = out_rw.as_slice_mut().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err(
                "internal error: freshly allocated output array is not contiguous",
            )
        })?;
        // SAFETY: Hash256 is repr(transparent) over [u8; 32].
        let out_blocks: &mut [Hash256] =
            unsafe { std::slice::from_raw_parts_mut(out_slice.as_mut_ptr() as *mut Hash256, n) };

        if release_gil {
            py.detach(|| run(out_blocks));
        } else {
            run(out_blocks);
        }
    }
    Ok(out_arr)
}

// Digests go straight into `out_arr`, a fresh (n, 16) array made by the caller.
fn digests128<'py>(
    py: Python<'py>,
    out_arr: Bound<'py, PyArray2<u8>>,
    release_gil: bool,
    run: impl FnOnce(&mut [Hash128]) + Send,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let n = out_arr.shape()[0];
    {
        let mut out_rw = out_arr.readwrite();
        let out_slice = out_rw.as_slice_mut().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err(
                "internal error: freshly allocated output array is not contiguous",
            )
        })?;
        // SAFETY: Hash128 is repr(transparent) over [u8; 16].
        let out_blocks: &mut [Hash128] =
            unsafe { std::slice::from_raw_parts_mut(out_slice.as_mut_ptr() as *mut Hash128, n) };

        if release_gil {
            py.detach(|| run(out_blocks));
        } else {
            run(out_blocks);
        }
    }
    Ok(out_arr)
}

// Digests of one `*_into` call, before they are copied into the caller's array. Per
// thread, kept under the same limit as the input scratch.
thread_local! {
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

struct ResultBuf(Vec<u8>);

impl ResultBuf {
    // At least `len` bytes. Zero-filled only when the buffer grows.
    fn take(len: usize) -> PyResult<Self> {
        let mut buf = RESULT
            .try_with(|s| std::mem::take(&mut *s.borrow_mut()))
            .unwrap_or_default();
        if buf.len() < len {
            buf.try_reserve_exact(len - buf.len())
                .map_err(|_| crate::python::alloc_error())?;
            buf.resize(len, 0);
        }
        Ok(Self(buf))
    }
}

impl Drop for ResultBuf {
    fn drop(&mut self) {
        if self.0.capacity() <= SCRATCH_KEEP {
            let buf = std::mem::take(&mut self.0);
            // Err only while the thread is exiting; the buffer is freed then.
            let _ = RESULT.try_with(|s| *s.borrow_mut() = buf);
        }
    }
}

fn out_not_contiguous_error() -> PyErr {
    pyo3::exceptions::PyValueError::new_err(
        "out must be a C-contiguous array (got a non-contiguous view, \
         e.g. from slicing or transposing — call np.ascontiguousarray() first)",
    )
}

// Checks `out` before any work is done: uint8, 2D, writable, shape (n, W), contiguous.
fn out_rows<'py, const W: usize>(
    out: &Bound<'py, PyAny>,
    n: usize,
) -> PyResult<PyReadwriteArray2<'py, u8>> {
    let out = out
        .cast::<PyArray2<u8>>()
        .map_err(|_| crate::python::array_type_error("out", "a 2D numpy.ndarray of dtype uint8"))?;
    let mut out = out.try_readwrite()?;
    if out.shape() != [n, W] {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "out must have shape ({n}, {W}), got {:?}",
            out.shape()
        )));
    }
    // ndarray's check: C layout only (NumPy's own slice accessor also accepts F order).
    if n != 0 && out.as_array_mut().as_slice_mut().is_none() {
        return Err(out_not_contiguous_error());
    }
    Ok(out)
}

// Python code or another thread can reshape `out` after it was validated. This reads the
// current shape (any ndim), so it must run before `as_array_mut`, which panics on a
// different ndim.
fn check_out_shape<const W: usize>(out: &PyReadwriteArray2<'_, u8>, n: usize) -> PyResult<()> {
    if out.shape() != [n, W] {
        return Err(out_changed_error());
    }
    Ok(())
}

pub(crate) fn overlaps(a: usize, a_len: usize, b: usize, b_len: usize) -> bool {
    a_len != 0 && b_len != 0 && a < b.saturating_add(b_len) && b < a.saturating_add(a_len)
}

// `PyArray2::zeros` panics when NumPy fails, and `n` comes from user `__len__`. This calls
// `PyArray_Zeros` directly and passes a failure on as the exception NumPy set (MemoryError).
fn alloc_out<'py, const W: usize>(py: Python<'py>, n: usize) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let too_big = || crate::python::alloc_error();
    let bytes = n.checked_mul(W).ok_or_else(too_big)?;
    let mut dims = [
        numpy::npyffi::npy_intp::try_from(n).map_err(|_| too_big())?,
        numpy::npyffi::npy_intp::try_from(W).map_err(|_| too_big())?,
    ];
    numpy::npyffi::npy_intp::try_from(bytes).map_err(|_| too_big())?;
    // SAFETY: the GIL is held; `dims` has 2 entries; `PyArray_Zeros` takes over the dtype
    // reference and returns a new reference or NULL with an exception set.
    unsafe {
        let ptr = numpy::npyffi::PY_ARRAY_API.PyArray_Zeros(
            py,
            2,
            dims.as_mut_ptr(),
            <u8 as numpy::Element>::get_dtype(py).into_dtype_ptr(),
            0,
        );
        Ok(Bound::from_owned_ptr_or_err(py, ptr)?.cast_into_unchecked())
    }
}

fn out_changed_error() -> PyErr {
    pyo3::exceptions::PyValueError::new_err("out changed shape while it was being written")
}

// Digests are computed with the GIL released into the thread's scratch, then copied into
// `out` with the GIL held. No write to caller memory happens while it is released.
fn digests_into<T: Send, const W: usize>(
    py: Python<'_>,
    mut out: PyReadwriteArray2<'_, u8>,
    n: usize,
    run: impl FnOnce(&mut [T]) + Send,
) -> PyResult<()> {
    debug_assert_eq!(std::mem::size_of::<T>(), W);
    if n == 0 {
        return Ok(());
    }
    let mut res = ResultBuf::take(n * W)?;
    {
        // SAFETY: T is a repr(transparent) wrapper over [u8; W], so align 1; the buffer
        // holds at least n * W initialised bytes.
        let blocks: &mut [T] =
            unsafe { std::slice::from_raw_parts_mut(res.0.as_mut_ptr() as *mut T, n) };
        py.detach(|| run(blocks));
    }
    // Re-borrowed after the GIL is back.
    check_out_shape::<W>(&out, n)?;
    let mut view = out.as_array_mut();
    let dst = view.as_slice_mut().ok_or_else(out_not_contiguous_error)?;
    if dst.len() != n * W {
        return Err(out_changed_error());
    }
    dst.copy_from_slice(&res.0[..n * W]);
    Ok(())
}

// ==========================================
// HASH API
// ==========================================

/// 256-bit digest of `data` (bytes or a byte buffer of any length) as 32 bytes.
/// `bytes` is not copied; other buffers are copied. The GIL is released while hashing.
#[pyfunction]
#[pyo3(name = "hash256")]
pub fn hash256_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = bytes_arg)] data: Cow<'_, [u8]>,
) -> Bound<'py, PyBytes> {
    let result = py.detach(|| hash256_impl(&data));
    PyBytes::new(py, result.as_bytes())
}

/// 128-bit digest of `data` (bytes or a byte buffer of any length) as 16 bytes.
/// `bytes` is not copied; other buffers are copied. The GIL is released while hashing.
#[pyfunction]
#[pyo3(name = "hash128")]
pub fn hash128_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = bytes_arg)] data: Cow<'_, [u8]>,
) -> Bound<'py, PyBytes> {
    let result = py.detach(|| hash128_impl(&data));
    PyBytes::new(py, result.as_bytes())
}

// ==========================================
// HASH BATCH API (FIXED LENGTH)
// ==========================================

/// 256-bit digests of the rows of a 2D uint8 array (all messages have the same
/// length). Returns a uint8 array of shape `(n, 32)`.
///
/// TypeError if `data` is not a 2D numpy.ndarray of dtype uint8.
///
/// Views whose rows are contiguous (`a[::2]`, `a[:, :16]`) are accepted. If any row
/// is not contiguous (`a[:, ::2]`, `a.T`), ValueError is raised. The input is copied
/// before the GIL is released, so hashing reads from the copy rather than the
/// original NumPy array.
#[pyfunction]
#[pyo3(name = "hash256_many_fixed")]
pub fn hash256_many_py<'py>(
    py: Python<'py>,
    data: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    require_numpy(py)?;
    let data = rows_array(data)?;
    // Use the non-panicking borrow API so conflicting NumPy borrows become Python errors.
    let data = data.try_readonly()?;
    let flat = copy_rows(data.as_array())?;
    let out = alloc_out::<32>(py, flat.n)?;
    digests256(py, out, true, |out| {
        hash256_many_fixed_into(&flat.buf, flat.len, out)
    })
}

/// 128-bit digests of the rows of a 2D uint8 array. Same input rules as
/// `hash256_many_fixed`. Returns a uint8 array of shape `(n, 16)`.
#[pyfunction]
#[pyo3(name = "hash128_many_fixed")]
pub fn hash128_many_py<'py>(
    py: Python<'py>,
    data: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    require_numpy(py)?;
    let data = rows_array(data)?;
    let data = data.try_readonly()?;
    let flat = copy_rows(data.as_array())?;
    let out = alloc_out::<16>(py, flat.n)?;
    digests128(py, out, true, |out| {
        hash128_many_fixed_into(&flat.buf, flat.len, out)
    })
}

// Digests straight into `out`, GIL held by the caller throughout.
fn write_direct<T, const W: usize>(
    out: &mut PyReadwriteArray2<'_, u8>,
    n: usize,
    run: impl FnOnce(&mut [T]),
) -> PyResult<()> {
    if n == 0 {
        return Ok(());
    }
    check_out_shape::<W>(out, n)?;
    let mut view = out.as_array_mut();
    let dst = view.as_slice_mut().ok_or_else(out_not_contiguous_error)?;
    // `out` was validated earlier; Python code that ran since then may have resized it.
    if dst.len() != n * W {
        return Err(out_changed_error());
    }
    // SAFETY: T is a repr(transparent) wrapper over [u8; W], so align 1; `dst` holds
    // n * W bytes (checked above).
    let blocks: &mut [T] = unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut T, n) };
    run(blocks);
    Ok(())
}

// Shared body of `hash{256,128}_many_fixed_into`. With the GIL held the input is read in
// place and the digests are written straight into `out`; this needs a C-contiguous `data`,
// any other view takes the copying path below.
fn fixed_into<T: Send, const W: usize>(
    py: Python<'_>,
    data: &Bound<'_, PyAny>,
    out: &Bound<'_, PyAny>,
    release_gil: bool,
    hash: fn(&[u8], usize, &mut [T]),
) -> PyResult<()> {
    require_numpy(py)?;
    let data = rows_array(data)?;
    let mut out = out_rows::<W>(out, data.shape()[0])?;
    let data = data.try_readonly()?;
    let view = data.as_array();
    if !release_gil {
        if let Some(input) = view.as_slice() {
            let (n, len) = view.dim();
            // Separate NumPy wrappers over one buffer are not seen by the borrow check.
            if overlaps(
                input.as_ptr() as usize,
                input.len(),
                out.data() as usize,
                n * W,
            ) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "data and out must not share memory",
                ));
            }
            return write_direct::<T, W>(&mut out, n, |blocks| hash(input, len, blocks));
        }
    }
    let flat = copy_rows(view)?;
    drop(data);
    digests_into::<T, W>(py, out, flat.n, |o| hash(&flat.buf, flat.len, o))
}

/// As `hash256_many_fixed`, writing the digests into `out` instead of returning a new
/// array. `out` must be a writable, C-contiguous uint8 array of shape `(len(data), 32)`;
/// ValueError for another shape or a non-contiguous `out`, TypeError for another type,
/// dtype or ndim. Returns None.
///
/// By default the digests are computed with the GIL released and copied into `out` once
/// it is held again, from a copy of `data`. With `release_gil=False` the GIL stays held:
/// `data` is read in place and the digests are written into `out` directly (no copies),
/// so other Python threads wait for the call. A `data` view that is not C-contiguous is
/// copied and hashed with the GIL released, as in the default mode. The same array,
/// or views of one array, as `data` and `out` raise TypeError (NumPy borrow check).
/// Separate wrappers over one buffer (`np.frombuffer` on one `bytearray`) are not seen
/// by it: with `release_gil=False` they raise ValueError, by default `data` is copied
/// first and the call is allowed.
#[pyfunction]
#[pyo3(name = "hash256_many_fixed_into", signature = (data, out, *, release_gil=true))]
pub fn hash256_many_into_py(
    py: Python<'_>,
    data: &Bound<'_, PyAny>,
    out: &Bound<'_, PyAny>,
    release_gil: bool,
) -> PyResult<()> {
    fixed_into::<Hash256, 32>(py, data, out, release_gil, hash256_many_fixed_into)
}

/// As `hash128_many_fixed`, writing the digests into `out`; same rules as
/// `hash256_many_fixed_into` with shape `(len(data), 16)`.
#[pyfunction]
#[pyo3(name = "hash128_many_fixed_into", signature = (data, out, *, release_gil=true))]
pub fn hash128_many_into_py(
    py: Python<'_>,
    data: &Bound<'_, PyAny>,
    out: &Bound<'_, PyAny>,
    release_gil: bool,
) -> PyResult<()> {
    fixed_into::<Hash128, 16>(py, data, out, release_gil, hash128_many_fixed_into)
}

// ==========================================
// HASH BATCH API (VARIABLE LENGTH)
// ==========================================

// Messages of a var call. With the GIL held throughout, an exact `list` or `tuple` of `bytes`
// is read without a reference per item.
// `apart` = (address, length) of the output when it is a caller's array: with the GIL held
// an item must not share memory with it.
fn load_messages<'py>(
    messages: &MessageSeq<'py>,
    release_gil: bool,
    apart: Option<(usize, usize)>,
) -> PyResult<Messages<'py>> {
    if release_gil {
        messages.load()
    } else {
        // SAFETY: the callers hash synchronously with the GIL held, and nothing between this
        // load and the last use of the slices runs Python code (the output is allocated or
        // validated first).
        unsafe { messages.load_borrowed(apart) }
    }
}

fn size_changed_error() -> PyErr {
    pyo3::exceptions::PyValueError::new_err("messages changed size while it was read")
}

// Fused read and hash of a big exact list or tuple straight into a fresh array; true when
// the array is filled. See `MessageSeq::hash_fused`.
fn fused_array<T, const W: usize>(
    out_arr: &Bound<'_, PyArray2<u8>>,
    messages: &MessageSeq<'_>,
    fused: fn(ListMessages, &mut [T]) -> bool,
) -> PyResult<bool> {
    let n = out_arr.shape()[0];
    let mut out_rw = out_arr.readwrite();
    let out_slice = out_rw.as_slice_mut().map_err(|_| {
        pyo3::exceptions::PyValueError::new_err(
            "internal error: freshly allocated output array is not contiguous",
        )
    })?;
    // SAFETY: T is a repr(transparent) wrapper over [u8; W], so align 1; the slice holds
    // n * W bytes.
    let blocks: &mut [T] =
        unsafe { std::slice::from_raw_parts_mut(out_slice.as_mut_ptr() as *mut T, n) };
    // SAFETY: the GIL is held throughout and the array is fresh, so it overlaps no item.
    Ok(unsafe { messages.hash_fused(blocks, fused) })
}

// Shared body of `hash{256,128}_many_var_into`.
fn var_into<T: Send, const W: usize>(
    py: Python<'_>,
    messages: MessageSeq<'_>,
    out: &Bound<'_, PyAny>,
    release_gil: bool,
    hash: fn(&[&[u8]], &mut [T]),
    fused: fn(ListMessages, &mut [T]) -> bool,
) -> PyResult<()> {
    require_numpy(py)?;
    let n = messages.len()?;
    let mut out = out_rows::<W>(out, n)?;
    if !release_gil {
        let mut done = false;
        write_direct::<T, W>(&mut out, n, |blocks| {
            // SAFETY: the GIL is held throughout; `out` is NumPy memory, the items are `bytes`.
            done = unsafe { messages.hash_fused(blocks, fused) };
        })?;
        if done {
            return Ok(());
        }
    }
    let apart = Some((out.data() as usize, n * W));
    let messages = load_messages(&messages, release_gil, apart)?;
    let rows = messages.slices();
    if rows.len() != n {
        return Err(size_changed_error());
    }
    if !release_gil {
        return write_direct::<T, W>(&mut out, n, |blocks| hash(rows, blocks));
    }
    digests_into::<T, W>(py, out, n, |o| hash(rows, o))
}

/// 256-bit digests of a sequence of bytes-like objects of any lengths. Returns a uint8
/// array of shape `(n, 32)`. TypeError for a non-sequence (a single string or buffer
/// included) or an item that is not a byte buffer.
/// `bytes` items are not copied; other buffers are copied. By default the GIL is released
/// while hashing. With `release_gil=False` it stays held and other Python threads wait for
/// the call; if `messages` is then exactly a `list` or `tuple` whose items are all exactly
/// `bytes` (no subclasses), no reference is taken on each item, which is faster for many
/// small messages. Any other input is handled as with the default.
#[pyfunction]
#[pyo3(name = "hash256_many_var", signature = (messages, *, release_gil=true))]
pub fn hash256_many_var_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = messages_arg)] messages: MessageSeq<'py>,
    release_gil: bool,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    require_numpy(py)?;
    let n = messages.len()?;
    // Allocated before the messages are read, so nothing runs between the read and the hash.
    let mut out_arr = alloc_out::<32>(py, n)?;
    if !release_gil && fused_array::<Hash256, 32>(&out_arr, &messages, hash256_many_var_blocks)? {
        return Ok(out_arr);
    }
    let messages = load_messages(&messages, release_gil, None)?;
    let rows = messages.slices();
    // `__len__` may disagree with the items actually read.
    if rows.len() != n {
        out_arr = alloc_out::<32>(py, rows.len())?;
    }
    digests256(py, out_arr, release_gil, |out| {
        hash256_many_var_into(rows, out)
    })
}

/// 128-bit digests of a sequence of bytes-like objects of any lengths. Same input rules
/// as `hash256_many_var`. Returns a uint8 array of shape `(n, 16)`.
#[pyfunction]
#[pyo3(name = "hash128_many_var", signature = (messages, *, release_gil=true))]
pub fn hash128_many_var_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = messages_arg)] messages: MessageSeq<'py>,
    release_gil: bool,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    require_numpy(py)?;
    let n = messages.len()?;
    let mut out_arr = alloc_out::<16>(py, n)?;
    if !release_gil && fused_array::<Hash128, 16>(&out_arr, &messages, hash128_many_var_blocks)? {
        return Ok(out_arr);
    }
    let messages = load_messages(&messages, release_gil, None)?;
    let rows = messages.slices();
    // `__len__` may disagree with the items actually read.
    if rows.len() != n {
        out_arr = alloc_out::<16>(py, rows.len())?;
    }
    digests128(py, out_arr, release_gil, |out| {
        hash128_many_var_into(rows, out)
    })
}

/// As `hash256_many_var`, writing the digests into `out`, a writable, C-contiguous uint8
/// array of shape `(len(messages), 32)` (ValueError for another shape or a non-contiguous
/// `out`, TypeError for another type, dtype or ndim). Returns None.
///
/// By default the digests are computed with the GIL released and copied into `out` once it
/// is held again. With `release_gil=False` the GIL stays held, the digests are written into
/// `out` directly and the faster read of an exact `list` or `tuple` of exact `bytes`
/// described at `hash256_many_var` applies; other Python threads wait for the call.
/// In that mode a big exact `list` or `tuple` (16384 items or more) is hashed while its
/// items are read, so `out` may be partly written when the call then fails for any reason:
/// a TypeError for an item that is not a byte buffer, a ValueError for an item that shares
/// memory with `out` (a `memoryview` of it, say), an error raised by an item's buffer
/// export, MemoryError. With the default `release_gil=True` nothing is written before all
/// items are read, and an item that shares memory with `out` is copied first and allowed.
#[pyfunction]
#[pyo3(name = "hash256_many_var_into", signature = (messages, out, *, release_gil=true))]
pub fn hash256_many_var_into_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = messages_arg)] messages: MessageSeq<'py>,
    out: &Bound<'_, PyAny>,
    release_gil: bool,
) -> PyResult<()> {
    var_into::<Hash256, 32>(
        py,
        messages,
        out,
        release_gil,
        hash256_many_var_into,
        hash256_many_var_blocks,
    )
}

/// As `hash256_many_var_into`, with 128-bit digests: `out` has shape `(len(messages), 16)`.
#[pyfunction]
#[pyo3(name = "hash128_many_var_into", signature = (messages, out, *, release_gil=true))]
pub fn hash128_many_var_into_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = messages_arg)] messages: MessageSeq<'py>,
    out: &Bound<'_, PyAny>,
    release_gil: bool,
) -> PyResult<()> {
    var_into::<Hash128, 16>(
        py,
        messages,
        out,
        release_gil,
        hash128_many_var_into,
        hash128_many_var_blocks,
    )
}
