//! Lustro V1 — Parallel, Scalar and AVX2 Engine Dispatcher.

#![allow(non_snake_case)]

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

// ==========================================
// CONFIG
// ==========================================

// HASH POLICY PARAMETERS
// Hashing uses the scalar implementation on every CPU; batches can still run in Rayon.

// Tunable hash parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HashParams {
    /// Minimum message count to use pool.
    pub(crate) mt_threshold_messages: usize,
    /// Minimum total input bytes to use pool.
    pub(crate) mt_threshold_bytes: usize,
}

const HASH_PARAMS: HashParams = HashParams {
    mt_threshold_messages: 1664,
    mt_threshold_bytes: 64 * 1024,
};

// STREAM POLICY PARAMETERS
// `chunk` must be non-zero, and a multiple of 4 for the AVX2 x4 kernel.

/// Tunable stream parameters of one backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamParams {
    // Lanes per Rayon work item.
    pub(crate) chunk: usize,
    // Minimum `lanes * steps` required for Rayon.
    pub(crate) mt_threshold_work: usize,
}

const SCALAR_STREAM: StreamParams = StreamParams {
    chunk: 32,
    mt_threshold_work: 1536,
};
const _: () = assert!(SCALAR_STREAM.chunk > 0);

#[cfg(target_arch = "x86_64")]
const AVX2_STREAM: StreamParams = StreamParams {
    chunk: 64,
    mt_threshold_work: 2048,
};
#[cfg(target_arch = "x86_64")]
const _: () = assert!(AVX2_STREAM.chunk > 0 && AVX2_STREAM.chunk.is_multiple_of(4));

// AVX2 BACKEND SELECTION
// Minimum lanes for one x4 kernel group.
#[cfg(target_arch = "x86_64")]
const AVX2_MIN_LANES: usize = 4;

// Suggested `steps` by lane count. Same policy for PRNG and XOF.
// Each entry is `(max_n, steps)`; `tail` applies above the last entry.
// Larger `steps` amortizes call cost; for larger batches, reduce `steps`
// to limit cache pressure. Small-N entries also avoid premature MT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StepsPolicy {
    table: &'static [(usize, usize)],
    tail: usize,
}

impl StepsPolicy {
    fn steps_for(self, n: usize) -> usize {
        self.table
            .iter()
            .find(|&&(max_n, _)| n <= max_n)
            .map_or(self.tail, |&(_, steps)| steps)
    }
}

const SCALAR_STEPS: StepsPolicy = StepsPolicy {
    table: &[(95, 16), (111, 12)],
    tail: 32,
};

#[cfg(target_arch = "x86_64")]
const AVX2_STEPS: StepsPolicy = StepsPolicy {
    table: &[
        (64, 32),
        (85, 24),
        (127, 16),
        (159, 12),
        (11_800, 32),
        (14_500, 24),
        (23_500, 16),
        (28_500, 12),
        (43_000, 8),
        (75_000, 4),
        (118_000, 2),
        (180_000, 1),
    ],
    tail: 32,
};

// ==========================================
// RUNTIME CONFIG
// ==========================================

const ENV_DISABLE_MT: &str = "LUSTRO_DISABLE_MT";
const ENV_DISABLE_SIMD: &str = "LUSTRO_DISABLE_SIMD";
const ENV_NUM_THREADS: &str = "LUSTRO_NUM_THREADS";

// Process-wide settings, read once at the first use of the dispatcher (batch calls,
// `suggested_steps()`). Later changes to the environment have no effect.
// They affect execution only; outputs do not depend on them.
//
// - `LUSTRO_DISABLE_MT`: present (any value, `0` included) = never enter the pool.
// - `LUSTRO_DISABLE_SIMD`: present = scalar backend for streams.
// - `LUSTRO_NUM_THREADS=N`: pool size, capped at the logical CPU count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuntimeConfig {
    disable_mt: bool,
    disable_simd: bool,
    threads: Option<NonZeroUsize>,
}

impl RuntimeConfig {
    // Parses runtime settings. `logical` is the logical CPU count.
    fn parse(get: impl Fn(&str) -> Option<OsString>, logical: usize) -> Self {
        let threads = get(ENV_NUM_THREADS)
            .and_then(|v| v.to_str().and_then(|s| s.trim().parse::<usize>().ok()))
            .filter(|&n| n > 0)
            .map(|n| n.min(logical.max(1)))
            .and_then(NonZeroUsize::new);

        RuntimeConfig {
            disable_mt: get(ENV_DISABLE_MT).is_some() || threads.map(NonZeroUsize::get) == Some(1),
            disable_simd: get(ENV_DISABLE_SIMD).is_some(),
            threads,
        }
    }

    // Build plan restrictions from the runtime flags.
    fn overrides(self) -> Overrides {
        Overrides {
            backend: if self.disable_simd {
                BackendPref::Scalar
            } else {
                BackendPref::Auto
            },
            parallel: if self.disable_mt {
                ParallelPref::Single
            } else {
                ParallelPref::Auto
            },
        }
    }
}

fn logical_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

static RUNTIME_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

// Settings from the environment, read on first use only.
fn runtime_config() -> RuntimeConfig {
    *RUNTIME_CONFIG
        .get_or_init(|| RuntimeConfig::parse(|name| std::env::var_os(name), logical_cpus()))
}

// ==========================================
// RAYON POOL
// ==========================================

static RAYON_POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();

// PID of the process that owns pool initialization. 0 = unclaimed.
static POOL_OWNER: AtomicU32 = AtomicU32::new(0);

// Initializes the dispatcher's Rayon pool once.
// A process that inherited the pool through `fork` uses the single-threaded fallback.
pub(crate) fn init_pool() {
    let pid = std::process::id();
    let _ = POOL_OWNER.compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire);
    if POOL_OWNER.load(Ordering::Acquire) != pid {
        return;
    }
    RAYON_POOL.get_or_init(|| {
        let workers = runtime_config()
            .threads
            .map_or_else(logical_cpus, NonZeroUsize::get);

        ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .ok()
    });
}

// None if the pool could not be built or this process inherited it through `fork`.
// Callers then use the calling thread.
fn get_or_init_pool() -> Option<&'static rayon::ThreadPool> {
    init_pool();
    if POOL_OWNER.load(Ordering::Acquire) != std::process::id() {
        return None;
    }
    RAYON_POOL.get().and_then(|opt| opt.as_ref())
}

// ==========================================
// INTERNAL STATE API
// ==========================================

// Dispatches 256-bit hashing directly into `out`.
pub(crate) fn dispatch_hash256_batch_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 32]]) {
    assert_eq!(
        messages.len(),
        out.len(),
        "dispatch_hash256_batch_into: length mismatch"
    );
    let plan = plan_hash_messages(messages, runtime_config().overrides().parallel);
    execute_hash(plan, messages, domain, out, hash256_into);
}

// Dispatches 128-bit hashing directly into `out`.
// Serializes only s0.
pub(crate) fn dispatch_hash128_batch_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 16]]) {
    assert_eq!(
        messages.len(),
        out.len(),
        "dispatch_hash128_batch_into: length mismatch"
    );
    let plan = plan_hash_messages(messages, runtime_config().overrides().parallel);
    execute_hash(plan, messages, domain, out, hash128_into);
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
// HASH PLAN
// ==========================================

// How a hash batch is executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HashParallel {
    // Calling thread only.
    Single,
    // Rayon pool, one message per work item.
    Rayon,
}

// Execution mode chosen for hash batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HashPlan {
    pub(crate) parallel: HashParallel,
}

// Counts messages and sums their lengths before planning.
fn plan_hash_messages(messages: &[&[u8]], parallel: ParallelPref) -> HashPlan {
    let total_bytes = messages
        .iter()
        .map(|m| m.len())
        .fold(0, usize::saturating_add);
    plan_hash_with(parallel, messages.len(), total_bytes)
}

// Chooses the execution mode for `count` messages of `total_bytes` bytes.
// Pure: no pool access.
pub(crate) fn plan_hash(count: usize, total_bytes: usize) -> HashPlan {
    let p = HASH_PARAMS;
    let big = count >= p.mt_threshold_messages || total_bytes >= p.mt_threshold_bytes;
    // One message gives the pool nothing to parallelize.
    let parallel = if count < 2 || !big {
        HashParallel::Single
    } else {
        HashParallel::Rayon
    };
    HashPlan { parallel }
}

// As `plan_hash`, with the caller's restriction.
pub(crate) fn plan_hash_with(parallel: ParallelPref, count: usize, total_bytes: usize) -> HashPlan {
    match parallel {
        ParallelPref::Auto => plan_hash(count, total_bytes),
        ParallelPref::Single => HashPlan {
            parallel: HashParallel::Single,
        },
    }
}

// Runs a hash batch under `plan`. `absorb_into` writes the digest of a message.
fn execute_hash<const N: usize>(
    plan: HashPlan,
    messages: &[&[u8]],
    domain: u128,
    out: &mut [[u8; N]],
    absorb_into: impl Fn(&[u8], u128, &mut [u8; N]) + Sync,
) {
    match plan.parallel {
        HashParallel::Single => {
            for (m, o) in messages.iter().copied().zip(out.iter_mut()) {
                absorb_into(m, domain, o);
            }
        }
        HashParallel::Rayon => match get_or_init_pool() {
            Some(pool) => {
                pool.install(|| {
                    messages
                        .par_iter()
                        .copied()
                        .zip(out.par_iter_mut())
                        .for_each(|(m, o)| absorb_into(m, domain, o));
                });
            }
            None => {
                for (m, o) in messages.iter().copied().zip(out.iter_mut()) {
                    absorb_into(m, domain, o);
                }
            }
        },
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
// Workers write disjoint blocks only (see `dispatch_streams`).
#[derive(Clone, Copy)]
struct OutPtr(*mut [u8; 32]);

unsafe impl Send for OutPtr {}
unsafe impl Sync for OutPtr {}

impl OutPtr {
    // Method (not `.0`) so closures capture the whole wrapper, not the raw field.
    #[inline(always)]
    fn get(self) -> *mut [u8; 32] {
        self.0
    }
}

// Advances `lanes` by `steps` stream steps. The block of lane `first + i` at
// step `s` is written to `out[s * n + first + i]`.
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
    for s in 0..steps {
        let row = out.add(s * n + first);
        for (i, lane) in lanes.iter_mut().enumerate() {
            let (s0, s1) = crate::api::stream_step(lane.s0, lane.s1, lane.step);
            lane.step = lane.step.wrapping_add(1);
            lane.s0 = s0;
            lane.s1 = s1;
            let block = &mut *row.add(i);
            block[..16].copy_from_slice(&s0.to_le_bytes());
            block[16..].copy_from_slice(&s1.to_le_bytes());
        }
    }
}

// ==========================================
// BACKEND
// ==========================================

// AVX2 capability.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Avx2Cap {
    _proof: (),
}

#[cfg(target_arch = "x86_64")]
impl Avx2Cap {
    pub(crate) fn detect() -> Option<Self> {
        std::arch::is_x86_feature_detected!("avx2").then_some(Self { _proof: () })
    }
}

// Execution backend for stream lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Scalar,
    #[cfg(target_arch = "x86_64")]
    Avx2(Avx2Cap),
}

impl Backend {
    // Stream policy parameters of this backend.
    pub(crate) fn stream_params(self) -> StreamParams {
        match self {
            Backend::Scalar => SCALAR_STREAM,
            #[cfg(target_arch = "x86_64")]
            Backend::Avx2(_) => AVX2_STREAM,
        }
    }

    // Suggested `steps` table of this backend.
    fn steps_policy(self) -> StepsPolicy {
        match self {
            Backend::Scalar => SCALAR_STEPS,
            #[cfg(target_arch = "x86_64")]
            Backend::Avx2(_) => AVX2_STEPS,
        }
    }
}

// CPU capabilities that select a backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Caps {
    #[cfg(target_arch = "x86_64")]
    avx2: Option<Avx2Cap>,
}

impl Caps {
    // Detect CPU capabilities.
    pub(crate) fn detect() -> Self {
        Caps {
            #[cfg(target_arch = "x86_64")]
            avx2: Avx2Cap::detect(),
        }
    }
}

// Backend for a workload of `n` lanes.
// AVX2 needs the capability and one full x4 group of lanes.
#[cfg(target_arch = "x86_64")]
fn choose_backend(caps: Caps, n: usize) -> Backend {
    match caps.avx2 {
        Some(cap) if n >= AVX2_MIN_LANES => Backend::Avx2(cap),
        _ => Backend::Scalar,
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn choose_backend(_caps: Caps, _n: usize) -> Backend {
    Backend::Scalar
}

// Runs one lane range on `backend`. AVX2 handles the largest multiple-of-4 prefix;
// the remaining 0..=3 lanes use scalar. Same contract as `process_lanes_scalar`.
#[inline(always)]
unsafe fn process_lanes(
    backend: Backend,
    lanes: &mut [StreamLane],
    out: *mut [u8; 32],
    n: usize,
    first: usize,
    steps: usize,
) {
    match backend {
        Backend::Scalar => process_lanes_scalar(lanes, out, n, first, steps),
        #[cfg(target_arch = "x86_64")]
        Backend::Avx2(_) => {
            let simd_len = lanes.len() & !3;
            let (head, tail) = lanes.split_at_mut(simd_len);
            if !head.is_empty() {
                // SAFETY: `Avx2Cap` exists only after AVX2 detection.
                crate::core_avx2::process_lanes_x4(head, out, n, first, steps);
            }
            if !tail.is_empty() {
                process_lanes_scalar(tail, out, n, first + simd_len, steps);
            }
        }
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

// Execution mode of `backend` for `n` lanes and `work = lanes * steps`.
// `single` keeps the workload on the calling thread.
fn plan_for(backend: Backend, single: bool, n: usize, work: usize) -> DispatchPlan {
    let params = backend.stream_params();
    let parallel = if single || n.div_ceil(params.chunk) < 2 || work < params.mt_threshold_work {
        Parallel::Single
    } else {
        Parallel::Rayon {
            chunk: params.chunk,
        }
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

// Chooses backend and execution mode for `n` lanes and `steps` stream steps, within the
// caller's restrictions. Parallelism uses the stream parameters of the chosen backend.
// Pure: no CPU detection, no pool access.
pub(crate) fn plan_streams_with(
    caps: Caps,
    overrides: Overrides,
    n: usize,
    steps: usize,
) -> DispatchPlan {
    let work = n.saturating_mul(steps);
    let backend = backend_for(caps, overrides, n);
    let single = overrides.parallel == ParallelPref::Single;
    plan_for(backend, single, n, work)
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

// As `dispatch_streams` for the given capability and restriction.
fn dispatch_streams_with(
    caps: Caps,
    overrides: Overrides,
    lanes: &mut [StreamLane],
    out: &mut [[u8; 32]],
    steps: usize,
) {
    let plan = plan_streams_with(caps, overrides, lanes.len(), steps);
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
        Parallel::Rayon { chunk } => match get_or_init_pool() {
            Some(pool) => {
                pool.install(|| {
                    lanes
                        .par_chunks_mut(chunk)
                        .enumerate()
                        .for_each(|(k, l_chunk)| {
                            let first = k * chunk;
                            // SAFETY: chunks cover disjoint lane ranges, and (step, lane)
                            // maps to a unique index, so no two workers write the same block.
                            unsafe { process_lanes(backend, l_chunk, out_ptr.get(), n, first, steps) };
                        });
                });
            }
            None => {
                // SAFETY: fallback to single thread when pool creation fails.
                unsafe { process_lanes(backend, lanes, out_ptr.get(), n, 0, steps) };
            }
        },
    }
}
