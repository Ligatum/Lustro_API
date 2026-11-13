//! Python bindings: NumPy output helpers.
//! New uint64 arrays for stream output and the block/word conversions.

use numpy::{Ix1, Ix3, PyArray, PyArray1, PyArray3, PyArrayDescrMethods, PyArrayMethods};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;

// A (steps, n, 4) uint64 output seen as blocks, for writing without the intermediate copy.
// Little-endian only: elsewhere the words need `blocks_to_words`.
#[cfg(target_endian = "little")]
pub(crate) fn words_as_blocks(words: &mut [u64]) -> &mut [[u8; 32]] {
    debug_assert_eq!(words.len() % 4, 0);
    // SAFETY: [u8; 32] has align 1 and the byte length is the same; any bit pattern is a
    // valid u8.
    unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut [u8; 32], words.len() / 4) }
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
    static IMPORTED: PyOnceLock<()> = PyOnceLock::new();
    IMPORTED
        .get_or_try_init(py, || numpy::get_array_module(py).map(|_| ()))
        .map(|_| ())
        .map_err(|e| {
            if e.is_instance_of::<pyo3::exceptions::PyImportError>(py) {
                pyo3::exceptions::PyImportError::new_err(
                    "this function needs NumPy (pip install numpy)",
                )
            } else {
                e
            }
        })
}

// New zeroed C-contiguous array of the given shape. `PyArray::zeros` panics when NumPy fails
// and `PyArray::from_vec` does not check the allocation; this calls `PyArray_Zeros` directly
// and passes a failure on as the exception NumPy set (MemoryError). The array owns its memory,
// so the stream is written straight into it.
fn zeroed<'py, T: numpy::Element, D: numpy::ndarray::Dimension>(
    py: Python<'py>,
    dims: &mut [numpy::npyffi::npy_intp],
) -> PyResult<Bound<'py, PyArray<T, D>>> {
    // SAFETY: the GIL is held; `dims` is valid for `dims.len()` entries; `PyArray_Zeros` takes
    // over the dtype reference and returns a new reference or NULL with an exception set.
    unsafe {
        let ptr = numpy::npyffi::PY_ARRAY_API.PyArray_Zeros(
            py,
            dims.len() as std::os::raw::c_int,
            dims.as_mut_ptr(),
            T::get_dtype(py).into_dtype_ptr(),
            0,
        );
        Ok(Bound::from_owned_ptr_or_err(py, ptr)?.cast_into_unchecked())
    }
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
// The size check, the NumPy check and the array allocation come before `fill`, so their
// failures leave the state untouched. `fill` writes into the new array, which nothing else
// references yet. The GIL is released while `fill` runs, from 4096 bytes.
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
    let arr = zeroed::<u64, Ix1>(py, &mut [n as numpy::npyffi::npy_intp])?;
    if n != 0 {
        let ptr = arr.data();
        // SAFETY: a new C-contiguous array holds `n` consecutive aligned u64 and nothing else
        // references it, so nothing reads or writes it while the GIL is released; any bit
        // pattern is a valid u8 and u64.
        let bytes = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, n * 8) };
        if n * 8 < crate::python::DETACH_MIN {
            fill(bytes);
        } else {
            py.detach(|| fill(bytes));
        }
        // SAFETY: as above; `bytes` is no longer used.
        words_from_le(unsafe { std::slice::from_raw_parts_mut(ptr, n) });
    }
    Ok(arr)
}

// New uint64 array of shape `(steps, lanes, 4)` laid out as `fill_blocks()` writes it.
// The size checks, the NumPy check and the array allocation come before `fill`, so their
// failures leave the state untouched. `fill` writes into the new array, which nothing else
// references yet. The GIL is released while `fill` runs.
pub(crate) fn blocks_array<'py>(
    py: Python<'py>,
    lanes: usize,
    steps: usize,
    fill: impl FnOnce(&mut [[u8; 32]]) + Send,
) -> PyResult<Bound<'py, PyArray3<u64>>> {
    let too_big =
        || pyo3::exceptions::PyOverflowError::new_err("output size exceeds isize::MAX bytes");
    if steps > isize::MAX as usize {
        return Err(too_big());
    }
    let blocks = lanes.checked_mul(steps).ok_or_else(|| {
        pyo3::exceptions::PyOverflowError::new_err("n_streams * steps overflows usize")
    })?;
    if blocks
        .checked_mul(32)
        .is_none_or(|b| b > isize::MAX as usize)
    {
        return Err(too_big());
    }
    require_numpy(py)?;
    let dim = |v: usize| numpy::npyffi::npy_intp::try_from(v).map_err(|_| too_big());
    let mut dims = [dim(steps)?, dim(lanes)?, 4];
    // Besides MemoryError, NumPy refuses a shape it cannot hold with ValueError (an empty batch
    // with a very large `steps`).
    let arr = zeroed::<u64, Ix3>(py, &mut dims).map_err(|e| {
        if e.is_instance_of::<pyo3::exceptions::PyValueError>(py) {
            too_big()
        } else {
            e
        }
    })?;
    if blocks != 0 {
        let ptr = arr.data();
        // SAFETY: a new C-contiguous array holds `blocks * 4` consecutive aligned u64 and nothing
        // else references it, so nothing reads or writes it while the GIL is released;
        // `[u8; 32]` has alignment 1 and any bit pattern is valid.
        let view = unsafe { std::slice::from_raw_parts_mut(ptr as *mut [u8; 32], blocks) };
        py.detach(|| fill(view));
        // SAFETY: as above; `view` is no longer used.
        words_from_le(unsafe { std::slice::from_raw_parts_mut(ptr, blocks * 4) });
    }
    Ok(arr)
}
