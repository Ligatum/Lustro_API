//! Lustro V1 PYTHON interface.

pub mod hash;
pub mod prng;
pub mod xof;

use numpy::{PyArray1, PyArray3, PyArrayMethods};
use pyo3::buffer::PyBuffer;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBytes, PySequence, PyString};
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

// PyO3 turns panics into exceptions only when unwinding.
#[cfg(panic = "abort")]
compile_error!("lustro Python bindings require panic = \"unwind\"");

/// Version of the native API (integer), independent of `__version__`.
#[pyfunction]
pub fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

/// Execution settings of this process as a dict: `backend` ("scalar" or "avx2", as
/// selected for stream and hash batches) and `threads` (configured pool size, not a count of
/// active workers; 0 = calling thread only). The pool is not started by this call and may
/// fail to start; batches then run on the calling thread. See `LUSTRO_NUM_THREADS`,
/// `LUSTRO_DISABLE_MT`, `LUSTRO_DISABLE_SIMD`.
#[pyfunction]
pub fn runtime_info(py: Python<'_>) -> PyResult<Bound<'_, pyo3::types::PyDict>> {
    let info = crate::api::runtime_info();
    let d = pyo3::types::PyDict::new(py);
    d.set_item("backend", info.backend)?;
    d.set_item("threads", info.threads)?;
    Ok(d)
}

/// Sets threads and SIMD for this process, before first use. `threads` >= 1, capped
/// at the logical CPU count, 1 = no pool. `allow_simd=False` = scalar only. None
/// follows the environment, a given value replaces it.
///
/// Fixed once, by an earlier call, a batch call, `suggested_steps()` or
/// `runtime_info()`. Later calls raise `RuntimeError`, also with identical arguments.
#[pyfunction]
#[pyo3(signature = (threads=None, allow_simd=None))]
pub fn configure(threads: Option<i64>, allow_simd: Option<bool>) -> PyResult<()> {
    let mut options = crate::RuntimeOptions::new();
    if let Some(n) = threads {
        if n < 1 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "threads must be at least 1",
            ));
        }
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        options =
            options.threads(std::num::NonZeroUsize::new(n).unwrap_or(std::num::NonZeroUsize::MIN));
    }
    if let Some(allow) = allow_simd {
        options = options.allow_simd(allow);
    }
    crate::configure(options).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}

// Separate module: the macro expands to a `gil-refs` cfg that rustc does not know.
#[allow(unexpected_cfgs)]
mod exceptions {
    pyo3::create_exception!(
        lustro,
        SnapshotError,
        pyo3::exceptions::PyValueError,
        "A snapshot could not be imported."
    );
}
pub use exceptions::SnapshotError;

// Maps SnapshotError to the Python SnapshotError (message from Display).
pub(crate) fn snapshot_error(err: crate::types::SnapshotError) -> pyo3::PyErr {
    SnapshotError::new_err(err.to_string())
}

// Maps an allocation failure to MemoryError.
pub(crate) fn alloc_error() -> pyo3::PyErr {
    pyo3::exceptions::PyMemoryError::new_err("allocation failed")
}

// Error of a batch `try_derive_path`. The callers check `path` first, so `EmptyPath`
// is not expected here.
pub(crate) fn derive_path_error(e: crate::types::DerivePathError) -> pyo3::PyErr {
    match e {
        crate::types::DerivePathError::EmptyPath => {
            pyo3::exceptions::PyValueError::new_err("path must not be empty")
        }
        crate::types::DerivePathError::Reserve(_) => alloc_error(),
    }
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
    let view = pyo3::buffer::PyBuffer::<u8>::get(buf).map_err(|_| bad())?;
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

// Exported buffer, released when dropped. The GIL is held for the whole life of the value.
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
// stays held. TypeError for an object without a buffer, a read-only or non-contiguous one;
// the exporters report those cases with TypeError, BufferError or ValueError.
pub(crate) fn fill_raw_buffer(
    buf: &Bound<'_, pyo3::PyAny>,
    fill: impl FnOnce(&mut [u8]),
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
    // SAFETY: the buffer was requested writable and is C-contiguous, so `buf` covers `len`
    // consecutive bytes. `raw` keeps the export alive, and no Python code runs until `fill`
    // returns.
    let slice = unsafe { std::slice::from_raw_parts_mut(raw.0.buf as *mut u8, len) };
    fill(slice);
    Ok(len)
}

const BYTES_EXPECTED: &str = "expected bytes or a C-contiguous byte buffer";

// Copy of a byte buffer (bytearray, memoryview, array('B'), NumPy uint8, ...), taken with
// the GIL held. TypeError if there is no buffer, its items are not bytes or it is not
// C-contiguous; MemoryError if the copy cannot be reserved. TypeError and BufferError
// from the exporter are folded into that TypeError, other exception types are passed on.
// The copy protects against other Python threads. Native code that writes to the buffer
// without the GIL is not covered.
fn copy_buffer(obj: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    copy_buffer_apart(obj, None)
}

// As `copy_buffer`; with `apart` = (address, length) of a range that is written afterwards,
// ValueError if the buffer shares memory with it.
fn copy_buffer_apart(obj: &Bound<'_, PyAny>, apart: Option<(usize, usize)>) -> PyResult<Vec<u8>> {
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
    let len = view.len_bytes();
    if let Some((at, at_len)) = apart {
        if hash::overlaps(view.buf_ptr() as usize, len, at, at_len) {
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

// u128 argument (`from_py_with`). A negative int is `OverflowError` on every Python
// version, PyO3 gives `ValueError` for it on 3.13.
pub(crate) fn u128_arg(obj: &Bound<'_, PyAny>) -> PyResult<u128> {
    obj.extract::<u128>().map_err(|e| {
        let py = obj.py();
        if e.is_instance_of::<pyo3::exceptions::PyValueError>(py) && obj.lt(0).unwrap_or(false) {
            pyo3::exceptions::PyOverflowError::new_err("can't convert negative int to unsigned")
        } else {
            e
        }
    })
}

// List of u128 ids (`from_py_with`), same rules per item as `u128_arg`.
pub(crate) fn ids_arg(obj: &Bound<'_, PyAny>) -> PyResult<Vec<u128>> {
    let items: Vec<Bound<'_, PyAny>> = obj.extract()?;
    items.iter().map(u128_arg).collect()
}

// Messages of a batch argument. `slices` point into `bytes` objects and into `copies`;
// they are 'static only inside this type and are handed out with the lifetime of `self`.
// A `bytes` object stays alive through `refs`, except after `load_borrowed()` where the
// sequence keeps it and the caller keeps the GIL until the slices are no longer used.
// `copies` are never modified, and moving the outer Vec does not move their buffers.
pub struct Messages<'py> {
    slices: Vec<&'static [u8]>,
    _refs: Vec<Bound<'py, PyBytes>>,
    _copies: Vec<Vec<u8>>,
}

impl Messages<'_> {
    pub(crate) fn slices(&self) -> &[&[u8]] {
        &self.slices
    }
}

// Largest table of slices kept per thread between calls. A fresh allocation of this size
// comes back as new pages on every call, which costs about as much as hashing short messages.
const TABLE_KEEP: usize = 32 << 20;

thread_local! {
    static TABLE: RefCell<Vec<&'static [u8]>> = const { RefCell::new(Vec::new()) };
}

// Empty table with room for `n` slices, from the thread's kept one if there is one.
fn messages_table(n: usize) -> PyResult<Vec<&'static [u8]>> {
    let mut table = TABLE
        .try_with(|t| std::mem::take(&mut *t.borrow_mut()))
        .unwrap_or_default();
    table.clear();
    table.try_reserve_exact(n).map_err(|_| alloc_error())?;
    Ok(table)
}

impl Drop for Messages<'_> {
    fn drop(&mut self) {
        if self.slices.capacity() * std::mem::size_of::<&[u8]>() <= TABLE_KEEP {
            let mut table = std::mem::take(&mut self.slices);
            // The slices must not outlive their owners; the table keeps only its capacity.
            table.clear();
            // Err only while the thread is exiting; the table is freed then.
            let _ = TABLE.try_with(|t| *t.borrow_mut() = table);
        }
    }
}

// Batch argument (`from_py_with`): only the container is checked here, the items are read
// by `load()`, so that other argument checks can come first. A single string or buffer is
// rejected as a whole, even though `memoryview` and `array` are sequences of integers.
pub struct MessageSeq<'py>(Bound<'py, PySequence>);

pub(crate) fn messages_arg<'py>(obj: &Bound<'py, PyAny>) -> PyResult<MessageSeq<'py>> {
    let type_error = |msg: &'static str| pyo3::exceptions::PyTypeError::new_err(msg);
    // SAFETY: `obj` is a valid object pointer and the GIL is held.
    if obj.is_instance_of::<PyString>()
        || unsafe { pyo3::ffi::PyObject_CheckBuffer(obj.as_ptr()) } != 0
    {
        return Err(type_error(
            "expected a sequence of bytes-like objects, not a single string or buffer",
        ));
    }
    let seq = obj
        .cast::<PySequence>()
        .map_err(|_| type_error("expected a sequence of bytes-like objects"))?;
    Ok(MessageSeq(seq.clone()))
}

impl<'py> MessageSeq<'py> {
    // Number of items, for checks that come before the items are read.
    pub(crate) fn len(&self) -> PyResult<usize> {
        self.0.len()
    }

    // Items with a reference held on each `bytes`, safe to use with the GIL released.
    // TypeError names the first item that is not a byte buffer.
    pub(crate) fn load(&self) -> PyResult<Messages<'py>> {
        self.load_inner(false, None)
    }

    // As `load()`, but for an exact `list` or `tuple` of exact `bytes` the items are not
    // referenced (no refcount traffic); any other input is loaded as in `load()`.
    //
    // SAFETY: the slices may point into objects kept alive only by the sequence. The caller
    // must keep the GIL held for as long as it uses them (no `detach`, no call that can run
    // Python code), and the sequence must stay alive. `apart` = (address, length) of the
    // memory the caller writes the digests to: ValueError if a copied item shares memory
    // with it (a `bytes` item cannot).
    pub(crate) unsafe fn load_borrowed(
        &self,
        apart: Option<(usize, usize)>,
    ) -> PyResult<Messages<'py>> {
        self.load_inner(true, apart)
    }

    // Hashes an exact `list` or `tuple` of at least `PAR_LOAD_MIN` messages into `out` with
    // the items read by the threads that hash them, so no table of slices is built and each
    // item is touched once. `run` is given the messages and `out` and returns false when it
    // did not hash anything. Returns true when `out` holds all digests. False means that
    // nothing was hashed, or that an item is not an exact `bytes`: `out` may then be partly
    // written, and the caller must hash the messages by another path. That path must not
    // read the other items from `out` (see `load_borrowed`, `apart`), and any error it
    // raises (not only TypeError) leaves `out` as it is, possibly partly written.
    //
    // SAFETY: the GIL is held by the caller and stays held, and `out` does not overlap any
    // exact `bytes` item. No Python code runs in here (`gil_enabled` is read at init).
    pub(crate) unsafe fn hash_fused<T>(
        &self,
        out: &mut [T],
        run: impl FnOnce(ListMessages, &mut [T]) -> bool,
    ) -> bool {
        use pyo3::ffi;
        // Before the size is read: this must not run Python code once `out` is borrowed.
        if !gil_enabled(self.0.py()) {
            return false;
        }
        let obj = self.0.as_ptr();
        let (is_list, n) = if ffi::PyList_CheckExact(obj) != 0 {
            (true, ffi::PyList_GET_SIZE(obj) as usize)
        } else if ffi::PyTuple_CheckExact(obj) != 0 {
            (false, ffi::PyTuple_GET_SIZE(obj) as usize)
        } else {
            return false;
        };
        if n != out.len() || n < PAR_LOAD_MIN {
            return false;
        }
        let bad = AtomicBool::new(false);
        let messages = ListMessages {
            seq: ItemSource { obj, is_list },
            start: 0,
            end: n,
            bad: &bad,
        };
        run(messages, out) && !bad.load(Ordering::Relaxed)
    }

    fn load_inner(&self, hold: bool, apart: Option<(usize, usize)>) -> PyResult<Messages<'py>> {
        if let Some(messages) = self.load_exact(hold)? {
            return Ok(messages);
        }
        let n = self.0.len()?;
        let mut messages = Messages {
            slices: messages_table(n)?,
            _refs: Vec::new(),
            _copies: Vec::new(),
        };
        messages
            ._refs
            .try_reserve_exact(n)
            .map_err(|_| alloc_error())?;
        // The iterator may yield more items than `len()` said, so each push is reserved.
        for (i, item) in self.0.try_iter()?.enumerate() {
            messages.slices.try_reserve(1).map_err(|_| alloc_error())?;
            match item?.cast_into::<PyBytes>() {
                Ok(bytes) => {
                    messages._refs.try_reserve(1).map_err(|_| alloc_error())?;
                    let s: *const [u8] = bytes.as_bytes();
                    // SAFETY: `bytes` is moved into `_refs` below and keeps the data alive.
                    messages.slices.push(unsafe { &*s });
                    messages._refs.push(bytes);
                }
                Err(e) => {
                    let copy = copy_buffer_apart(&e.into_inner(), apart).map_err(|err| {
                        if err.is_instance_of::<pyo3::exceptions::PyTypeError>(self.0.py()) {
                            pyo3::exceptions::PyTypeError::new_err(format!(
                                "messages[{i}]: {BYTES_EXPECTED}"
                            ))
                        } else {
                            err
                        }
                    })?;
                    messages._copies.try_reserve(1).map_err(|_| alloc_error())?;
                    let s: *const [u8] = copy.as_slice();
                    messages._copies.push(copy);
                    // SAFETY: the buffer of the Vec just stored in `_copies` does not move.
                    messages.slices.push(unsafe { &*s });
                }
            }
        }
        Ok(messages)
    }

    // One pass over an exact `list` or `tuple` of exact `bytes`; None for anything else.
    // No Python code runs: the type checks and the size/item macros only read the objects.
    fn load_exact(&self, hold: bool) -> PyResult<Option<Messages<'py>>> {
        use pyo3::ffi;
        let py = self.0.py();
        // The raw reads below rely on the GIL to keep the sequence unchanged, so this comes first.
        if !gil_enabled(py) {
            return Ok(None);
        }
        let obj = self.0.as_ptr();
        // SAFETY: the GIL is held; `obj` is a live object.
        let (is_list, n) = unsafe {
            if ffi::PyList_CheckExact(obj) != 0 {
                (true, ffi::PyList_GET_SIZE(obj) as usize)
            } else if ffi::PyTuple_CheckExact(obj) != 0 {
                (false, ffi::PyTuple_GET_SIZE(obj) as usize)
            } else {
                return Ok(None);
            }
        };
        let mut messages = Messages {
            slices: messages_table(n)?,
            _refs: Vec::new(),
            _copies: Vec::new(),
        };
        if hold {
            let seq = ItemSource { obj, is_list };
            let bad = AtomicBool::new(false);
            let spare = &mut messages.slices.spare_capacity_mut()[..n];
            crate::dispatch::for_chunks_mut(spare, PAR_LOAD_MIN, |start, chunk| {
                for (k, slot) in chunk.iter_mut().enumerate() {
                    // SAFETY: `start + k < n`. The calling thread holds the GIL and waits
                    // for this call, no Python code runs, so the sequence and its items do
                    // not change while other threads read them.
                    match unsafe { seq.bytes_at(start + k) } {
                        Some(s) => {
                            slot.write(s);
                        }
                        None => {
                            bad.store(true, Ordering::Relaxed);
                            slot.write(&[]);
                        }
                    }
                }
            });
            if bad.load(Ordering::Relaxed) {
                return Ok(None);
            }
            // SAFETY: all `n` slots were written above.
            unsafe { messages.slices.set_len(n) };
            return Ok(Some(messages));
        }
        messages
            ._refs
            .try_reserve_exact(n)
            .map_err(|_| alloc_error())?;
        let seq = ItemSource { obj, is_list };
        for i in 0..n {
            // SAFETY: `i < n`, and nothing in this loop can run Python code, so the
            // container does not change; the item is a borrowed pointer.
            unsafe {
                let item = seq.item(i);
                match seq.bytes_at(i) {
                    Some(s) => messages.slices.push(s),
                    None => return Ok(None),
                }
                messages
                    ._refs
                    .push(Bound::from_borrowed_ptr(py, item).cast_into_unchecked());
            }
        }
        Ok(Some(messages))
    }
}

// Fewest messages for which an in-place read of the items is split across the pool.
const PAR_LOAD_MIN: usize = 16384;

// Items of an exact `list` or `tuple`, read by pointer.
#[derive(Clone, Copy)]
struct ItemSource {
    obj: *mut pyo3::ffi::PyObject,
    is_list: bool,
}

// SAFETY: only used for reads while the thread that holds the GIL waits (see `load_exact`).
unsafe impl Sync for ItemSource {}

impl ItemSource {
    // SAFETY: GIL held by the calling thread (or by a thread waiting for this one), `i` is in
    // range and the sequence is not modified.
    unsafe fn item(&self, i: usize) -> *mut pyo3::ffi::PyObject {
        use pyo3::ffi;
        if self.is_list {
            ffi::PyList_GET_ITEM(self.obj, i as ffi::Py_ssize_t)
        } else {
            ffi::PyTuple_GET_ITEM(self.obj, i as ffi::Py_ssize_t)
        }
    }

    // The bytes of item `i` if it is an exact `bytes`. Same contract as `item`; the slice is
    // valid while the item is alive.
    unsafe fn bytes_at(&self, i: usize) -> Option<&'static [u8]> {
        use pyo3::ffi;
        let item = self.item(i);
        if ffi::PyBytes_CheckExact(item) == 0 {
            return None;
        }
        let data = ffi::PyBytes_AS_STRING(item) as *const u8;
        Some(std::slice::from_raw_parts(
            data,
            ffi::Py_SIZE(item) as usize,
        ))
    }
}

// Messages `start..end` of an exact `list` or `tuple` of a `hash_fused` call, turned into
// slices on the threads that hash them. `bad` is set when an item is not an exact `bytes`
// (its slice is empty then).
#[derive(Clone, Copy)]
pub(crate) struct ListMessages {
    seq: ItemSource,
    start: usize,
    end: usize,
    bad: *const AtomicBool,
}

// SAFETY: only used inside `hash_fused`, where the thread that holds the GIL waits for the
// workers: the sequence and its items are only read and `bad` outlives them.
unsafe impl Sync for ListMessages {}

impl crate::dispatch::MessageBlocks for ListMessages {
    fn len(&self) -> usize {
        self.end - self.start
    }

    fn range(&self, start: usize, end: usize) -> Self {
        Self {
            start: self.start + start,
            end: self.start + end,
            ..*self
        }
    }

    fn for_each_block(&self, mut f: impl FnMut(usize, &[&[u8]])) {
        let mut block: [&[u8]; crate::dispatch::BLOCK] = [&[]; crate::dispatch::BLOCK];
        let mut at = 0;
        while self.start + at < self.end {
            let count = (self.end - self.start - at).min(crate::dispatch::BLOCK);
            for (k, slot) in block[..count].iter_mut().enumerate() {
                // SAFETY: in range; see the `Sync` impl.
                *slot = match unsafe { self.seq.bytes_at(self.start + at + k) } {
                    Some(s) => s,
                    None => {
                        // SAFETY: `bad` outlives the workers.
                        unsafe { (*self.bad).store(true, Ordering::Relaxed) };
                        &[]
                    }
                };
            }
            f(at, &block[..count]);
            at += count;
        }
    }
}

// Whether the GIL is enabled (False on a free-threaded build run without it). Read once, at
// module init (`init_gil_flag`), so the later calls run no Python code. No `sys._is_gil_enabled`
// (Python before 3.13) means a build with the GIL; any other failure counts as disabled.
fn gil_enabled(py: Python<'_>) -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    if let Some(&on) = CACHE.get() {
        return on;
    }
    // Queried outside `get_or_init`: the call may run user code that re-enters this function.
    let on = match py
        .import("sys")
        .and_then(|sys| sys.getattr("_is_gil_enabled"))
    {
        Ok(f) => f.call0().and_then(|r| r.is_truthy()).unwrap_or(false),
        Err(e) => e.is_instance_of::<pyo3::exceptions::PyAttributeError>(py),
    };
    *CACHE.get_or_init(|| on)
}

// Fills the GIL flag; called from the module init.
pub(crate) fn init_gil_flag(py: Python<'_>) {
    gil_enabled(py);
}

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
