//! Python bindings for Lustro XOF.
//! Output stream is derived from an absorbed message.

use numpy::{PyArray3, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyList};

use crate::types::StreamId;
use crate::xof::{LustroXof, LustroXofBatch};

// ==========================================
// XOF SINGLE API
// ==========================================

/// Deterministic output stream derived from an absorbed message.
///
/// One instance is one stream. An instance must not be used by several threads
/// at the same time: an overlapping call raises RuntimeError ("Already borrowed").
/// Use `fork()` or separate instances for independent streams.
#[pyclass(name = "LustroXof")]
pub struct LustroXofPy {
    inner: LustroXof,
}

#[pymethods]
impl LustroXofPy {
    /// `message` is a bytes object of any length, including empty.
    #[new]
    pub fn new(message: &[u8]) -> Self {
        Self {
            inner: LustroXof::new(message),
        }
    }

    /// Next 8 bytes of the stream as an unsigned integer.
    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    /// Next 16 bytes of the stream as an unsigned integer.
    pub fn next_u128(&mut self) -> u128 {
        self.inner.next_u128()
    }

    /// Next full 32-byte block. Unread bytes of the current block are discarded.
    pub fn next_block<'py>(&mut self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new_bound(py, &self.inner.next_block())
    }

    /// Returns the next `size` bytes of the stream. The GIL is released while generating.
    ///
    /// Raises OverflowError if `size` exceeds isize::MAX, MemoryError if the
    /// buffer cannot be allocated.
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
        .map_err(|e| {
            if e.is_instance_of::<pyo3::exceptions::PyMemoryError>(py) {
                crate::python::alloc_error()
            } else {
                e
            }
        })
    }

    /// Fills a writable, C-contiguous byte buffer (bytearray, writable memoryview,
    /// numpy uint8 array) in place with the next `len(buf)` bytes of the stream.
    /// The GIL is held while writing. Raises TypeError for any other object, a
    /// read-only or non-contiguous buffer, or a buffer whose items are not bytes
    /// (for other numpy dtypes pass `arr.view(numpy.uint8)`).
    pub fn fill_into(&mut self, buf: &Bound<'_, PyAny>) -> PyResult<()> {
        crate::python::fill_buffer(buf, |slice| self.inner.fill_bytes(slice))
    }

    /// Returns an independent XOF with identical stream state.
    pub fn copy(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }

    pub fn __copy__(&self) -> Self {
        self.copy()
    }

    pub fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.copy()
    }

    /// Derives a child XOF from the current state and `id`.
    /// The parent is not advanced.
    pub fn fork(&self, id: u128) -> Self {
        Self {
            inner: self.inner.fork(StreamId(id)),
        }
    }

    /// Derives an XOF from `message` along `path`:
    /// `LustroXof(message).fork(path[0]).fork(path[1])...`
    /// `path` must not be empty (ValueError).
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

    // State is not printed.
    pub fn __repr__(&self) -> String {
        "LustroXof(<redacted>)".to_string()
    }

    /// Exports the current state as 56 bytes.
    pub fn export_snapshot<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let snapshot = self.inner.export_snapshot();
        PyBytes::new_bound(py, &snapshot.to_le_bytes())
    }

    /// Restores an XOF from `export_snapshot()` bytes.
    /// Raises ValueError for a wrong length, version, kind or cursor.
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

/// Many independent XOF streams, advanced together.
///
/// Lane `i` is the same stream as `LustroXof(messages[i])`. Output is written by
/// `fill_blocks()`. The batch keeps an internal buffer the size of its largest
/// `fill_blocks()` call until it is dropped; `release_buffer()` frees it earlier.
/// An instance must not be used by several threads at the same time: an
/// overlapping call raises RuntimeError ("Already borrowed").
#[pyclass(name = "LustroXofBatch")]
pub struct LustroXofBatchPy {
    inner: LustroXofBatch,
    blocks_buf: Vec<[u8; 32]>,
}

#[pymethods]
impl LustroXofBatchPy {
    /// Creates a batch with one lane per message. `messages` is a list of bytes
    /// objects (TypeError for any other container or item type).
    #[new]
    pub fn new(messages: &Bound<'_, PyList>) -> PyResult<Self> {
        let items = crate::python::list_items(messages)?;
        let refs: Vec<&[u8]> = items.iter().map(|b| b.as_bytes()).collect();
        let inner = LustroXofBatch::new(&refs);
        Ok(Self {
            inner,
            blocks_buf: Vec::new(),
        })
    }

    /// Number of lanes.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// True if there are no lanes.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn __len__(&self) -> usize {
        self.inner.len()
    }

    // Length only; stream state is not printed.
    pub fn __repr__(&self) -> String {
        format!("LustroXofBatch(len={})", self.inner.len())
    }

    /// Returns an independent batch with identical lane states.
    pub fn copy(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            blocks_buf: Vec::new(),
        }
    }

    pub fn __copy__(&self) -> Self {
        self.copy()
    }

    pub fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.copy()
    }

    /// Frees the internal buffer kept by `fill_blocks()`. The next call allocates
    /// it again if needed. It does not change the state of the lanes.
    pub fn release_buffer(&mut self) {
        self.blocks_buf = Vec::new();
    }

    /// Suggested `steps` for `fill_blocks()` at the current `len()`.
    /// A speed hint: it does not change the output or the state.
    pub fn suggested_steps(&self) -> usize {
        self.inner.suggested_steps()
    }

    /// Advances every lane by `steps` stream steps and writes the blocks to `out`.
    ///
    /// `out` must be a writable uint64 array of shape `(steps, len(self), 4)`.
    /// Non-empty output must also be C-contiguous and aligned. Output is step-major:
    /// `out[s, i]` is block `s` of lane `i` (32 bytes as 4 little-endian uint64).
    /// The bytes do not depend on how steps are split across calls. The GIL is
    /// released while generating.
    ///
    /// Raises ValueError for a wrong shape, or for non-contiguous or misaligned
    /// non-empty output. TypeError is raised for a wrong type, dtype or ndim.
    /// Read-only or conflicting borrows raise the corresponding Python error.
    pub fn fill_blocks(
        &mut self,
        py: Python<'_>,
        out: &Bound<'_, PyAny>,
        steps: usize,
    ) -> PyResult<()> {
        let out = out.downcast::<PyArray3<u64>>().map_err(|_| {
            crate::python::array_type_error("out", "a 3D numpy.ndarray of dtype uint64")
        })?;
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
        crate::python::blocks_to_words(buf, flat);
        Ok(())
    }

    /// Derives one child per lane; `len(ids)` must equal `len(self)` (ValueError).
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

    /// Derives `len(ids)` children per lane. Parent-major order:
    /// child `j` of lane `i` is lane `i * len(ids) + j` of the result.
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

    /// One lane per message, each walked along `path`; lane `i` equals
    /// `LustroXof.derive_path(messages[i], path)`.
    /// `messages` is a list of bytes objects. `path` must not be empty (ValueError).
    /// Empty `messages` gives an empty batch.
    #[staticmethod]
    pub fn derive_path(messages: &Bound<'_, PyList>, path: Vec<u128>) -> PyResult<Self> {
        if path.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "path must not be empty",
            ));
        }
        let items = crate::python::list_items(messages)?;
        let refs: Vec<&[u8]> = items.iter().map(|b| b.as_bytes()).collect();
        let path_ids: Vec<StreamId> = path.into_iter().map(StreamId).collect();
        let inner = LustroXofBatch::derive_path(&refs, &path_ids);
        Ok(Self {
            inner,
            blocks_buf: Vec::new(),
        })
    }

    /// Derives one child per lane with ids `first`, `first + 1`, ... (modulo 2**128).
    pub fn fork_range(&self, first: u128) -> Self {
        let inner = self.inner.fork_range(StreamId(first));
        Self {
            inner,
            blocks_buf: Vec::new(),
        }
    }

    /// Exports the current state as `16 + len(self) * 48` bytes.
    pub fn export_snapshot<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let snapshot = self.inner.export_snapshot();
        PyBytes::new_bound(py, &snapshot.to_le_bytes())
    }

    /// Restores a batch from `export_snapshot()` bytes.
    /// Raises ValueError for a wrong length, version, kind or lane count.
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
