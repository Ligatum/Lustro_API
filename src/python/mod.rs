//! Lustro V1 PYTHON interface.

pub mod hash;
pub mod prng;
pub mod xof;

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyList};

// PyO3 turns panics into exceptions only when unwinding.
#[cfg(panic = "abort")]
compile_error!("lustro Python bindings require panic = \"unwind\"");

/// Version of the native API (integer), independent of `__version__`.
#[pyfunction]
pub fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

// Maps SnapshotError to a PyValueError (message from Display).
pub(crate) fn snapshot_error(err: crate::types::SnapshotError) -> pyo3::PyErr {
    pyo3::exceptions::PyValueError::new_err(err.to_string())
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

// Keep the bytes alive while the GIL is released (the list may change meanwhile).
pub(crate) fn list_items<'py>(messages: &Bound<'py, PyList>) -> PyResult<Vec<Bound<'py, PyBytes>>> {
    messages
        .iter()
        .map(|item| {
            item.downcast_into::<PyBytes>().map_err(|_| {
                pyo3::exceptions::PyTypeError::new_err("messages must be a list of bytes")
            })
        })
        .collect()
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
