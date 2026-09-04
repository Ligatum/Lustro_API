//! Lustro V1 — Parallel and Scalar Engine Dispatcher.

#![allow(non_snake_case)]

use std::sync::OnceLock;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

// ==========================================
// CONFIG
// ==========================================

// HASH THRESHOLDS
const MT_THRESHOLD_MESSAGES: usize = 1664;
const MT_THRESHOLD_BYTES: usize = 64 * 1024;

// STREAM CHUNK SIZES
const PARALLEL_CHUNK_STATES: usize = 288;
const PARALLEL_CHUNK_STATES_MANY: usize = 34;

// STREAMS THRESHOLDS
const MT_THRESHOLD_STREAMS: usize = 1536;
const MT_THRESHOLD_STREAMS_MANY: usize = 1536;

// ==========================================
// THREAD TOPOLOGY FLAGS
// ==========================================

/// Whether to exclude SMT siblings from the worker pool.
const HT_BLOCK: bool = false;

/// Whether to pin Rayon workers to physical cores.
const PIN_THREADS: bool = false;

// ==========================================
// CPU TOPOLOGY
// ==========================================

/// Cached SMT ratio from CPUID leaf 0xB.
static HT_RATIO: OnceLock<usize> = OnceLock::new();

fn ht_ratio() -> usize {
    *HT_RATIO.get_or_init(|| {
        #[cfg(target_arch = "x86_64")]
        {
            detect_ht_ratio_x86()
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            1
        }
    })
}

#[cfg(target_arch = "x86_64")]
fn detect_ht_ratio_x86() -> usize {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    unsafe {
        let max_leaf = __cpuid(0).eax;
        if max_leaf < 0xB {
            return 1;
        }
        let mut level: u32 = 0;
        loop {
            let res = __cpuid_count(0xB, level);
            let level_type = (res.ecx >> 8) & 0xFF;
            let count      = (res.ebx & 0xFFFF) as usize;
            if level_type == 1 && count > 0 {
                return count;
            }
            if count == 0 {
                break;
            }
            level += 1;
            if level > 8 {
                break;
            }
        }
        1
    }
}

// ==========================================
// THREAD PINNING
// ==========================================

#[cfg(all(target_os = "windows", feature = "thread-pinning"))]
fn pin_thread_to_core(core_id: usize) {
    extern "system" {
        fn GetCurrentThread() -> *mut core::ffi::c_void;
        fn SetThreadAffinityMask(
            h_thread:               *mut core::ffi::c_void,
            dw_thread_affinity_mask: usize,
        ) -> usize;
    }
    let mask = match 1usize.checked_shl(core_id as u32) {
        Some(m) if m != 0 => m,
        _ => return,
    };
    unsafe {
        let result = SetThreadAffinityMask(GetCurrentThread(), mask);
        debug_assert_ne!(result, 0, "SetThreadAffinityMask failed");
    }
}

#[cfg(not(all(target_os = "windows", feature = "thread-pinning")))]
#[inline(always)]
fn pin_thread_to_core(_core_id: usize) {}

// ==========================================
// RAYON POOL
// ==========================================

/// Assumes a uniform SMT ratio; hybrid P/E-core topologies may differ.
static RAYON_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

/// Initializes the global Rayon thread pool once.
pub(crate) fn init_pool() {
    RAYON_POOL.get_or_init(|| {
        let logical = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let ht_active = HT_BLOCK
            && std::env::var_os("LUSTRO_DISABLE_HT").is_none();
        let ratio    = if ht_active { ht_ratio() } else { 1 };
        let physical = (logical / ratio).max(1);

        if physical <= 1 {
            return ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .expect("Failed to create fallback pool");
        }

        let pin_active = PIN_THREADS
            && std::env::var_os("LUSTRO_DISABLE_AFFINITY").is_none();

        ThreadPoolBuilder::new()
            .num_threads(physical)
            .start_handler(move |thread_id| {
                if !pin_active {
                    return;
                }
                let core_id = thread_id.saturating_mul(ratio);
                if core_id < logical {
                    pin_thread_to_core(core_id);
                }
            })
            .build()
            .expect("Failed to create Rayon pool")
    });
}

fn get_or_init_pool() -> &'static rayon::ThreadPool {
    match RAYON_POOL.get() {
        Some(pool) => pool,
        None => {
            init_pool();
            RAYON_POOL.get().unwrap()
        }
    }
}

// ==========================================
// INTERNAL STATE API
// ==========================================

/// Dispatches 256-bit hashing directly into `out`.
pub(crate) fn dispatch_hash256_batch_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 32]]) {
    assert_eq!(messages.len(), out.len(), "dispatch_hash256_batch_into: length mismatch");

    #[inline(always)]
    fn absorb_into(m: &[u8], domain: u128, out: &mut [u8; 32]) {
        let (s0, s1) = crate::api::absorb_with_domain(m, domain);
        out[..16].copy_from_slice(&s0.to_le_bytes());
        out[16..].copy_from_slice(&s1.to_le_bytes());
    }

    let len = messages.len();
    if len == 0 {
        return;
    }
    if len == 1 {
        absorb_into(messages[0], domain, &mut out[0]);
        return;
    }

    let total_bytes: usize = messages.iter().map(|m| m.len()).sum();

    if len >= MT_THRESHOLD_MESSAGES || total_bytes >= MT_THRESHOLD_BYTES {
        let pool = get_or_init_pool();
        pool.install(|| {
            messages.par_iter().copied().zip(out.par_iter_mut())
                .for_each(|(m, o)| absorb_into(m, domain, o));
        });
    } else {
        for (m, o) in messages.iter().copied().zip(out.iter_mut()) {
            absorb_into(m, domain, o);
        }
    }
}

/// Dispatches 128-bit hashing directly into `out`.
/// Serializes only s0.
pub(crate) fn dispatch_hash128_batch_into(messages: &[&[u8]], domain: u128, out: &mut [[u8; 16]]) {
    assert_eq!(messages.len(), out.len(), "dispatch_hash128_batch_into: length mismatch");

    #[inline(always)]
    fn absorb_into(m: &[u8], domain: u128, out: &mut [u8; 16]) {
        let (s0, _) = crate::api::absorb_with_domain(m, domain);
        out.copy_from_slice(&s0.to_le_bytes());
    }

    let len = messages.len();
    if len == 0 {
        return;
    }
    if len == 1 {
        absorb_into(messages[0], domain, &mut out[0]);
        return;
    }

    let total_bytes: usize = messages.iter().map(|m| m.len()).sum();

    if len >= MT_THRESHOLD_MESSAGES || total_bytes >= MT_THRESHOLD_BYTES {
        let pool = get_or_init_pool();
        pool.install(|| {
            messages.par_iter().copied().zip(out.par_iter_mut())
                .for_each(|(m, o)| absorb_into(m, domain, o));
        });
    } else {
        for (m, o) in messages.iter().copied().zip(out.iter_mut()) {
            absorb_into(m, domain, o);
        }
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

#[inline(always)]
fn process_lanes_scalar(lanes: &mut [StreamLane], out: &mut [[u8; 32]]) {
    for (lane, block) in lanes.iter_mut().zip(out.iter_mut()) {
        let (s0, s1) = crate::api::stream_step(lane.s0, lane.s1, lane.step);
        lane.step = lane.step.wrapping_add(1);
        lane.s0 = s0;
        lane.s1 = s1;
        block[..16].copy_from_slice(&s0.to_le_bytes());
        block[16..].copy_from_slice(&s1.to_le_bytes());
    }
}

/// Advances every lane by one lockstep round.
pub(crate) fn dispatch_streams(lanes: &mut [StreamLane], out: &mut [[u8; 32]]) {
    assert_eq!(lanes.len(), out.len(), "dispatch_streams: length mismatch");
    match lanes.len() {
        0 => return,

        len if len == 1 || len < MT_THRESHOLD_STREAMS => {
            process_lanes_scalar(lanes, out);
        }

        _ => {
            let pool = get_or_init_pool();
            pool.install(|| {
                lanes.par_chunks_mut(PARALLEL_CHUNK_STATES)
                    .zip(out.par_chunks_mut(PARALLEL_CHUNK_STATES))
                    .for_each(|(l_chunk, o_chunk)| {
                        process_lanes_scalar(l_chunk, o_chunk);
                    });
            });
        }
    }
}

#[inline(always)]
fn process_lanes_many_scalar(lanes: &mut [StreamLane], out: &mut [[u8; 32]], steps: usize) {
    debug_assert_eq!(
        out.len(), lanes.len() * steps,
        "process_lanes_many_scalar: out length must equal lanes.len() * steps"
    );
    for (lane, block_chunk) in lanes.iter_mut().zip(out.chunks_exact_mut(steps)) {
        let mut cur_s0 = lane.s0;
        let mut cur_s1 = lane.s1;
        let mut step   = lane.step;

        for block in block_chunk.iter_mut() {
            let (next_s0, next_s1) = crate::api::stream_step(cur_s0, cur_s1, step);
            cur_s0 = next_s0;
            cur_s1 = next_s1;
            block[..16].copy_from_slice(&cur_s0.to_le_bytes());
            block[16..].copy_from_slice(&cur_s1.to_le_bytes());
            step = step.wrapping_add(1);
        }

        lane.s0   = cur_s0;
        lane.s1   = cur_s1;
        lane.step = step;
    }
}

/// Advances each lane by `steps` rounds in one dispatch.
/// Output is stream-major: each lane's blocks are stored in step order.
pub(crate) fn dispatch_streams_many(lanes: &mut [StreamLane], out: &mut [[u8; 32]], steps: usize) {
    let n = lanes.len();
    let expected = n.checked_mul(steps)
        .expect("dispatch_streams_many: lanes.len() * steps overflows usize");
    assert_eq!(
        out.len(), expected,
        "dispatch_streams_many: out length must equal lanes.len() * steps"
    );

    if n == 0 || steps == 0 {
        return;
    }

    if steps == 1 {
        dispatch_streams(lanes, out);
        return;
    }

    let total_work = expected;
    let chunk_size = PARALLEL_CHUNK_STATES_MANY;
    let real_chunks = n.div_ceil(chunk_size);

    let go_parallel = real_chunks >= 2 && total_work >= MT_THRESHOLD_STREAMS_MANY;

    if !go_parallel {
        process_lanes_many_scalar(lanes, out, steps);
        return;
    }

    let pool = get_or_init_pool();
    pool.install(|| {
        lanes.par_chunks_mut(chunk_size)
            .zip(out.par_chunks_mut(chunk_size * steps))
            .for_each(|(l_chunk, o_chunk)| {
                process_lanes_many_scalar(l_chunk, o_chunk, steps);
            });
    });
}
