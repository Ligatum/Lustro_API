//! Python bindings: byte buffer helpers.
//! Writing into caller buffers, copying and borrowing input bytes.

use super::alloc_error;
use pyo3::buffer::PyBuffer;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::borrow::Cow;

// Smallest input handled with the GIL released; for less, releasing it costs more than it gives.
pub(crate) const DETACH_MIN: usize = 4096;

// `advance_blocks` and `advance_bytes` skip in chunks of this many blocks and look for signals
// in between (about 30 ms of work per chunk).
pub(crate) const SKIP_CHUNK_BLOCKS: u64 = 1 << 20;

// Skips `n` units (bytes or blocks) of a stream. Below `small` units the skip runs with the GIL
// held. Otherwise it runs with the GIL released, `chunk` units at a time, and a pending signal
// is raised between the chunks. The work is done on a copy that replaces `stream` at the end,
// so a KeyboardInterrupt leaves the stream where it was.
pub(crate) fn skip_chunked<T: Clone + Send>(
    py: Python<'_>,
    stream: &mut T,
    n: u64,
    small: u64,
    chunk: u64,
    skip: impl Fn(&mut T, u64) + Sync,
) -> PyResult<()> {
    if n < small {
        skip(stream, n);
        return Ok(());
    }
    let mut work = stream.clone();
    let mut left = n;
    while left > 0 {
        let take = left.min(chunk);
        py.detach(|| skip(&mut work, take));
        left -= take;
        py.check_signals()?;
    }
    *stream = work;
    Ok(())
}

// Largest piece generated per released GIL when writing into a caller's buffer.
const STAGE_CHUNK: usize = 16 << 20;

// Writes `len` bytes produced by `fill` to `dst`. Below `DETACH_MIN` the bytes are written
// in place with the GIL held. Otherwise they are generated into a Rust buffer with the GIL
// released and copied to `dst` with the GIL held, `STAGE_CHUNK` bytes at a time, so no
// other Python thread can change `dst` while Rust borrows it. The staging buffer is
// allocated before the first byte is generated: on MemoryError the stream has not moved.
//
// SAFETY: `dst` is valid for `len` writable bytes, kept alive and in place by a buffer
// view held by the caller. Native code that writes to it without the GIL is not covered.
unsafe fn write_into(
    py: Python<'_>,
    dst: *mut u8,
    len: usize,
    mut fill: impl FnMut(&mut [u8]) + Send,
) -> PyResult<()> {
    if len < DETACH_MIN {
        fill(std::slice::from_raw_parts_mut(dst, len));
        return Ok(());
    }
    let n = len.min(STAGE_CHUNK);
    let mut stage: Vec<u8> = Vec::new();
    stage.try_reserve_exact(n).map_err(|_| alloc_error())?;
    stage.resize(n, 0);
    let mut done = 0;
    while done < len {
        let take = (len - done).min(n);
        py.detach(|| fill(&mut stage[..take]));
        std::ptr::copy_nonoverlapping(stage.as_ptr(), dst.add(done), take);
        done += take;
    }
    Ok(())
}

// Writes into a writable, C-contiguous byte buffer, see `write_into`; the held buffer view
// keeps the exporter alive and blocks resizing. TypeError, BufferError and ValueError of the
// exporter become the same TypeError as for a missing buffer, other exceptions are passed on
// (as in `fill_raw_buffer`).
pub(crate) fn fill_buffer(
    buf: &Bound<'_, pyo3::PyAny>,
    fill: impl FnMut(&mut [u8]) + Send,
) -> PyResult<()> {
    let bad = || {
        pyo3::exceptions::PyTypeError::new_err(
            "buf must be a writable, C-contiguous buffer of bytes \
             (bytearray, memoryview, numpy uint8 array)",
        )
    };
    let view = pyo3::buffer::PyBuffer::<u8>::get(buf).map_err(|e| {
        let py = buf.py();
        if e.is_instance_of::<pyo3::exceptions::PyTypeError>(py)
            || e.is_instance_of::<pyo3::exceptions::PyBufferError>(py)
            || e.is_instance_of::<pyo3::exceptions::PyValueError>(py)
        {
            bad()
        } else {
            e
        }
    })?;
    if view.readonly() || !view.is_c_contiguous() {
        return Err(bad());
    }
    let len = view.len_bytes();
    if len == 0 {
        return Ok(());
    }
    // SAFETY: `view` is writable and C-contiguous, so the pointer covers `len` consecutive
    // bytes. `view` keeps the exporting object alive until `write_into` returns.
    unsafe { write_into(buf.py(), view.buf_ptr() as *mut u8, len, fill) }
}

// Exported buffer, released when dropped. Dropped with the GIL held.
struct RawBuffer(pyo3::ffi::Py_buffer);

impl Drop for RawBuffer {
    fn drop(&mut self) {
        // SAFETY: the buffer was filled by a successful `PyObject_GetBuffer` and is released once.
        unsafe { pyo3::ffi::PyBuffer_Release(&mut self.0) };
    }
}

// Whether an item format holds a Python object (`O`, also inside a struct format). A NULL
// format means bytes. Field names of a struct format stand between colons and are skipped.
//
// SAFETY: `format` is NULL or points to a NUL-terminated string.
unsafe fn has_object_items(format: *const std::os::raw::c_char) -> bool {
    if format.is_null() {
        return false;
    }
    let mut in_name = false;
    for &c in std::ffi::CStr::from_ptr(format).to_bytes() {
        match c {
            b':' => in_name = !in_name,
            b'O' if !in_name => return true,
            _ => {}
        }
    }
    false
}

// As `fill_buffer`, but for any writable, C-contiguous buffer, whatever its item format
// (`bytearray`, `array('I')`, a numpy array of any dtype, ...), except one that holds Python
// objects. The stream bytes are written as raw memory, without conversion, and the number
// of bytes written is returned. Written as in `write_into`. TypeError for
// an object without a buffer, a read-only, a non-contiguous or an object buffer (numpy
// `dtype=object`): raw bytes over object pointers would corrupt the interpreter.
// TypeError, BufferError and ValueError of the exporter become the same TypeError as for a
// missing buffer, other exceptions are passed on.
pub(crate) fn fill_raw_buffer(
    buf: &Bound<'_, pyo3::PyAny>,
    fill: impl FnMut(&mut [u8]) + Send,
) -> PyResult<usize> {
    let py = buf.py();
    let bad = || {
        pyo3::exceptions::PyTypeError::new_err(
            "buf must be a writable, C-contiguous bytes-like object",
        )
    };
    let mut view = std::mem::MaybeUninit::<pyo3::ffi::Py_buffer>::uninit();
    // SAFETY: `buf` is a live object and the GIL is held; on success `view` is initialized.
    // `PyBUF_ND` is there because a memoryview refuses `PyBUF_FORMAT` without a shape request.
    let rc = unsafe {
        pyo3::ffi::PyObject_GetBuffer(
            buf.as_ptr(),
            view.as_mut_ptr(),
            pyo3::ffi::PyBUF_WRITABLE | pyo3::ffi::PyBUF_FORMAT | pyo3::ffi::PyBUF_ND,
        )
    };
    if rc != 0 {
        let err = PyErr::fetch(py);
        let folded = err.is_instance_of::<pyo3::exceptions::PyTypeError>(py)
            || err.is_instance_of::<pyo3::exceptions::PyBufferError>(py)
            || err.is_instance_of::<pyo3::exceptions::PyValueError>(py);
        return Err(if folded { bad() } else { err });
    }
    // SAFETY: `PyObject_GetBuffer` succeeded.
    let raw = RawBuffer(unsafe { view.assume_init() });
    // SAFETY: `raw` holds a valid buffer.
    if unsafe { pyo3::ffi::PyBuffer_IsContiguous(&raw.0, b'C' as std::os::raw::c_char) } == 0 {
        return Err(bad());
    }
    // SAFETY: `PyBUF_FORMAT` was requested, so `format` is NULL or a NUL-terminated string.
    if unsafe { has_object_items(raw.0.format) } {
        return Err(pyo3::exceptions::PyTypeError::new_err(
            "buf must not hold Python objects",
        ));
    }
    let len = raw.0.len as usize;
    if len == 0 {
        return Ok(0);
    }
    // SAFETY: the buffer was requested writable and is C-contiguous, so the pointer covers
    // `len` consecutive bytes. `raw` keeps the export alive until `write_into` returns.
    unsafe { write_into(py, raw.0.buf as *mut u8, len, fill) }?;
    Ok(len)
}

pub(super) const BYTES_EXPECTED: &str = "expected bytes or a C-contiguous byte buffer";

// Read-only view of a byte buffer (bytearray, memoryview, array('B'), NumPy uint8, ...) with
// the same errors as `copy_buffer`. The view keeps the exporter alive and in place; the
// bytes can still be changed by other Python threads once the GIL is released.
pub(super) fn byte_view(obj: &Bound<'_, PyAny>) -> PyResult<PyBuffer<u8>> {
    let bad = || pyo3::exceptions::PyTypeError::new_err(BYTES_EXPECTED);
    // PyO3 reports a missing buffer and a wrong item format as TypeError/BufferError, which
    // cannot be told apart from the same types raised by an exporter.
    let view = PyBuffer::<u8>::get(obj).map_err(|e| {
        let py = obj.py();
        if e.is_instance_of::<pyo3::exceptions::PyTypeError>(py)
            || e.is_instance_of::<pyo3::exceptions::PyBufferError>(py)
        {
            bad()
        } else {
            e
        }
    })?;
    if !view.is_c_contiguous() {
        return Err(bad());
    }
    Ok(view)
}

// Copy of a byte buffer (bytearray, memoryview, array('B'), NumPy uint8, ...), taken with
// the GIL held. TypeError if there is no buffer, its items are not bytes or it is not
// C-contiguous; MemoryError if the copy cannot be reserved. TypeError and BufferError
// from the exporter are folded into that TypeError, other exception types are passed on.
// The copy protects against other Python threads. Native code that writes to the buffer
// without the GIL is not covered.
pub(super) fn copy_buffer(obj: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    copy_buffer_apart(obj, None)
}

// As `copy_buffer`; with `apart` = (address, length) of a range that is written afterwards,
// ValueError if the buffer shares memory with it.
pub(super) fn copy_buffer_apart(
    obj: &Bound<'_, PyAny>,
    apart: Option<(usize, usize)>,
) -> PyResult<Vec<u8>> {
    let view = byte_view(obj)?;
    let len = view.len_bytes();
    if let Some((at, at_len)) = apart {
        if super::hash::overlaps(view.buf_ptr() as usize, len, at, at_len) {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "messages and out must not share memory",
            ));
        }
    }
    let mut copy = Vec::new();
    copy.try_reserve_exact(len).map_err(|_| alloc_error())?;
    if len != 0 {
        // SAFETY: `view` is C-contiguous, so `buf_ptr()` covers `len` consecutive bytes,
        // and it keeps the exporter alive and the buffer in place until it is dropped.
        let src = unsafe { std::slice::from_raw_parts(view.buf_ptr() as *const u8, len) };
        copy.extend_from_slice(src);
    }
    Ok(copy)
}

// Bytes-like argument (`from_py_with`). `bytes` is borrowed, any other byte buffer is
// copied, so the data stays valid and unchanged while the GIL is released.
pub(crate) fn bytes_arg<'a>(obj: &'a Bound<'_, PyAny>) -> PyResult<Cow<'a, [u8]>> {
    if let Ok(bytes) = obj.cast::<PyBytes>() {
        return Ok(Cow::Borrowed(bytes.as_bytes()));
    }
    copy_buffer(obj).map(Cow::Owned)
}
