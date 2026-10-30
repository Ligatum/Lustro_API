//! Python bindings: NumPy output helpers.
//! New uint64 arrays for stream output and the block/word conversions.

use super::alloc_error;
use numpy::{PyArray1, PyArray3, PyArrayMethods};
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
        py.detach(|| fill(bytes));
        words_from_le(&mut words);
    }
    Ok(PyArray1::from_vec(py, words))
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
        py.detach(|| fill(view));
        words_from_le(&mut words);
    }
    PyArray1::from_vec(py, words).reshape([steps, lanes, 4])
}
