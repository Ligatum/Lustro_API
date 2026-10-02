//! Python bindings for Lustro XOF.
//! Output stream is derived from an absorbed message.

use numpy::{PyArray3, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes};

use crate::types::StreamId;
use crate::xof::{LustroXof, LustroXofBatch};

// ==========================================
// XOF SINGLE API
// ==========================================

#[pyclass]
pub struct LustroXofPy {
    inner: LustroXof,
}

#[pymethods]
impl LustroXofPy {
    #[new]
    pub fn new(message: &[u8]) -> Self {
        Self {
            inner: LustroXof::new(message),
        }
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

    // Returns `size` output bytes.
    // Raises OverflowError if `size` exceeds isize::MAX, MemoryError if the
    // buffer cannot be allocated.
    pub fn fill<'py>(&mut self, py: Python<'py>, size: usize) -> PyResult<Bound<'py, PyBytes>> {
        if size > isize::MAX as usize {
            return Err(pyo3::exceptions::PyOverflowError::new_err(
                "size exceeds isize::MAX",
            ));
        }
        PyBytes::new_bound_with(py, size, |buf| {
            py.allow_threads(|| self.inner.fill_bytes(buf));
            Ok(())
        })
    }

    // Fills an existing bytearray in-place without allocation.
    pub fn fill_into(&mut self, _py: Python<'_>, buf: &Bound<'_, PyByteArray>) {
        // SAFETY: GIL prevents concurrent resize or drop of the buffer.
        let slice = unsafe { buf.as_bytes_mut() };
        self.inner.fill_bytes(slice);
    }

    // Returns an XOF with identical stream state.
    pub fn clone_xof(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }

    // Derives a child XOF from the current state and identifier.
    pub fn fork(&self, id: u128) -> Self {
        Self {
            inner: self.inner.fork(StreamId(id)),
        }
    }

    // Derives an XOF from `message` along `path`.
    // `path` must not be empty.
    #[staticmethod]
    pub fn derive_path(message: &[u8], path: Vec<u128>) -> PyResult<Self> {
        if path.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "path must not be empty",
            ));
        }
        let ids: Vec<StreamId> = path.into_iter().map(StreamId).collect();
        Ok(Self {
            inner: LustroXof::derive_path(message, &ids),
        })
    }

    pub fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }

    // Exports the current snapshot as 56 bytes.
    pub fn export_snapshot<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let snapshot = self.inner.export_snapshot();
        PyBytes::new_bound(py, &snapshot.to_le_bytes())
    }

    // Restores a generator from snapshot bytes.
    #[staticmethod]
    pub fn import_snapshot(bytes: &[u8]) -> PyResult<Self> {
        let array_ref: &[u8; 56] = bytes.try_into().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("snapshot must be exactly 56 bytes")
        })?;
        let snapshot = crate::types::LustroXofSnapshot::from_le_bytes(array_ref)
            .map_err(crate::python::snapshot_error)?;

        Ok(Self {
            inner: LustroXof::import_snapshot(snapshot),
        })
    }
}

// ==========================================
// XOF BATCH API
// ==========================================

#[pyclass]
pub struct LustroXofBatchPy {
    inner: LustroXofBatch,
    blocks_buf: Vec<[u8; 32]>,
}

#[pymethods]
impl LustroXofBatchPy {
    #[staticmethod]
    pub fn new(messages: Vec<Vec<u8>>) -> Self {
        let refs: Vec<&[u8]> = messages.iter().map(Vec::as_slice).collect();
        let inner = LustroXofBatch::new(&refs);
        Self {
            inner,
            blocks_buf: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    // Advances all streams by `steps` rounds.
    // Output is step-major, shape `(steps, n_streams, 4)`.
    pub fn fill_blocks(
        &mut self,
        py: Python<'_>,
        out: &Bound<'_, PyArray3<u64>>,
        steps: usize,
    ) -> PyResult<()> {
        // Read before borrowing: `try_readwrite` itself does not touch the data.
        let misaligned = (out.data() as usize) % std::mem::align_of::<u64>() != 0;
        // Use the non-panicking mutable borrow API so read-only or conflicting borrows become Python errors.
        let mut out = out.try_readwrite()?;
        let n = self.inner.len();
        if out.shape() != [steps, n, 4] {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "out must have shape (steps, n_streams, 4)",
            ));
        }

        let needed = n.checked_mul(steps).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("n_streams * steps overflows usize")
        })?;

        // Empty output: nothing to write. NumPy reports zero strides here.
        if needed == 0 {
            return Ok(());
        }

        // A misaligned buffer must not become a `&mut [u64]`.
        if misaligned {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "out must be aligned for uint64 (got a misaligned buffer, \
                 e.g. from np.frombuffer with an odd offset — copy it first)",
            ));
        }

        // Validate contiguity first; the scope drops the borrow before allow_threads.
        {
            let mut out_arr = out.as_array_mut();
            if out_arr.as_slice_mut().is_none() {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "out must be a C-contiguous array (got a non-contiguous view, \
                     e.g. from slicing or transposing — call np.ascontiguousarray() first)",
                ));
            }
        }

        if self.blocks_buf.len() < needed {
            let additional = needed - self.blocks_buf.len();
            self.blocks_buf
                .try_reserve_exact(additional)
                .map_err(|_| crate::python::alloc_error())?;
            self.blocks_buf.resize(needed, [0u8; 32]);
        }
        let buf = &mut self.blocks_buf[..needed];

        py.allow_threads(|| self.inner.fill_blocks(buf, steps));

        // Step-major blocks map 1:1 onto the C-contiguous (steps, n, 4) output.
        // Re-borrow the output after the GIL is reacquired.
        let mut out_arr = out.as_array_mut();
        let flat = out_arr.as_slice_mut().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(
                "out must be a C-contiguous array (got a non-contiguous view, \
                 e.g. from slicing or transposing — call np.ascontiguousarray() first)",
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

    // Derives one child XOF per lane.
    // `ids.len()` must equal `len()`.
    pub fn fork(&self, ids: Vec<u128>) -> PyResult<Self> {
        if ids.len() != self.inner.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "ids length must match batch stream count",
            ));
        }
        let stream_ids: Vec<StreamId> = ids.into_iter().map(StreamId).collect();
        let inner = self.inner.fork(&stream_ids);
        Ok(Self {
            inner,
            blocks_buf: Vec::new(),
        })
    }

    // Derives `len(ids)` children per lane. Output is parent-major:
    // child `j` of lane `i` is at index `i * len(ids) + j`.
    pub fn fork_many(&self, ids: Vec<u128>) -> PyResult<Self> {
        self.inner.len().checked_mul(ids.len()).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("fork_many: len() * len(ids) overflows")
        })?;
        let stream_ids: Vec<StreamId> = ids.into_iter().map(StreamId).collect();
        let inner = self
            .inner
            .try_fork_many(&stream_ids)
            .map_err(|_| crate::python::alloc_error())?;
        Ok(Self {
            inner,
            blocks_buf: Vec::new(),
        })
    }

    // Derives a canonical batch: one lane per message, each walked along `path`.
    // `path` must not be empty. Empty `messages` produces an empty batch.
    #[staticmethod]
    pub fn derive_path(messages: Vec<Vec<u8>>, path: Vec<u128>) -> PyResult<Self> {
        if path.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "path must not be empty",
            ));
        }
        let refs: Vec<&[u8]> = messages.iter().map(Vec::as_slice).collect();
        let path_ids: Vec<StreamId> = path.into_iter().map(StreamId).collect();
        let inner = LustroXofBatch::derive_path(&refs, &path_ids);
        Ok(Self {
            inner,
            blocks_buf: Vec::new(),
        })
    }

    // Derives sequential child identifiers starting at `first`.
    pub fn fork_range(&self, first: u128) -> Self {
        let inner = self.inner.fork_range(StreamId(first));
        Self {
            inner,
            blocks_buf: Vec::new(),
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
        let snapshot = crate::types::LustroXofBatchSnapshot::from_le_bytes(bytes)
            .map_err(crate::python::snapshot_error)?;

        let inner = LustroXofBatch::import_snapshot(snapshot);
        Ok(Self {
            inner,
            blocks_buf: Vec::new(),
        })
    }
}
