//! Python bindings: message batches.
//! Reads a sequence of bytes-like objects for the hash batch calls, also in place from an
//! exact `list` or `tuple` of `bytes`.

use super::{alloc_error, copy_buffer_apart, gil_enabled, BYTES_EXPECTED};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PySequence, PyString};
use std::cell::{Cell, RefCell};

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
    // No Python code runs: the type checks and the size/item calls only read the objects.
    // With `hold` no reference is taken on the items (see `load_borrowed`). The items are
    // read on this thread only: the C API calls need the GIL on the calling thread.
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
                (true, ffi::PyList_Size(obj) as usize)
            } else if ffi::PyTuple_CheckExact(obj) != 0 {
                (false, ffi::PyTuple_Size(obj) as usize)
            } else {
                return Ok(None);
            }
        };
        let mut messages = Messages {
            slices: messages_table(n)?,
            _refs: Vec::new(),
            _copies: Vec::new(),
        };
        if !hold {
            messages
                ._refs
                .try_reserve_exact(n)
                .map_err(|_| alloc_error())?;
        }
        let seq = ItemSource { obj, is_list };
        for i in 0..n {
            // SAFETY: `i < n`, and nothing in this loop can run Python code, so the
            // container does not change; the item is a borrowed pointer.
            unsafe {
                let item = seq.item(i);
                match bytes_of(item) {
                    Some(s) => messages.slices.push(s),
                    None => return Ok(None),
                }
                if !hold {
                    messages
                        ._refs
                        .push(Bound::from_borrowed_ptr(py, item).cast_into_unchecked());
                }
            }
        }
        Ok(Some(messages))
    }
}

// Fewest messages for which `hash_var_pipelined` is tried.
pub(crate) const PIPELINE_MIN: usize = 4096;

// An exact `list` or `tuple` read range by range into a table of slices, for
// `hash{256,128}_var_pipelined`. The items are read on the thread that holds the GIL; the
// slices handed out stay valid while the sequence is alive and unchanged; `_keep` holds a
// reference to it for as long as this value lives.
pub(crate) struct ExactItems<'py> {
    _keep: Bound<'py, PySequence>,
    seq: ItemSource,
    n: usize,
    table: Vec<&'static [u8]>,
    // Start of `table`'s buffer, taken once: handed out ranges are read by the workers while
    // later ones are written.
    base: *mut &'static [u8],
    next: Cell<usize>,
}

impl<'py> MessageSeq<'py> {
    // The items of an exact `list` or `tuple` of at least `min` items, for a range by range
    // read; None for anything else. Nothing is read yet.
    pub(crate) fn exact_items(&self, min: usize) -> PyResult<Option<ExactItems<'py>>> {
        use pyo3::ffi;
        let py = self.0.py();
        if !gil_enabled(py) {
            return Ok(None);
        }
        let obj = self.0.as_ptr();
        // SAFETY: the GIL is held; `obj` is a live object.
        let (is_list, n) = unsafe {
            if ffi::PyList_CheckExact(obj) != 0 {
                (true, ffi::PyList_Size(obj) as usize)
            } else if ffi::PyTuple_CheckExact(obj) != 0 {
                (false, ffi::PyTuple_Size(obj) as usize)
            } else {
                return Ok(None);
            }
        };
        if n < min {
            return Ok(None);
        }
        let mut table = messages_table(n)?;
        let base = table.as_mut_ptr();
        Ok(Some(ExactItems {
            _keep: self.0.clone(),
            seq: ItemSource { obj, is_list },
            n,
            table,
            base,
            next: Cell::new(0),
        }))
    }
}

impl ExactItems<'_> {
    pub(crate) fn len(&self) -> usize {
        self.n
    }

    // Reads items `start..end` into the table; None if one of them is not an exact `bytes`.
    // Ranges must follow each other from 0.
    //
    // SAFETY: the GIL is held by this thread from `exact_items` until the last returned
    // slice is no longer used, and no Python code runs in between, so the sequence and its
    // items do not change.
    pub(crate) unsafe fn read(&self, start: usize, end: usize) -> Option<&[&[u8]]> {
        assert!(start == self.next.get() && start <= end && end <= self.n);
        for i in start..end {
            // Only the slots `start..end` are written; earlier ones may be read meanwhile.
            self.base.add(i).write(self.seq.bytes_at(i)?);
        }
        self.next.set(end);
        Some(std::slice::from_raw_parts(
            self.base.add(start),
            end - start,
        ))
    }
}

impl Drop for ExactItems<'_> {
    fn drop(&mut self) {
        // The table is kept for the next call; its length stays 0, so no slice outlives this.
        let table = std::mem::take(&mut self.table);
        if table.capacity() * std::mem::size_of::<&[u8]>() <= TABLE_KEEP {
            let _ = TABLE.try_with(|t| *t.borrow_mut() = table);
        }
    }
}

// Items of an exact `list` or `tuple`, read by pointer.
struct ItemSource {
    obj: *mut pyo3::ffi::PyObject,
    is_list: bool,
}

impl ItemSource {
    // SAFETY: GIL held by the calling thread, `i` is in range and the sequence is not
    // modified.
    unsafe fn item(&self, i: usize) -> *mut pyo3::ffi::PyObject {
        use pyo3::ffi;
        if self.is_list {
            ffi::PyList_GetItem(self.obj, i as ffi::Py_ssize_t)
        } else {
            ffi::PyTuple_GetItem(self.obj, i as ffi::Py_ssize_t)
        }
    }

    // The bytes of item `i` if it is an exact `bytes`. Same contract as `item`; the slice is
    // valid while the item is alive.
    unsafe fn bytes_at(&self, i: usize) -> Option<&'static [u8]> {
        bytes_of(self.item(i))
    }
}

// The bytes of `item` if it is an exact `bytes`. GIL held; the slice is valid while the
// item is alive.
unsafe fn bytes_of(item: *mut pyo3::ffi::PyObject) -> Option<&'static [u8]> {
    use pyo3::ffi;
    if ffi::PyBytes_CheckExact(item) == 0 {
        return None;
    }
    let mut data = std::ptr::null_mut();
    let mut len: ffi::Py_ssize_t = 0;
    // Does not fail for an exact `bytes` when the length is asked for; if it did, the caller
    // falls back to the path that reports the error.
    if ffi::PyBytes_AsStringAndSize(item, &mut data, &mut len) != 0 {
        ffi::PyErr_Clear();
        return None;
    }
    Some(std::slice::from_raw_parts(data as *const u8, len as usize))
}
