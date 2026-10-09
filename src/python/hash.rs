//! Python bindings for Lustro Hash.

use numpy::ndarray::ArrayView2;
use numpy::{PyArray2, PyArrayMethods};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyList};

use crate::hash::hash128 as hash128_impl;
use crate::hash::hash128_many_into;
use crate::hash::hash256 as hash256_impl;
use crate::hash::hash256_many_into;
use crate::python::list_items;
use crate::types::{Hash128, Hash256};

// ==========================================
// INPUT HELPERS
// ==========================================

// Rows copied into one buffer; `rows()` gives per-message slices.
struct FlatRows {
    buf: Vec<u8>,
    n: usize,
    len: usize,
}

impl FlatRows {
    fn rows(&self) -> Vec<&[u8]> {
        // chunks_exact(0) panics
        if self.len == 0 {
            return vec![&[][..]; self.n];
        }
        self.buf.chunks_exact(self.len).collect()
    }
}

// Own copy — can't hold a &[u8] into NumPy's buffer across allow_threads().
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
    data.downcast::<PyArray2<u8>>()
        .map_err(|_| crate::python::array_type_error("data", "a 2D numpy.ndarray of dtype uint8"))
}

fn digests256<'py>(py: Python<'py>, rows: &[&[u8]]) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let n = rows.len();
    let out_arr = PyArray2::<u8>::zeros_bound(py, [n, 32], false);
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

        py.allow_threads(|| hash256_many_into(rows, out_blocks));
    }
    Ok(out_arr)
}

// Digests go straight into the output array buffer.
fn digests128<'py>(py: Python<'py>, rows: &[&[u8]]) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let n = rows.len();
    let out_arr = PyArray2::<u8>::zeros_bound(py, [n, 16], false);
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

        py.allow_threads(|| hash128_many_into(rows, out_blocks));
    }
    Ok(out_arr)
}

// ==========================================
// HASH API
// ==========================================

/// 256-bit digest of `data` (bytes of any length) as 32 bytes.
/// The GIL is released while hashing.
#[pyfunction]
#[pyo3(name = "hash256")]
pub fn hash256_py<'py>(py: Python<'py>, data: &[u8]) -> Bound<'py, PyBytes> {
    let result = py.allow_threads(|| hash256_impl(data));
    PyBytes::new_bound(py, result.as_bytes())
}

/// 128-bit digest of `data` (bytes of any length) as 16 bytes.
/// The GIL is released while hashing.
#[pyfunction]
#[pyo3(name = "hash128")]
pub fn hash128_py<'py>(py: Python<'py>, data: &[u8]) -> Bound<'py, PyBytes> {
    let result = py.allow_threads(|| hash128_impl(data));
    PyBytes::new_bound(py, result.as_bytes())
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
#[pyo3(name = "hash256_many")]
pub fn hash256_many_py<'py>(
    py: Python<'py>,
    data: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let data = rows_array(data)?;
    // Use the non-panicking borrow API so conflicting NumPy borrows become Python errors.
    let data = data.try_readonly()?;
    let flat = copy_rows(data.as_array()).ok_or_else(non_contiguous_error)?;
    digests256(py, &flat.rows())
}

/// 128-bit digests of the rows of a 2D uint8 array. Same input rules as
/// `hash256_many`. Returns a uint8 array of shape `(n, 16)`.
#[pyfunction]
#[pyo3(name = "hash128_many")]
pub fn hash128_many_py<'py>(
    py: Python<'py>,
    data: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let data = rows_array(data)?;
    let data = data.try_readonly()?;
    let flat = copy_rows(data.as_array()).ok_or_else(non_contiguous_error)?;
    digests128(py, &flat.rows())
}

// ==========================================
// HASH BATCH API (VARIABLE LENGTH)
// ==========================================

/// 256-bit digests of a list of bytes objects of any lengths. Returns a uint8
/// array of shape `(n, 32)`. TypeError for a non-list or a non-bytes item.
/// The messages are not copied; the GIL is released while hashing.
#[pyfunction]
#[pyo3(name = "hash256_many_var")]
pub fn hash256_many_var_py<'py>(
    py: Python<'py>,
    messages: &Bound<'py, PyList>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let items = list_items(messages)?;
    let rows: Vec<&[u8]> = items.iter().map(|b| b.as_bytes()).collect();
    digests256(py, &rows)
}

/// 128-bit digests of a list of bytes objects of any lengths. Same input rules
/// as `hash256_many_var`. Returns a uint8 array of shape `(n, 16)`.
#[pyfunction]
#[pyo3(name = "hash128_many_var")]
pub fn hash128_many_var_py<'py>(
    py: Python<'py>,
    messages: &Bound<'py, PyList>,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    let items = list_items(messages)?;
    let rows: Vec<&[u8]> = items.iter().map(|b| b.as_bytes()).collect();
    digests128(py, &rows)
}
