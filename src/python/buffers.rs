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

// Address of a writable buffer for the thread that fills it while the GIL is released.
struct SendPtr(*mut u8);

// SAFETY: the exporting object is kept alive and in place by the buffer view held by the
// caller until the fill returns. The filling thread needs exclusive access to the memory,
// reads included; the binding does not check that no other thread touches it.
unsafe impl Send for SendPtr {}

// Writes into a writable, C-contiguous byte buffer. From `DETACH_MIN` bytes the GIL is
// released while writing; the held buffer view keeps the exporter alive and blocks resizing.
pub(crate) fn fill_buffer(
    buf: &Bound<'_, pyo3::PyAny>,
    fill: impl FnOnce(&mut [u8]) + Send,
) -> PyResult<()> {
    let bad = || {
        pyo3::exceptions::PyTypeError::new_err(
            "buf must be a writable, C-contiguous buffer of bytes \
             (bytearray, memoryview, numpy uint8 array)",
        )
    };
    let view = pyo3::buffer::PyBuffer::<u8>::get(buf).map_err(|_| bad())?;
    if view.readonly() || !view.is_c_contiguous() {
        return Err(bad());
    }
    let len = view.len_bytes();
    if len == 0 {
        return Ok(());
    }
    let ptr = SendPtr(view.buf_ptr() as *mut u8);
    // SAFETY: `view` is writable and C-contiguous, so `ptr` covers `len` consecutive bytes.
    // `view` keeps the exporting object alive until `fill` returns.
    if len < DETACH_MIN {
        fill(unsafe { std::slice::from_raw_parts_mut(ptr.0, len) });
    } else {
        buf.py().detach(move || {
            let ptr = ptr;
            fill(unsafe { std::slice::from_raw_parts_mut(ptr.0, len) })
        });
    }
    Ok(())
}

// Exported buffer, released when dropped. Dropped with the GIL held.
struct RawBuffer(pyo3::ffi::Py_buffer);

impl Drop for RawBuffer {
    fn drop(&mut self) {
        // SAFETY: the buffer was filled by a successful `PyObject_GetBuffer` and is released once.
        unsafe { pyo3::ffi::PyBuffer_Release(&mut self.0) };
    }
}

// As `fill_buffer`, but for any writable, C-contiguous buffer, whatever its item format
// (`bytearray`, `array('I')`, a numpy array of any dtype, ...). The stream bytes are written
// as raw memory, without conversion, and the number of bytes written is returned. The GIL
// is released from `DETACH_MIN` bytes. TypeError for an object without a buffer, a read-only
// or non-contiguous one; TypeError, BufferError and ValueError of the exporter become this
// TypeError, other exceptions are passed on.
pub(crate) fn fill_raw_buffer(
    buf: &Bound<'_, pyo3::PyAny>,
    fill: impl FnOnce(&mut [u8]) + Send,
) -> PyResult<usize> {
    let py = buf.py();
    let bad = || {
        pyo3::exceptions::PyTypeError::new_err(
            "buf must be a writable, C-contiguous bytes-like object",
        )
    };
    let mut view = std::mem::MaybeUninit::<pyo3::ffi::Py_buffer>::uninit();
    // SAFETY: `buf` is a live object and the GIL is held; on success `view` is initialized.
    let rc = unsafe {
        pyo3::ffi::PyObject_GetBuffer(buf.as_ptr(), view.as_mut_ptr(), pyo3::ffi::PyBUF_WRITABLE)
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
    let len = raw.0.len as usize;
    if len == 0 {
        return Ok(0);
    }
    let ptr = SendPtr(raw.0.buf as *mut u8);
    // SAFETY: the buffer was requested writable and is C-contiguous, so `ptr` covers `len`
    // consecutive bytes. `raw` keeps the export alive until `fill` returns.
    if len < DETACH_MIN {
        fill(unsafe { std::slice::from_raw_parts_mut(ptr.0, len) });
    } else {
        py.detach(move || {
            let ptr = ptr;
            fill(unsafe { std::slice::from_raw_parts_mut(ptr.0, len) })
        });
    }
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
