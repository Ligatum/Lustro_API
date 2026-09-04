//! Python bindings for Lustro PRNG.
//! Each instance is an independent stream.

use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes};
use numpy::{PyReadwriteArray2, PyReadwriteArray3, PyUntypedArrayMethods};

use crate::prng::LustroPrng;
use crate::types::{Seed256, StreamId};
use crate::prng::LustroPrngBatch;

// ==========================================
// STREAM SINGLE API
// ==========================================

#[pyclass]
pub struct LustroPrngPy {
    inner: LustroPrng,
}

#[pymethods]
impl LustroPrngPy {

    #[new]
    pub fn new(seed: &[u8], stream_id: u128) -> PyResult<Self> {
        if seed.len() != 32 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "seed must be exactly 32 bytes"
            ));
        }
        let seed256 = Seed256::from_bytes(seed.try_into().unwrap());
        Ok(Self { inner: LustroPrng::new(&seed256, StreamId(stream_id)) })
    }

    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    pub fn next_u128(&mut self) -> u128 {
        self.inner.next_u128()
    }

    pub fn next_block<'py>(&mut self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new_bound(py, &self.inner.next_block())
    }

    // Returns `size` random bytes.
    pub fn fill<'py>(&mut self, py: Python<'py>, size: usize) -> Bound<'py, PyBytes> {
        let mut buf = vec![0u8; size];
        py.allow_threads(|| self.inner.fill_bytes(&mut buf));
        PyBytes::new_bound(py, &buf)
    }

    // Fills an existing bytearray in-place without allocation.
    pub fn fill_into(&mut self, _py: Python<'_>, buf: &Bound<'_, PyByteArray>) {
        // SAFETY: GIL prevents concurrent resize or drop of the buffer.
        let slice = unsafe { buf.as_bytes_mut() };
        self.inner.fill_bytes(slice);
    }

    // Returns a generator with identical stream state.
    pub fn clone_rng(&self) -> Self {
        Self { inner: self.inner.clone() }
    }

    // Derives a child generator from the current state and identifier.
    pub fn fork(&self, id: u128) -> Self {
        Self { inner: self.inner.fork(StreamId(id)) }
    }

    pub fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }

    // Exports the current snapshot as 56 bytes.
    pub fn export_snapshot<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let snapshot = self.inner.export_snapshot();
        PyBytes::new_bound(py, &snapshot.to_le_bytes())
    }

    /// Restores a generator from snapshot bytes.
    #[staticmethod]
    pub fn import_snapshot(bytes: &[u8]) -> PyResult<Self> {
        let array_ref: &[u8; 56] = bytes.try_into().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("snapshot must be exactly 56 bytes")
        })?;

        let snapshot = crate::types::LustroPrngSnapshot::from_le_bytes(array_ref)
            .map_err(crate::python::snapshot_error)?;

        Ok(Self { inner: LustroPrng::import_snapshot(snapshot) })
    }
}

// ==========================================
// STREAM BATCH API
// ==========================================

#[pyclass]
pub struct LustroPrngBatchPy {
    inner: LustroPrngBatch,
    blocks_buf: Vec<[u8; 32]>,
    blocks_buf_many: Vec<[u8; 32]>,
}

#[pymethods]
impl LustroPrngBatchPy {

    // Creates a batch from explicit stream identifiers.
    #[staticmethod]
    pub fn new(seed: &[u8], stream_ids: Vec<u128>) -> PyResult<Self> {
        if seed.len() != 32 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "seed must be exactly 32 bytes"
            ));
        }
        let seed256 = Seed256::from_bytes(seed.try_into().unwrap());
        let ids: Vec<StreamId> = stream_ids.into_iter().map(StreamId).collect();
        let count = ids.len();
        let inner = LustroPrngBatch::new(&seed256, &ids);
        Ok(Self {
            inner,
            blocks_buf: vec![[0u8; 32]; count],
            blocks_buf_many: Vec::new(),
        })
    }

    #[staticmethod]
    pub fn new_range(seed: &[u8], first_stream_id: u128, count: usize) -> PyResult<Self> {
        if seed.len() != 32 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "seed must be exactly 32 bytes"
            ));
        }
        let seed256 = Seed256::from_bytes(seed.try_into().unwrap());
        let inner = LustroPrngBatch::new_range(&seed256, StreamId(first_stream_id), count);
        Ok(Self {
            inner,
            blocks_buf: vec![[0u8; 32]; count],
            blocks_buf_many: Vec::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn fill_blocks(
        &mut self,
        py: Python<'_>,
        mut out: PyReadwriteArray2<'_, u64>,
    ) -> PyResult<()> {
        let n = self.inner.len();
        if out.shape() != [n, 4] {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "out must have shape (n_streams, 4)"
            ));
        }

        py.allow_threads(|| self.inner.fill_blocks(&mut self.blocks_buf));

        let mut out_arr = out.as_array_mut();
        let flat = out_arr.as_slice_mut().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(
                "out must be a C-contiguous array (got a non-contiguous view, \
                 e.g. from slicing or transposing — call np.ascontiguousarray() first)"
            )
        })?;
        for (block, words) in self.blocks_buf.iter().zip(flat.chunks_exact_mut(4)) {
            words[0] = u64::from_le_bytes(block[0..8].try_into().unwrap());
            words[1] = u64::from_le_bytes(block[8..16].try_into().unwrap());
            words[2] = u64::from_le_bytes(block[16..24].try_into().unwrap());
            words[3] = u64::from_le_bytes(block[24..32].try_into().unwrap());
        }
        Ok(())
    }

    // Advances all streams by `steps` rounds.
    // Output shape: `(n_streams, steps, 4)`.
    pub fn fill_blocks_many(
        &mut self,
        py: Python<'_>,
        steps: usize,
        mut out: PyReadwriteArray3<'_, u64>,
    ) -> PyResult<()> {
        let n = self.inner.len();
        if out.shape() != [n, steps, 4] {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "out must have shape (n_streams, steps, 4)"
            ));
        }

        let needed = n * steps;
        if self.blocks_buf_many.len() < needed {
            self.blocks_buf_many.resize(needed, [0u8; 32]);
        }
        let buf = &mut self.blocks_buf_many[..needed];

        py.allow_threads(|| self.inner.fill_blocks_many(buf, steps));

        // Stream-major layout matches the C-contiguous output layout.
        let mut out_arr = out.as_array_mut();
        let flat = out_arr.as_slice_mut().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(
                "out must be a C-contiguous array (got a non-contiguous view, \
                 e.g. from slicing or transposing — call np.ascontiguousarray() first)"
            )
        })?;
        for (block, words) in buf.iter().zip(flat.chunks_exact_mut(4)) {
            words[0] = u64::from_le_bytes(block[0..8].try_into().unwrap());
            words[1] = u64::from_le_bytes(block[8..16].try_into().unwrap());
            words[2] = u64::from_le_bytes(block[16..24].try_into().unwrap());
            words[3] = u64::from_le_bytes(block[24..32].try_into().unwrap());
        }
        Ok(())
    }

    // Derives one child generator per lane.
    // `ids.len()` must equal `len()`.
    pub fn fork(&self, ids: Vec<u128>) -> PyResult<Self> {
        if ids.len() != self.inner.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "ids length must match batch stream count"
            ));
        }
        let stream_ids: Vec<StreamId> = ids.into_iter().map(StreamId).collect();
        let inner = self.inner.fork(&stream_ids);
        let count = inner.len();
        Ok(Self {
            inner,
            blocks_buf: vec![[0u8; 32]; count],
            blocks_buf_many: Vec::new(),
        })
    }

    // Derives sequential child identifiers starting at `first`.
    pub fn fork_range(&self, first: u128) -> Self {
        let inner = self.inner.fork_range(StreamId(first));
        let count = inner.len();
        Self {
            inner,
            blocks_buf: vec![[0u8; 32]; count],
            blocks_buf_many: Vec::new(),
        }
    }

    // Exports the current batch snapshot.
    // Length: `16 + len() * 48` bytes.
    pub fn export_snapshot<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let snapshot = self.inner.export_snapshot();
        PyBytes::new_bound(py, &snapshot.to_le_bytes())
    }

    /// Restores a batch from snapshot bytes.
    #[staticmethod]
    pub fn import_snapshot(bytes: &[u8]) -> PyResult<Self> {
        let snapshot = crate::types::LustroPrngBatchSnapshot::from_le_bytes(bytes)
            .map_err(crate::python::snapshot_error)?;

        let inner = LustroPrngBatch::import_snapshot(snapshot);
        let count = inner.len();
        Ok(Self {
            inner,
            blocks_buf: vec![[0u8; 32]; count],
            blocks_buf_many: Vec::new(),
        })
    }
}