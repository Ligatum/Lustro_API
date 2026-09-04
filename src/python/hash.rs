//! Python bindings for Lustro Hash.

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use numpy::{PyArray2, PyArrayMethods, PyReadonlyArray2};

use crate::types::{Hash128, Hash256};
use crate::hash::hash256 as hash256;
use crate::hash::hash128 as hash128;
use crate::hash::hash256_many_into as hash256_many_into;
use crate::hash::hash128_many_into as hash128_many_into;

// ==========================================
// HASH API
// ==========================================

#[pyclass]
pub struct LustroHashPy;

#[pymethods]
impl LustroHashPy {
    #[new]
    pub fn new() -> Self { Self }

    pub fn hash256<'py>(&self, py: Python<'py>, data: &[u8])
        -> Bound<'py, PyBytes>
    {
        let result = py.allow_threads(|| hash256(data));
        PyBytes::new_bound(py, result.as_bytes())
    }

    pub fn hash128<'py>(&self, py: Python<'py>, data: &[u8])
        -> Bound<'py, PyBytes>
    {
        let result = py.allow_threads(|| hash128(data));
        PyBytes::new_bound(py, result.as_bytes())
    }

// ==========================================
// HASH BATCH API
// ==========================================

    // Writes digests directly into the output array buffer.
    pub fn hash256_many<'py>(
        &self,
        py: Python<'py>,
        data: PyReadonlyArray2<'_, u8>,
    ) -> PyResult<Bound<'py, PyArray2<u8>>> {
        let data_arr = data.as_array();
        let n = data_arr.shape()[0];

        let mut rows: Vec<&[u8]> = Vec::with_capacity(n);
        for r in data_arr.rows() {
            let slice = r.to_slice().ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(
                    "data must be a C-contiguous array (got a non-contiguous view, \
                     e.g. from slicing or transposing — call np.ascontiguousarray() first)"
                )
            })?;
            rows.push(slice);
        }

        let out_arr = PyArray2::<u8>::zeros_bound(py, [n, 32], false);
        {
            let mut out_rw = out_arr.readwrite();
            let out_slice = out_rw.as_slice_mut().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err(
                    "internal error: freshly allocated output array is not contiguous"
                )
            })?;
            // SAFETY: Hash256 is repr(transparent) over [u8; 32].
            let out_blocks: &mut [Hash256] = unsafe {
                std::slice::from_raw_parts_mut(out_slice.as_mut_ptr() as *mut Hash256, n)
            };

            py.allow_threads(|| hash256_many_into(&rows, out_blocks));
        }

        Ok(out_arr)
    }

    // Writes digests directly into the output array buffer.
    pub fn hash128_many<'py>(
        &self,
        py: Python<'py>,
        data: PyReadonlyArray2<'_, u8>,
    ) -> PyResult<Bound<'py, PyArray2<u8>>> {
        let data_arr = data.as_array();
        let n = data_arr.shape()[0];

        let mut rows: Vec<&[u8]> = Vec::with_capacity(n);
        for r in data_arr.rows() {
            let slice = r.to_slice().ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(
                    "data must be a C-contiguous array (got a non-contiguous view, \
                     e.g. from slicing or transposing — call np.ascontiguousarray() first)"
                )
            })?;
            rows.push(slice);
        }

        let out_arr = PyArray2::<u8>::zeros_bound(py, [n, 16], false);
        {
            let mut out_rw = out_arr.readwrite();
            let out_slice = out_rw.as_slice_mut().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err(
                    "internal error: freshly allocated output array is not contiguous"
                )
            })?;
            // SAFETY: Hash128 is repr(transparent) over [u8; 16].
            let out_blocks: &mut [Hash128] = unsafe {
                std::slice::from_raw_parts_mut(out_slice.as_mut_ptr() as *mut Hash128, n)
            };

            py.allow_threads(|| hash128_many_into(&rows, out_blocks));
        }

        Ok(out_arr)
    }
}