//! FFI bindings for Lustro PRNG.
//! Each instance is an independent stream.

use crate::errors::LustroError;
use crate::prng::LustroPrng;
use crate::prng::LustroPrngBatch;
use crate::types::{Seed256, StreamId};

use super::guarded;
use super::types::{buf_in, buf_out, fits_slice, slice_in, slice_out};

// ==========================================
// FFI STREAM SINGLE API
// ==========================================

// Creates a PRNG context from a 32-byte seed and 128-bit stream ID.
// `stream_id` is passed as `(hi, lo)` u64 values.
// Returns null on invalid input.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_new(
    seed: *const u8,
    stream_id_hi: u64,
    stream_id_lo: u64,
) -> *mut LustroPrng {
    guarded(std::ptr::null_mut(), || {
        let seed_bytes = match buf_in(seed, 32) {
            Some(s) if s.len() == 32 => s,
            _ => return std::ptr::null_mut(),
        };
        let stream_id = ((stream_id_hi as u128) << 64) | (stream_id_lo as u128);

        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        Box::into_raw(Box::new(LustroPrng::new(&seed256, StreamId(stream_id))))
    })
}

// Frees a PRNG context. Passing null is safe (no-op).
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_free(ctx: *mut LustroPrng) {
    guarded((), || {
        if !ctx.is_null() {
            drop(Box::from_raw(ctx));
        }
    })
}

// Returns the next 64 random bits.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_u64(ctx: *mut LustroPrng, out: *mut u64) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() || out.is_null() {
            return LustroError::InvalidPointer;
        }
        let prng = &mut *ctx;

        let val = prng.next_u64();
        core::ptr::write_unaligned(out, val);
        LustroError::Ok
    })
}

// Returns the next 128 random bits as 16 bytes (LE).
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_u128(ctx: *mut LustroPrng, out: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 16) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let prng = &mut *ctx;

        let val = prng.next_u128();
        output.copy_from_slice(&val.to_le_bytes());
        LustroError::Ok
    })
}

// Returns one full 32-byte engine block.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_next_block(ctx: *mut LustroPrng, out: *mut u8) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 32) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let prng = &mut *ctx;

        let val = prng.next_block();
        output.copy_from_slice(&val);
        LustroError::Ok
    })
}

// Fills `out` with `out_len` random bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fill(
    ctx: *mut LustroPrng,
    out: *mut u8,
    out_len: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        if !fits_slice::<u8>(out_len) {
            return LustroError::InvalidLength;
        }
        let output = match buf_out(out, out_len) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        (*ctx).fill_bytes(output);
        LustroError::Ok
    })
}

// Clones the PRNG context and returns a new independent instance.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_clone(ctx: *const LustroPrng) -> *mut LustroPrng {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        Box::into_raw(Box::new((*ctx).clone()))
    })
}

// Derives a child PRNG from the current state and 128-bit identifier.
// `id` is passed as `(hi, lo)` u64 values.
// Returns null on null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_fork(
    ctx: *const LustroPrng,
    id_hi: u64,
    id_lo: u64,
) -> *mut LustroPrng {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let prng = &*ctx;
        let id = ((id_hi as u128) << 64) | (id_lo as u128);

        Box::into_raw(Box::new(prng.fork(StreamId(id))))
    })
}

// Derives a PRNG from a 32-byte seed along a path of `n` identifiers.
// IDs are passed as parallel `(hi, lo)` u64 arrays.
// Returns null on invalid input, including `n == 0`.
//
// # Safety
// `seed` must be valid for 32 bytes.
// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_derive_path(
    seed: *const u8,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
) -> *mut LustroPrng {
    guarded(std::ptr::null_mut(), || {
        let seed_bytes = match buf_in(seed, 32) {
            Some(s) if s.len() == 32 => s,
            _ => return std::ptr::null_mut(),
        };
        if n == 0 || ids_hi.is_null() || ids_lo.is_null() {
            return std::ptr::null_mut();
        }
        let his = match slice_in(ids_hi, n) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let los = match slice_in(ids_lo, n) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let path: Vec<StreamId> = his
            .iter()
            .zip(los.iter())
            .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
            .collect();

        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        Box::into_raw(Box::new(LustroPrng::derive_path(&seed256, &path)))
    })
}

// Exports the current PRNG snapshot into `out`.
// `out` must provide at least 56 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_export_snapshot(
    ctx: *const LustroPrng,
    out: *mut u8,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let output = match buf_out(out, 56) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };
        let prng = &*ctx;

        let snapshot = prng.export_snapshot();
        output.copy_from_slice(&snapshot.to_le_bytes());
        LustroError::Ok
    })
}

// Restores a PRNG context from a 56-byte snapshot.
// Returns null on invalid input or decoding failure.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_import_snapshot(bytes: *const u8) -> *mut LustroPrng {
    guarded(std::ptr::null_mut(), || {
        let snapshot_bytes = match buf_in(bytes, 56) {
            Some(s) if s.len() == 56 => s,
            _ => return std::ptr::null_mut(),
        };
        let array_ref: &[u8; 56] = snapshot_bytes.try_into().unwrap();

        if let Ok(snapshot) = crate::types::LustroPrngSnapshot::from_le_bytes(array_ref) {
            Box::into_raw(Box::new(LustroPrng::import_snapshot(snapshot)))
        } else {
            std::ptr::null_mut()
        }
    })
}

// ==========================================
// FFI STREAM BATCH API
// ==========================================

// Creates a PRNG batch from `n` stream identifiers.
// IDs are passed as parallel `(hi, lo)` u64 arrays.
// Returns null on invalid input.
//
// # Safety
// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_new(
    seed: *const u8,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
) -> *mut LustroPrngBatch {
    guarded(std::ptr::null_mut(), || {
        let seed_bytes = match buf_in(seed, 32) {
            Some(s) if s.len() == 32 => s,
            _ => return std::ptr::null_mut(),
        };

        let stream_ids: Vec<StreamId> = if n == 0 {
            Vec::new()
        } else {
            if ids_hi.is_null() || ids_lo.is_null() {
                return std::ptr::null_mut();
            }
            let his = match slice_in(ids_hi, n) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            let los = match slice_in(ids_lo, n) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            his.iter()
                .zip(los.iter())
                .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
                .collect()
        };

        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        Box::into_raw(Box::new(LustroPrngBatch::new(&seed256, &stream_ids)))
    })
}

// Creates `count` streams with sequential IDs starting at `first_stream_id`.
// `first_stream_id` is passed as `(hi, lo)` u64 values.
// Returns null on invalid input or allocation failure.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_new_range(
    seed: *const u8,
    first_hi: u64,
    first_lo: u64,
    count: usize,
) -> *mut LustroPrngBatch {
    guarded(std::ptr::null_mut(), || {
        let seed_bytes = match buf_in(seed, 32) {
            Some(s) if s.len() == 32 => s,
            _ => return std::ptr::null_mut(),
        };
        let first_id = ((first_hi as u128) << 64) | (first_lo as u128);

        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        match LustroPrngBatch::try_new_range(&seed256, StreamId(first_id), count) {
            Ok(batch) => Box::into_raw(Box::new(batch)),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

// Frees a batch context. Passing null is safe (no-op).
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_free(ctx: *mut LustroPrngBatch) {
    guarded((), || {
        if !ctx.is_null() {
            drop(Box::from_raw(ctx));
        }
    })
}

// Returns the number of streams, or 0 for null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_len(ctx: *const LustroPrngBatch) -> usize {
    if ctx.is_null() {
        return 0;
    }
    (*ctx).len()
}

// Advances all streams by `steps` rounds.
// `out_len` must equal `batch_len * steps * 32`.
// Output is step-major: the block of `lane` at `step` starts at
// byte offset `(step * batch_len + lane) * 32`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fill_blocks(
    ctx: *mut LustroPrngBatch,
    out: *mut u8,
    out_len: usize,
    steps: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let batch = &mut *ctx;
        let n = batch.len();

        let expected_len = match n.checked_mul(steps).and_then(|v| v.checked_mul(32)) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        if out_len != expected_len {
            return LustroError::InvalidLength;
        }
        if n == 0 || steps == 0 {
            return LustroError::Ok;
        }
        if out.is_null() {
            return LustroError::InvalidPointer;
        }

        if !fits_slice::<u8>(out_len) {
            return LustroError::InvalidLength;
        }

        let n_blocks = n * steps;
        // SAFETY: `out_len` was validated for `n * steps * 32` bytes.
        let out_blocks: &mut [[u8; 32]] = match slice_out(out as *mut [u8; 32], n_blocks) {
            Some(v) => v,
            None => return LustroError::InvalidPointer,
        };

        batch.fill_blocks(out_blocks, steps);
        LustroError::Ok
    })
}

// Derives one child PRNG per lane from `n` child identifiers.
// IDs are passed as parallel `(hi, lo)` u64 arrays.
// `n` must equal the batch length.
// Returns null on invalid input.
//
// # Safety
// `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork(
    ctx: *const LustroPrngBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    n: usize,
) -> *mut LustroPrngBatch {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let batch = &*ctx;
        if n != batch.len() {
            return std::ptr::null_mut();
        }

        let stream_ids: Vec<StreamId> = if n == 0 {
            Vec::new()
        } else {
            if ids_hi.is_null() || ids_lo.is_null() {
                return std::ptr::null_mut();
            }
            let his = match slice_in(ids_hi, n) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            let los = match slice_in(ids_lo, n) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            his.iter()
                .zip(los.iter())
                .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
                .collect()
        };

        Box::into_raw(Box::new(batch.fork(&stream_ids)))
    })
}

// Derives `k` children per lane. Output is parent-major:
// child `j` of lane `i` is at index `i * k + j`.
// IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
// Returns null on invalid input, `len() * k` overflow, or allocation failure.
//
// # Safety
// `ids_hi` and `ids_lo` must each be valid for `k` elements when `k > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork_many(
    ctx: *const LustroPrngBatch,
    ids_hi: *const u64,
    ids_lo: *const u64,
    k: usize,
) -> *mut LustroPrngBatch {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let batch = &*ctx;
        if batch.len().checked_mul(k).is_none() {
            return std::ptr::null_mut();
        }

        let ids: Vec<StreamId> = if k == 0 {
            Vec::new()
        } else {
            if ids_hi.is_null() || ids_lo.is_null() {
                return std::ptr::null_mut();
            }
            let his = match slice_in(ids_hi, k) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            let los = match slice_in(ids_lo, k) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            his.iter()
                .zip(los.iter())
                .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
                .collect()
        };

        match batch.try_fork_many(&ids) {
            Ok(child) => Box::into_raw(Box::new(child)),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

// Derives a canonical batch: one lane per root, each walked along a path
// of `n_path` identifiers. Root and path IDs are each passed as parallel
// `(hi, lo)` u64 arrays. `n_path` must be nonzero; `n_roots` may be zero.
// Returns null on invalid input, including an empty path.
//
// # Safety
// `seed` must be valid for 32 bytes.
// `roots_hi` and `roots_lo` must each be valid for `n_roots` elements when
// `n_roots > 0`. `path_hi` and `path_lo` must each be valid for `n_path`
// elements when `n_path > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_derive_path(
    seed: *const u8,
    roots_hi: *const u64,
    roots_lo: *const u64,
    n_roots: usize,
    path_hi: *const u64,
    path_lo: *const u64,
    n_path: usize,
) -> *mut LustroPrngBatch {
    guarded(std::ptr::null_mut(), || {
        let seed_bytes = match buf_in(seed, 32) {
            Some(s) if s.len() == 32 => s,
            _ => return std::ptr::null_mut(),
        };
        if n_path == 0 || path_hi.is_null() || path_lo.is_null() {
            return std::ptr::null_mut();
        }
        if n_roots > 0 && (roots_hi.is_null() || roots_lo.is_null()) {
            return std::ptr::null_mut();
        }

        let ids_from_parts = |hi: *const u64, lo: *const u64, n: usize| -> Option<Vec<StreamId>> {
            if n == 0 {
                return Some(Vec::new());
            }
            let his = slice_in(hi, n)?;
            let los = slice_in(lo, n)?;
            Some(
                his.iter()
                    .zip(los.iter())
                    .map(|(&hi, &lo)| StreamId(((hi as u128) << 64) | (lo as u128)))
                    .collect(),
            )
        };
        let roots = match ids_from_parts(roots_hi, roots_lo, n_roots) {
            Some(v) => v,
            None => return std::ptr::null_mut(),
        };
        let path = match ids_from_parts(path_hi, path_lo, n_path) {
            Some(v) => v,
            None => return std::ptr::null_mut(),
        };

        let seed256 = Seed256::from_bytes(seed_bytes.try_into().unwrap());
        Box::into_raw(Box::new(LustroPrngBatch::derive_path(
            &seed256, &roots, &path,
        )))
    })
}

// Derives one child PRNG per lane with sequential IDs starting at `first`.
// `first` is passed as `(hi, lo)` u64 values.
// Returns null on null `ctx`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_fork_range(
    ctx: *const LustroPrngBatch,
    first_hi: u64,
    first_lo: u64,
) -> *mut LustroPrngBatch {
    guarded(std::ptr::null_mut(), || {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let batch = &*ctx;
        let first = ((first_hi as u128) << 64) | (first_lo as u128);

        Box::into_raw(Box::new(batch.fork_range(StreamId(first))))
    })
}

// Returns the snapshot size in bytes, or 0 for null `ctx`.
// Size: `16 + batch_len * 48`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_snapshot_size(ctx: *const LustroPrngBatch) -> usize {
    if ctx.is_null() {
        return 0;
    }
    let batch = &*ctx;
    crate::types::batch_snapshot_encoded_len(batch.len()).unwrap_or(0)
}

// Exports the current batch snapshot.
// `out_len` must equal `lustro_prng_batch_snapshot_size(ctx)`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_export_snapshot(
    ctx: *const LustroPrngBatch,
    out: *mut u8,
    out_len: usize,
) -> LustroError {
    guarded(LustroError::InternalPanic, || {
        if ctx.is_null() {
            return LustroError::InvalidPointer;
        }
        let batch = &*ctx;

        let expected_len = match crate::types::batch_snapshot_encoded_len(batch.len()) {
            Some(v) => v,
            None => return LustroError::InvalidLength,
        };
        if out_len != expected_len {
            return LustroError::InvalidLength;
        }
        let output = match buf_out(out, expected_len) {
            Some(s) => s,
            None => return LustroError::InvalidPointer,
        };

        let snapshot = batch.export_snapshot();
        output.copy_from_slice(&snapshot.to_le_bytes());
        LustroError::Ok
    })
}

// Restores a PRNG batch from `len` snapshot bytes.
// Returns null on invalid input or decoding failure.
//
// # Safety
// `bytes` must be valid for `len` bytes when `len > 0`.
#[no_mangle]
pub unsafe extern "C" fn lustro_prng_batch_import_snapshot(
    bytes: *const u8,
    len: usize,
) -> *mut LustroPrngBatch {
    guarded(std::ptr::null_mut(), || {
        let snapshot_bytes = match buf_in(bytes, len) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        if let Ok(snapshot) = crate::types::LustroPrngBatchSnapshot::from_le_bytes(snapshot_bytes) {
            Box::into_raw(Box::new(LustroPrngBatch::import_snapshot(snapshot)))
        } else {
            std::ptr::null_mut()
        }
    })
}
