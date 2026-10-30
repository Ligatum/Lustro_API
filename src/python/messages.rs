//! Python bindings: message batches.
//! Reads a sequence of bytes-like objects for the hash batch calls, also in place from an
//! exact `list` or `tuple` of `bytes`.

use super::{alloc_error, copy_buffer_apart, gil_enabled, BYTES_EXPECTED};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PySequence, PyString};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

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
