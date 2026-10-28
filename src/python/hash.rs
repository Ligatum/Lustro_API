//! Python bindings for Lustro Hash.

use numpy::ndarray::ArrayView2;
use numpy::{PyArray2, PyArrayMethods};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::borrow::Cow;

use crate::hash::hash128 as hash128_impl;
use crate::hash::hash128_many_fixed_into;
use crate::hash::hash128_many_var_into;
use crate::hash::hash256 as hash256_impl;
use crate::hash::hash256_many_fixed_into;
use crate::hash::hash256_many_var_into;
use crate::python::{bytes_arg, messages_arg, require_numpy, MessageSeq};
use crate::types::{Hash128, Hash256};

// ==========================================
// INPUT HELPERS
// ==========================================

// Rows copied into one buffer: `n` messages of `len` bytes, back to back.
struct FlatRows {
    buf: Vec<u8>,
    n: usize,
    len: usize,
}

// Own copy — can't hold a &[u8] into NumPy's buffer across detach().
// Row-contiguous views (a[::2], a[:, :16]) are accepted, `None` if any row is not.
fn copy_rows(view: ArrayView2<u8>) -> Option<FlatRows> {
    let (n, len) = view.dim();
    let buf = match view.as_slice() {
        Some(s) => s.to_vec(),
        None => {
            let mut b = Vec::with_capacity(view.len());
            for r in view.rows() {
                b.extend_from_slice(r.to_slice()?);
            }
            b
        }
    };
    Some(FlatRows { buf, n, len })
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

fn digests256<'py>(
    py: Python<'py>,
    n: usize,
    run: impl FnOnce(&mut [Hash256]) + Send,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let out_arr = PyArray2::<u8>::zeros(py, [n, 32], false);
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

        py.detach(|| run(out_blocks));
    }
    Ok(out_arr)
}

// Digests go straight into the output array buffer.
fn digests128<'py>(
    py: Python<'py>,
    n: usize,
    run: impl FnOnce(&mut [Hash128]) + Send,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let out_arr = PyArray2::<u8>::zeros(py, [n, 16], false);
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

        py.detach(|| run(out_blocks));
    }
    Ok(out_arr)
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
    let flat = copy_rows(data.as_array()).ok_or_else(non_contiguous_error)?;
    digests256(py, flat.n, |out| {
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
    let flat = copy_rows(data.as_array()).ok_or_else(non_contiguous_error)?;
    digests128(py, flat.n, |out| {
        hash128_many_fixed_into(&flat.buf, flat.len, out)
    })
}

// ==========================================
// HASH BATCH API (VARIABLE LENGTH)
// ==========================================

/// 256-bit digests of a sequence of bytes-like objects of any lengths. Returns a uint8
/// array of shape `(n, 32)`. TypeError for a non-sequence (a single string or buffer
/// included) or an item that is not a byte buffer.
/// `bytes` items are not copied; other buffers are copied. The GIL is released while hashing.
#[pyfunction]
#[pyo3(name = "hash256_many_var")]
pub fn hash256_many_var_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = messages_arg)] messages: MessageSeq<'py>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    require_numpy(py)?;
    let messages = messages.load()?;
    let rows = messages.slices();
    digests256(py, rows.len(), |out| hash256_many_var_into(&rows, out))
}

/// 128-bit digests of a sequence of bytes-like objects of any lengths. Same input rules
/// as `hash256_many_var`. Returns a uint8 array of shape `(n, 16)`.
#[pyfunction]
#[pyo3(name = "hash128_many_var")]
pub fn hash128_many_var_py<'py>(
    py: Python<'py>,
    #[pyo3(from_py_with = messages_arg)] messages: MessageSeq<'py>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    require_numpy(py)?;
    let messages = messages.load()?;
    let rows = messages.slices();
    digests128(py, rows.len(), |out| hash128_many_var_into(&rows, out))
}
