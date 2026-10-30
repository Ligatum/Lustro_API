//! Lustro V1 — Parallel, Scalar and AVX2 Engine Dispatcher.

#![allow(non_snake_case)]

#[cfg(feature = "parallel")]
use rayon::prelude::*;

mod backend;
mod config;
mod params;
mod pool;
pub(crate) use backend::*;
pub(crate) use config::*;
pub(crate) use params::*;
#[cfg(feature = "python")]
pub(crate) use pool::for_chunks_mut;
#[cfg(feature = "ffi")]
pub(crate) use pool::map_ranges;
#[cfg(feature = "parallel")]
use pool::*;

// ==========================================
// INTERNAL STATE API
// ==========================================

// Dispatches 256-bit hashing of `out.len()` messages of `message_len` bytes each, stored
// back to back in `data`.
pub(crate) fn dispatch_hash256_fixed_into(
    data: &[u8],
    message_len: usize,
    domain: u128,
    out: &mut [[u8; 32]],
) {
    assert_eq!(
        Some(data.len()),
        out.len().checked_mul(message_len),
        "dispatch_hash256_fixed_into: data.len() must equal out.len() * message_len"
    );
    let plan = plan_hash_fixed_with(
        Caps::detect(),
        runtime_config().overrides(),
        out.len(),
        message_len,
    );
    execute_hash_fixed(plan, data, message_len, domain, out, hash256_into);
}

// As `dispatch_hash256_fixed_into`, 128-bit digests.
pub(crate) fn dispatch_hash128_fixed_into(
    data: &[u8],
    message_len: usize,
    domain: u128,
    out: &mut [[u8; 16]],
) {
    assert_eq!(
        Some(data.len()),
        out.len().checked_mul(message_len),
        "dispatch_hash128_fixed_into: data.len() must equal out.len() * message_len"
    );
    let plan = plan_hash_fixed_with(
        Caps::detect(),
        runtime_config().overrides(),
        out.len(),
        message_len,
    );
    execute_hash_fixed(plan, data, message_len, domain, out, hash128_into);
}

// Dispatches 256-bit hashing directly into `out`.
pub(crate) fn dispatch_hash256_var_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 32]]) {
    assert_eq!(
        messages.len(),
        out.len(),
        "dispatch_hash256_var_into: length mismatch"
    );
    let plan = plan_hash_var_with(
        Caps::detect(),
        runtime_config().overrides(),
        messages.len(),
        total_len(messages),
    );
    execute_hash_var(plan, messages, domain, out, hash256_into);
}

// Dispatches 128-bit hashing directly into `out`.
// Serializes only s0.
pub(crate) fn dispatch_hash128_var_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 16]]) {
    assert_eq!(
        messages.len(),
        out.len(),
        "dispatch_hash128_var_into: length mismatch"
    );
    let plan = plan_hash_var_with(
        Caps::detect(),
        runtime_config().overrides(),
        messages.len(),
        total_len(messages),
    );
    execute_hash_var(plan, messages, domain, out, hash128_into);
}

// As `dispatch_hash256_var_into`, from a `RawMessages` table. `total_bytes` is the sum of the
// lengths, saturating.
#[cfg(feature = "ffi")]
pub(crate) fn dispatch_hash256_var_raw_into(
    messages: RawMessages<'_>,
    total_bytes: usize,
    domain: u128,
    out: &mut [[u8; 32]],
) {
    assert_eq!(
        messages.len(),
        out.len(),
        "dispatch_hash256_var_raw_into: length mismatch"
    );
    let plan = plan_hash_var_with(
        Caps::detect(),
        runtime_config().overrides(),
        messages.len(),
        total_bytes,
    );
    execute_hash_var_blocks(plan, messages, domain, out, hash256_into);
}

// 256-bit / 128-bit digests of block-wise messages; see `dispatch_hash_blocks_parallel`.
#[cfg(feature = "python")]
pub(crate) fn dispatch_hash256_blocks_parallel<M: MessageBlocks>(
    messages: M,
    domain: u128,
    out: &mut [[u8; 32]],
) -> bool {
    dispatch_hash_blocks_parallel(messages, domain, out, hash256_into)
}

#[cfg(feature = "python")]
pub(crate) fn dispatch_hash128_blocks_parallel<M: MessageBlocks>(
    messages: M,
    domain: u128,
    out: &mut [[u8; 16]],
) -> bool {
    dispatch_hash_blocks_parallel(messages, domain, out, hash128_into)
}

// 128-bit digests; see `dispatch_hash256_var_raw_into`.
#[cfg(feature = "ffi")]
pub(crate) fn dispatch_hash128_var_raw_into(
    messages: RawMessages<'_>,
    total_bytes: usize,
    domain: u128,
    out: &mut [[u8; 16]],
) {
    assert_eq!(
        messages.len(),
        out.len(),
        "dispatch_hash128_var_raw_into: length mismatch"
    );
    let plan = plan_hash_var_with(
        Caps::detect(),
        runtime_config().overrides(),
        messages.len(),
        total_bytes,
    );
    execute_hash_var_blocks(plan, messages, domain, out, hash128_into);
}

#[inline(always)]
fn hash256_into(m: &[u8], domain: u128, out: &mut [u8; 32]) {
    let (s0, s1) = crate::api::absorb_with_domain(m, domain);
    out[..16].copy_from_slice(&s0.to_le_bytes());
    out[16..].copy_from_slice(&s1.to_le_bytes());
}

#[inline(always)]
fn hash128_into(m: &[u8], domain: u128, out: &mut [u8; 16]) {
    let (s0, _) = crate::api::absorb_with_domain(m, domain);
    out.copy_from_slice(&s0.to_le_bytes());
}

// ==========================================
// HASH PLAN AND EXECUTION
// ==========================================

// How a hash batch is executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HashParallel {
    // Calling thread only.
    Single,
    // Rayon pool; the work item size depends on the backend.
    Rayon,
}

// Execution mode of a scalar hash batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HashPlan {
    pub(crate) parallel: HashParallel,
}

// The plan only compares the total with a `mt_threshold_bytes`, so counting stops there.
const TOTAL_LEN_CAP: usize = 256 * 1024;
const _: () = assert!(HASH_PARAMS.mt_threshold_bytes <= TOTAL_LEN_CAP);
const _: () = assert!(HASH_VAR_PARAMS.mt_threshold_bytes <= TOTAL_LEN_CAP);
#[cfg(target_arch = "x86_64")]
const _: () = assert!(HASH_AVX2_PARAMS.mt_threshold_bytes <= TOTAL_LEN_CAP);
#[cfg(target_arch = "x86_64")]
const _: () = assert!(HASH_AVX2_VAR_PARAMS.mt_threshold_bytes <= TOTAL_LEN_CAP);

// Sum of the message lengths, saturating, or any value at least `TOTAL_LEN_CAP` once the
// sum gets there.
fn total_len(messages: &[&[u8]]) -> usize {
    let mut sum = 0usize;
    for m in messages {
        sum = sum.saturating_add(m.len());
        if sum >= TOTAL_LEN_CAP {
            break;
        }
    }
    sum
}

// Backend and execution mode of a hash batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HashBatchPlan {
    pub(crate) backend: Backend,
    pub(crate) parallel: HashParallel,
}

// Chooses backend and execution mode for `count` messages of `message_len` bytes within
// the caller's restrictions. Parallelism uses the hash parameters of the chosen backend.
// Pure: no CPU detection, no pool access.
pub(crate) fn plan_hash_fixed_with(
    caps: Caps,
    overrides: Overrides,
    count: usize,
    message_len: usize,
) -> HashBatchPlan {
    // Empty messages stay scalar.
    let backend = if message_len == 0 {
        Backend::Scalar
    } else {
        backend_for(caps, overrides, count)
    };
    let parallel = match overrides.parallel {
        ParallelPref::Single => HashParallel::Single,
        ParallelPref::Auto => backend
            .hash_params()
            .parallel_for(count, count.saturating_mul(message_len)),
    };
    HashBatchPlan { backend, parallel }
}

// Chooses backend and execution mode for `count` messages of any lengths, `total_bytes`
// bytes together, within the caller's restrictions. Parallelism uses the hash parameters
// of the chosen backend.
// Pure: no CPU detection, no pool access.
pub(crate) fn plan_hash_var_with(
    caps: Caps,
    overrides: Overrides,
    count: usize,
    total_bytes: usize,
) -> HashBatchPlan {
    let backend = backend_for(caps, overrides, count);
    let parallel = match overrides.parallel {
        ParallelPref::Single => HashParallel::Single,
        ParallelPref::Auto => hash_var_parallel(backend, count, total_bytes),
    };
    HashBatchPlan { backend, parallel }
}

// Rayon or the calling thread for a variable-length batch on `backend`.
fn hash_var_parallel(backend: Backend, count: usize, total_bytes: usize) -> HashParallel {
    #[cfg(target_arch = "x86_64")]
    if matches!(backend, Backend::Avx2(_)) && count < HASH_AVX2_VAR_MIN_MT_MESSAGES {
        return HashParallel::Single;
    }
    backend.hash_var_params().parallel_for(count, total_bytes)
}

// Fewest messages per Rayon work item for `count` messages on `threads` workers:
// `ceil(count / (threads * tasks_per_thread))`, at least 1.
#[cfg(feature = "parallel")]
pub(crate) fn hash_min_len(count: usize, threads: usize) -> usize {
    min_len_for(count, threads, HASH_PARAMS)
}

// As `hash_min_len`, for scalar batches of messages of any lengths.
#[cfg(all(any(feature = "ffi", feature = "python"), feature = "parallel"))]
pub(crate) fn hash_var_min_len(count: usize, threads: usize) -> usize {
    min_len_for(count, threads, HASH_VAR_PARAMS)
}

#[cfg(feature = "parallel")]
fn min_len_for(count: usize, threads: usize, params: HashParams) -> usize {
    let items = threads.max(1).saturating_mul(params.tasks_per_thread);
    count.div_ceil(items).max(1)
}

// Runs a hash batch under `plan`; `params` sets the Rayon work item size.
// `absorb_into` writes the digest of a message.
#[cfg_attr(not(feature = "parallel"), allow(unused_variables))]
fn execute_hash<const N: usize>(
    plan: HashPlan,
    params: HashParams,
    messages: &[&[u8]],
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) {
    match plan.parallel {
        HashParallel::Single => hash_single(messages, domain, out, absorb_into),
        HashParallel::Rayon => {
            #[cfg(feature = "parallel")]
            if let Some(pool) = get_or_init_pool() {
                let min_len = min_len_for(messages.len(), pool.current_num_threads(), params);
                pool.install(|| {
                    messages
                        .par_iter()
                        .copied()
                        .zip(out.par_iter_mut())
                        .with_min_len(min_len)
                        .for_each(|(m, o)| absorb_into(m, domain, o));
                });
                return;
            }
            hash_single(messages, domain, out, absorb_into);
        }
    }
}

// Messages per Rayon work item on the AVX2 path: whole x4 groups, `count` spread over
// `threads * tasks_per_thread` items.
#[cfg(all(feature = "parallel", target_arch = "x86_64"))]
pub(crate) fn hash_chunk_len(count: usize, threads: usize, params: HashParams) -> usize {
    let items = threads.max(1).saturating_mul(params.tasks_per_thread);
    let per_item = count.div_ceil(items);
    per_item
        .checked_next_multiple_of(HASH_AVX2_GROUP)
        .unwrap_or(per_item)
        .max(HASH_AVX2_GROUP)
}

// Runs a hash batch of messages with a common length under `plan`.
// `data.len()` must equal `out.len() * message_len`.
fn execute_hash_fixed<const N: usize>(
    plan: HashBatchPlan,
    data: &[u8],
    message_len: usize,
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) {
    match plan.backend {
        Backend::Scalar => {
            execute_hash_scalar_fixed(plan.parallel, data, message_len, domain, out, absorb_into)
        }
        #[cfg(target_arch = "x86_64")]
        Backend::Avx2(cap) => execute_hash_avx2(cap, plan.parallel, data, message_len, domain, out),
    }
}

// Runs a hash batch of messages of any lengths under `plan`.
fn execute_hash_var<const N: usize>(
    plan: HashBatchPlan,
    messages: &[&[u8]],
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) {
    match plan.backend {
        Backend::Scalar => {
            let plan = HashPlan {
                parallel: plan.parallel,
            };
            execute_hash(plan, HASH_VAR_PARAMS, messages, domain, out, absorb_into);
        }
        #[cfg(target_arch = "x86_64")]
        Backend::Avx2(cap) => execute_hash_var_avx2(cap, plan.parallel, messages, domain, out),
    }
}

// AVX2 kernel for messages of any lengths on the calling thread; same contract as
// `hash_var_x4`, minus the CPU check.
#[cfg(target_arch = "x86_64")]
fn hash_var_avx2<const N: usize>(
    _cap: Avx2Cap,
    messages: &[&[u8]],
    domain: u128,
    out: &mut [[u8; N]],
) {
    // SAFETY: `Avx2Cap` exists only after AVX2 detection.
    unsafe { crate::core_avx2::hash_var_x4::<N>(messages, domain, out) }
}

#[cfg(target_arch = "x86_64")]
fn execute_hash_var_avx2<const N: usize>(
    cap: Avx2Cap,
    parallel: HashParallel,
    messages: &[&[u8]],
    domain: u128,
    out: &mut [[u8; N]],
) {
    match parallel {
        HashParallel::Single => hash_var_avx2(cap, messages, domain, out),
        HashParallel::Rayon => {
            #[cfg(feature = "parallel")]
            if out.len() > HASH_AVX2_GROUP {
                if let Some(pool) = get_or_init_pool() {
                    let chunk =
                        hash_chunk_len(out.len(), pool.current_num_threads(), HASH_AVX2_VAR_PARAMS);
                    pool.install(|| {
                        messages
                            .par_chunks(chunk)
                            .zip(out.par_chunks_mut(chunk))
                            .for_each(|(m, o)| hash_var_avx2(cap, m, domain, o));
                    });
                    return;
                }
            }
            hash_var_avx2(cap, messages, domain, out);
        }
    }
}

// Messages per block rebuilt from a message table; the block is a stack array.
#[cfg(any(feature = "ffi", feature = "python"))]
pub(crate) const BLOCK: usize = 256;

// Messages that are turned into slices block by block on the thread that hashes them, so
// no table of slices is allocated.
#[cfg(any(feature = "ffi", feature = "python"))]
pub(crate) trait MessageBlocks: Copy + Sync {
    fn len(&self) -> usize;

    // Messages `start..end`.
    #[cfg_attr(not(feature = "parallel"), allow(dead_code))]
    fn range(&self, start: usize, end: usize) -> Self;

    // Calls `f(offset, block)` over consecutive blocks of at most `BLOCK` messages.
    fn for_each_block(&self, f: impl FnMut(usize, &[&[u8]]));
}

// Table of message pointers and lengths from the C API.
#[cfg(feature = "ffi")]
#[derive(Clone, Copy)]
pub(crate) struct RawMessages<'a> {
    ptrs: &'a [*const u8],
    lens: &'a [usize],
}

// SAFETY: the table is only read, and `RawMessages::new` requires the tables and every
// message to stay readable and unmodified for `'a`.
#[cfg(feature = "ffi")]
unsafe impl Sync for RawMessages<'_> {}

#[cfg(feature = "ffi")]
impl<'a> RawMessages<'a> {
    // SAFETY: `ptrs` and `lens` have equal length. Every `ptrs[i]` is valid for `lens[i]`
    // readable bytes for all of `'a` (any pointer when the length is 0). Neither the tables
    // nor the message bytes are written by any thread during `'a`, and no message overlaps
    // memory that is written while it is read.
    pub(crate) unsafe fn new(ptrs: &'a [*const u8], lens: &'a [usize]) -> Self {
        debug_assert_eq!(ptrs.len(), lens.len());
        Self { ptrs, lens }
    }
}

#[cfg(feature = "ffi")]
impl MessageBlocks for RawMessages<'_> {
    fn len(&self) -> usize {
        self.ptrs.len()
    }

    fn range(&self, start: usize, end: usize) -> Self {
        Self {
            ptrs: &self.ptrs[start..end],
            lens: &self.lens[start..end],
        }
    }

    fn for_each_block(&self, mut f: impl FnMut(usize, &[&[u8]])) {
        let mut block: [&[u8]; BLOCK] = [&[]; BLOCK];
        let mut at = 0;
        for (ps, ls) in self.ptrs.chunks(BLOCK).zip(self.lens.chunks(BLOCK)) {
            for ((slot, &p), &l) in block.iter_mut().zip(ps).zip(ls) {
                // SAFETY: guaranteed by `new`; a null pointer only comes with length 0.
                *slot = if l == 0 {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(p, l) }
                };
            }
            f(at, &block[..ps.len()]);
            at += ps.len();
        }
    }
}

// As `execute_hash_var`, from messages turned into slices per block; both backends rebuild
// the slices per block.
#[cfg(any(feature = "ffi", feature = "python"))]
fn execute_hash_var_blocks<M: MessageBlocks, const N: usize>(
    plan: HashBatchPlan,
    messages: M,
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) {
    match plan.backend {
        Backend::Scalar => {
            execute_hash_scalar_blocks(plan.parallel, messages, domain, out, absorb_into)
        }
        #[cfg(target_arch = "x86_64")]
        Backend::Avx2(cap) => {
            execute_hash_var_avx2_blocks(cap, plan.parallel, messages, domain, out)
        }
    }
}

// Scalar kernel over block-wise messages on the calling thread.
#[cfg(any(feature = "ffi", feature = "python"))]
fn hash_scalar_blocks<M: MessageBlocks, const N: usize>(
    messages: M,
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: &impl Fn(&[u8], u128, &mut [u8; N]),
) {
    messages.for_each_block(|at, block| {
        hash_single(block, domain, &mut out[at..at + block.len()], absorb_into)
    });
}

// Scalar hashing of block-wise messages. A work item has `hash_var_min_len` messages, as
// the per-message split of `execute_hash` with `HASH_VAR_PARAMS`.
#[cfg(any(feature = "ffi", feature = "python"))]
fn execute_hash_scalar_blocks<M: MessageBlocks, const N: usize>(
    parallel: HashParallel,
    messages: M,
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) {
    match parallel {
        HashParallel::Single => hash_scalar_blocks(messages, domain, out, &absorb_into),
        HashParallel::Rayon => {
            #[cfg(feature = "parallel")]
            if let Some(pool) = get_or_init_pool() {
                let min_len = hash_var_min_len(out.len(), pool.current_num_threads());
                pool.install(|| {
                    out.par_chunks_mut(min_len).enumerate().for_each(|(i, o)| {
                        let start = i * min_len;
                        hash_scalar_blocks(
                            messages.range(start, start + o.len()),
                            domain,
                            o,
                            &absorb_into,
                        );
                    });
                });
                return;
            }
            hash_scalar_blocks(messages, domain, out, &absorb_into);
        }
    }
}

// AVX2 kernel over block-wise messages on the calling thread.
#[cfg(all(any(feature = "ffi", feature = "python"), target_arch = "x86_64"))]
fn hash_var_avx2_blocks<M: MessageBlocks, const N: usize>(
    cap: Avx2Cap,
    messages: M,
    domain: u128,
    out: &mut [[u8; N]],
) {
    messages.for_each_block(|at, block| {
        hash_var_avx2(cap, block, domain, &mut out[at..at + block.len()])
    });
}

#[cfg(all(any(feature = "ffi", feature = "python"), target_arch = "x86_64"))]
fn execute_hash_var_avx2_blocks<M: MessageBlocks, const N: usize>(
    cap: Avx2Cap,
    parallel: HashParallel,
    messages: M,
    domain: u128,
    out: &mut [[u8; N]],
) {
    let run = |m: M, o: &mut [[u8; N]]| hash_var_avx2_blocks(cap, m, domain, o);
    match parallel {
        HashParallel::Single => run(messages, out),
        HashParallel::Rayon => {
            #[cfg(feature = "parallel")]
            if out.len() > HASH_AVX2_GROUP {
                if let Some(pool) = get_or_init_pool() {
                    let chunk =
                        hash_chunk_len(out.len(), pool.current_num_threads(), HASH_AVX2_VAR_PARAMS);
                    pool.install(|| {
                        out.par_chunks_mut(chunk).enumerate().for_each(|(i, o)| {
                            let start = i * chunk;
                            run(messages.range(start, start + o.len()), o);
                        });
                    });
                    return;
                }
            }
            run(messages, out);
        }
    }
}

// Hashes block-wise messages in parallel when the plan for this many messages says so.
// Returns false without touching `out` when it does not. Meant for batches of at least
// `HASH_AVX2_PARAMS.mt_threshold_messages` messages, where the total length does not matter
// to the plan.
#[cfg(feature = "python")]
pub(crate) fn dispatch_hash_blocks_parallel<M: MessageBlocks, const N: usize>(
    messages: M,
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) -> bool {
    assert_eq!(
        messages.len(),
        out.len(),
        "dispatch_hash_blocks_parallel: length mismatch"
    );
    let plan = plan_hash_var_with(
        Caps::detect(),
        runtime_config().overrides(),
        messages.len(),
        TOTAL_LEN_CAP,
    );
    if plan.parallel != HashParallel::Rayon {
        return false;
    }
    execute_hash_var_blocks(plan, messages, domain, out, absorb_into);
    true
}

// Scalar hashing of messages with a common length; same split as `execute_hash`.
// `data.len()` must equal `out.len() * message_len`.
fn execute_hash_scalar_fixed<const N: usize>(
    parallel: HashParallel,
    data: &[u8],
    message_len: usize,
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) {
    let message = |i: usize| &data[i * message_len..(i + 1) * message_len];
    match parallel {
        HashParallel::Single => {
            for (i, o) in out.iter_mut().enumerate() {
                absorb_into(message(i), domain, o);
            }
        }
        HashParallel::Rayon => {
            #[cfg(feature = "parallel")]
            if let Some(pool) = get_or_init_pool() {
                let min_len = hash_min_len(out.len(), pool.current_num_threads());
                pool.install(|| {
                    out.par_iter_mut()
                        .enumerate()
                        .with_min_len(min_len)
                        .for_each(|(i, o)| absorb_into(message(i), domain, o));
                });
                return;
            }
            for (i, o) in out.iter_mut().enumerate() {
                absorb_into(message(i), domain, o);
            }
        }
    }
}

// AVX2 kernel on the calling thread; same contract as `hash_fixed_x4`, minus the CPU check.
#[cfg(target_arch = "x86_64")]
fn hash_fixed_avx2<const N: usize>(
    _cap: Avx2Cap,
    data: &[u8],
    message_len: usize,
    domain: u128,
    out: &mut [[u8; N]],
) {
    // SAFETY: `Avx2Cap` exists only after AVX2 detection.
    unsafe { crate::core_avx2::hash_fixed_x4::<N>(data, message_len, domain, out) }
}

#[cfg(target_arch = "x86_64")]
fn execute_hash_avx2<const N: usize>(
    cap: Avx2Cap,
    parallel: HashParallel,
    data: &[u8],
    message_len: usize,
    domain: u128,
    out: &mut [[u8; N]],
) {
    let run = |d: &[u8], o: &mut [[u8; N]]| hash_fixed_avx2(cap, d, message_len, domain, o);
    match parallel {
        HashParallel::Single => run(data, out),
        HashParallel::Rayon => {
            // A work item has at least 4 messages, so up to 4 make a single item.
            #[cfg(feature = "parallel")]
            if message_len > 0 && out.len() > HASH_AVX2_GROUP {
                if let Some(pool) = get_or_init_pool() {
                    let chunk =
                        hash_chunk_len(out.len(), pool.current_num_threads(), HASH_AVX2_PARAMS);
                    pool.install(|| {
                        data.par_chunks(chunk.saturating_mul(message_len))
                            .zip(out.par_chunks_mut(chunk))
                            .for_each(|(d, o)| run(d, o));
                    });
                    return;
                }
            }
            run(data, out);
        }
    }
}

fn hash_single<const N: usize>(
    messages: &[&[u8]],
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]),
) {
    for (m, o) in messages.iter().copied().zip(out.iter_mut()) {
        absorb_into(m, domain, o);
    }
}

// ==========================================
// BATCH STREAM API
// ==========================================

#[derive(Debug, Clone, Copy)]
pub(crate) struct StreamLane {
    pub(crate) s0: u128,
    pub(crate) s1: u128,
    pub(crate) step: u64,
}

// Raw output pointer shared between workers.
#[derive(Clone, Copy)]
struct OutPtr(*mut [u8; 32]);

// SAFETY: built in `execute_streams` from the exclusively borrowed `out`. Workers get
// disjoint lane ranges and (step, lane) maps to one block, so no two write the same block.
unsafe impl Send for OutPtr {}
unsafe impl Sync for OutPtr {}

impl OutPtr {
    // Method (not `.0`) so closures capture the whole wrapper, not the raw field.
    #[inline(always)]
    fn get(self) -> *mut [u8; 32] {
        self.0
    }
}

// Lanes and steps per block in `process_lanes_scalar`.
const SCALAR_BLOCK_LANES: usize = 4;
const SCALAR_BLOCK_STEPS: usize = 8;

// Advances `lanes` by `steps` stream steps. The block of lane `first + i` at
// step `s` is written to `out[s * n + first + i]`. For `steps > 1` the work goes
// in blocks of `SCALAR_BLOCK_LANES` lanes by `SCALAR_BLOCK_STEPS` steps; a lane
// block keeps its state in locals within a step block.
//
// SAFETY: `out` must be valid for `n * steps` blocks, `first + lanes.len() <= n`,
// and no other thread may write to this lane range.
#[inline(always)]
unsafe fn process_lanes_scalar(
    lanes: &mut [StreamLane],
    out: *mut [u8; 32],
    n: usize,
    first: usize,
    steps: usize,
) {
    debug_assert!(first + lanes.len() <= n);
    if steps == 1 {
        let row = out.add(first);
        for (i, lane) in lanes.iter_mut().enumerate() {
            let (s0, s1) = crate::api::stream_step(lane.s0, lane.s1, lane.step);
            lane.step = lane.step.wrapping_add(1);
            lane.s0 = s0;
            lane.s1 = s1;
            let block = &mut *row.add(i);
            block[..16].copy_from_slice(&s0.to_le_bytes());
            block[16..].copy_from_slice(&s1.to_le_bytes());
        }
        return;
    }
    let mut s_first = 0;
    while s_first < steps {
        let s_end = steps.min(s_first + SCALAR_BLOCK_STEPS);
        let mut base = first;
        for block in lanes.chunks_mut(SCALAR_BLOCK_LANES) {
            let k = block.len();
            let mut st = [(0u128, 0u128, 0u64); SCALAR_BLOCK_LANES];
            for (j, lane) in block.iter().enumerate() {
                st[j] = (lane.s0, lane.s1, lane.step);
            }
            for s in s_first..s_end {
                let row = out.add(s * n + base);
                for (j, lane) in st[..k].iter_mut().enumerate() {
                    let (s0, s1) = crate::api::stream_step(lane.0, lane.1, lane.2);
                    *lane = (s0, s1, lane.2.wrapping_add(1));
                    let dst = &mut *row.add(j);
                    dst[..16].copy_from_slice(&s0.to_le_bytes());
                    dst[16..].copy_from_slice(&s1.to_le_bytes());
                }
            }
            for (lane, &(s0, s1, step)) in block.iter_mut().zip(&st[..k]) {
                lane.s0 = s0;
                lane.s1 = s1;
                lane.step = step;
            }
            base += k;
        }
        s_first = s_end;
    }
}

// ==========================================
// DISPATCH PLAN
// ==========================================

// How a stream workload is executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parallel {
    // Calling thread only.
    Single,
    // Rayon pool, `chunk` lanes per work item.
    Rayon { chunk: usize },
}

// Backend and execution mode for one stream workload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DispatchPlan {
    pub(crate) backend: Backend,
    pub(crate) parallel: Parallel,
}

// Execution mode of `backend` for `n` lanes and `steps` steps on a pool of `threads`
// workers. `single` keeps the workload on the calling thread.
fn plan_for(
    backend: Backend,
    single: bool,
    n: usize,
    steps: usize,
    threads: usize,
) -> DispatchPlan {
    let parallel = if single {
        Parallel::Single
    } else {
        backend.stream_params().parallel_for(n, steps, threads)
    };
    DispatchPlan { backend, parallel }
}

// ==========================================
// PLAN OVERRIDES
// ==========================================

// Backend requested by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackendPref {
    // Planner chooses.
    Auto,
    Scalar,
}

// Execution mode requested by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParallelPref {
    // Planner chooses.
    Auto,
    // Calling thread only; the pool is never entered.
    Single,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Overrides {
    pub(crate) backend: BackendPref,
    pub(crate) parallel: ParallelPref,
}

// Chooses backend and execution mode for `n` lanes and `steps` stream steps on a pool of
// `threads` workers, within the caller's restrictions. Parallelism uses the stream
// parameters of the chosen backend.
// Pure: no CPU detection, no pool access.
pub(crate) fn plan_streams_with(
    caps: Caps,
    overrides: Overrides,
    n: usize,
    steps: usize,
    threads: usize,
) -> DispatchPlan {
    let backend = backend_for(caps, overrides, n);
    let single = overrides.parallel == ParallelPref::Single;
    plan_for(backend, single, n, steps, threads)
}

// Backend for `n` lanes within the caller's restriction.
fn backend_for(caps: Caps, overrides: Overrides, n: usize) -> Backend {
    match overrides.backend {
        BackendPref::Auto => choose_backend(caps, n),
        BackendPref::Scalar => Backend::Scalar,
    }
}

// Suggested `steps` for `n` lanes under the selected backend.
// Pure: no CPU detection or pool access.
pub(crate) fn suggested_steps_with(caps: Caps, overrides: Overrides, n: usize) -> usize {
    backend_for(caps, overrides, n).steps_policy().steps_for(n)
}

// Backend and configured pool size for the given capabilities and config; no pool access.
// `threads` is not a count of active workers: if the pool cannot be built, or this process
// is a child forked after its parent claimed the pool (`POOL_OWNER`), batches still run on
// the calling thread.
fn runtime_info_with(caps: Caps, config: RuntimeConfig) -> crate::types::RuntimeInfo {
    // Backend of the largest batch, within `LUSTRO_DISABLE_SIMD`.
    let backend = backend_for(caps, config.overrides(), usize::MAX);
    #[cfg(feature = "parallel")]
    let threads = if config.disable_mt { 0 } else { config.workers };
    #[cfg(not(feature = "parallel"))]
    let threads = 0;
    crate::types::RuntimeInfo {
        backend: backend.name(),
        threads,
    }
}

// Settings of the current CPU and runtime config. Reads the config like any dispatch.
pub(crate) fn runtime_info() -> crate::types::RuntimeInfo {
    runtime_info_with(Caps::detect(), runtime_config())
}

// Suggested `steps` for the current CPU and runtime config.
pub(crate) fn suggested_steps(n: usize) -> usize {
    suggested_steps_with(Caps::detect(), runtime_config().overrides(), n)
}

// Advances every lane by `steps` stream steps, planned for the capabilities of CPU and the runtime config.
// Output is step-major: `out[step * n + lane]`, with `n = lanes.len()`.
pub(crate) fn dispatch_streams(lanes: &mut [StreamLane], out: &mut [[u8; 32]], steps: usize) {
    dispatch_streams_with(
        Caps::detect(),
        runtime_config().overrides(),
        lanes,
        out,
        steps,
    );
}

// As `dispatch_streams` for the given capability and restriction. The pool size is
// that of the runtime config.
fn dispatch_streams_with(
    caps: Caps,
    overrides: Overrides,
    lanes: &mut [StreamLane],
    out: &mut [[u8; 32]],
    steps: usize,
) {
    let threads = runtime_config().workers;
    let plan = plan_streams_with(caps, overrides, lanes.len(), steps, threads);
    execute_streams(plan, lanes, out, steps);
}

// Runs `plan`. Panics unless `out.len() == lanes.len() * steps`.
fn execute_streams(
    plan: DispatchPlan,
    lanes: &mut [StreamLane],
    out: &mut [[u8; 32]],
    steps: usize,
) {
    let n = lanes.len();
    let expected = n
        .checked_mul(steps)
        .expect("dispatch_streams: lanes.len() * steps overflows usize");
    assert_eq!(
        out.len(),
        expected,
        "dispatch_streams: out length must equal lanes.len() * steps"
    );

    if n == 0 || steps == 0 {
        return;
    }

    let backend = plan.backend;
    let out_ptr = OutPtr(out.as_mut_ptr());

    match plan.parallel {
        Parallel::Single => {
            // SAFETY: one call covers all lanes; out holds n * steps blocks.
            unsafe { process_lanes(backend, lanes, out_ptr.get(), n, 0, steps) };
        }
        Parallel::Rayon { chunk } => {
            #[cfg(feature = "parallel")]
            if let Some(pool) = get_or_init_pool() {
                pool.install(|| {
                    lanes
                        .par_chunks_mut(chunk)
                        .enumerate()
                        .for_each(|(k, l_chunk)| {
                            let first = k * chunk;
                            // SAFETY: chunks cover disjoint lane ranges, and (step, lane)
                            // maps to a unique index, so no two workers write the same block.
                            unsafe {
                                process_lanes(backend, l_chunk, out_ptr.get(), n, first, steps)
                            };
                        });
                });
                return;
            }
            #[cfg(not(feature = "parallel"))]
            let _ = chunk;
            // SAFETY: fallback to single thread when there is no pool.
            unsafe { process_lanes(backend, lanes, out_ptr.get(), n, 0, steps) };
        }
    }
}
