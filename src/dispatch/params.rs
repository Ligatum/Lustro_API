//! Dispatcher policy parameters.
//! Thresholds and chunking for hash batches and stream batches.

use super::*;

// ==========================================
// CONFIG
// ==========================================

// HASH POLICY PARAMETERS
// Hash batches use the backend of their count (AVX2 from 4 messages); single messages use
// the scalar implementation. Batches of either backend can run in Rayon. Batches of
// messages with a common length and batches of messages of any lengths have their own
// AVX2 parameters.

// Tunable hash parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HashParams {
    /// Message count that alone qualifies a batch (of at least 2) for the pool.
    pub(crate) mt_threshold_messages: usize,
    /// Total input bytes that alone qualify a batch (of at least 2) for the pool.
    pub(crate) mt_threshold_bytes: usize,
    /// Rayon work items per pool thread; see `hash_min_len`.
    pub(crate) tasks_per_thread: usize,
}

// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
pub(super) const HASH_PARAMS: HashParams = HashParams {
    mt_threshold_messages: 2048,
    mt_threshold_bytes: 80 * 1024,
    tasks_per_thread: 4,
};
const _: () = assert!(HASH_PARAMS.tasks_per_thread > 0);

// Scalar parameters of batches of messages of any lengths. Same values as `HASH_PARAMS`, tuned
// apart from it.
// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
pub(super) const HASH_VAR_PARAMS: HashParams = HashParams {
    mt_threshold_messages: 2048,
    mt_threshold_bytes: 80 * 1024,
    tasks_per_thread: 4,
};
const _: () = assert!(HASH_VAR_PARAMS.tasks_per_thread > 0);

// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
#[cfg(target_arch = "x86_64")]
pub(super) const HASH_AVX2_PARAMS: HashParams = HashParams {
    mt_threshold_messages: 4096,
    mt_threshold_bytes: 192 * 1024,
    tasks_per_thread: 2,
};
#[cfg(target_arch = "x86_64")]
const _: () = assert!(HASH_AVX2_PARAMS.tasks_per_thread > 0);

// AVX2 parameters of batches of messages of any lengths.
// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
#[cfg(target_arch = "x86_64")]
pub(super) const HASH_AVX2_VAR_PARAMS: HashParams = HashParams {
    mt_threshold_messages: 3072,
    mt_threshold_bytes: 192 * 1024,
    tasks_per_thread: 2,
};
#[cfg(target_arch = "x86_64")]
const _: () = assert!(HASH_AVX2_VAR_PARAMS.tasks_per_thread > 0);

// Fewest messages of a variable-length AVX2 batch that go to the pool: below it there are
// fewer than 4 work items of one group, and the pool did not pay on the calling thread's
// time (measured on 8 and 12 messages).
#[cfg(target_arch = "x86_64")]
pub(super) const HASH_AVX2_VAR_MIN_MT_MESSAGES: usize = 16;

// Messages per AVX2 group. A Rayon work item is a whole number of groups, so a batch of one
// group stays on the calling thread.
#[cfg(target_arch = "x86_64")]
#[cfg_attr(not(feature = "parallel"), allow(dead_code))]
pub(super) const HASH_AVX2_GROUP: usize = 4;

impl HashParams {
    // Rayon or the calling thread for `count` messages of `total_bytes` bytes.
    pub(super) fn parallel_for(self, count: usize, total_bytes: usize) -> HashParallel {
        let big = count >= self.mt_threshold_messages || total_bytes >= self.mt_threshold_bytes;
        // One message gives the pool nothing to parallelize.
        if count < 2 || !big {
            HashParallel::Single
        } else {
            HashParallel::Rayon
        }
    }
}

// STREAM POLICY PARAMETERS

// Lanes per Rayon work item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChunkRule {
    // `ceil4(n / near_tasks)`, cut to `floor4(rounds / steps)` (at least 4), but never
    // below `ceil4(n / threads)`: no more items than pool threads. Multiples of 4 for
    // the AVX2 x4 kernel; the scalar backend uses them as well.
    Adaptive { near_tasks: usize, rounds: usize },
}

impl ChunkRule {
    fn chunk_for(self, n: usize, steps: usize, threads: usize) -> usize {
        let ChunkRule::Adaptive { near_tasks, rounds } = self;
        // Overflow only for n near usize::MAX; one item then.
        let x4 = |v: usize| v.checked_next_multiple_of(4).unwrap_or(v).max(4);
        let even = x4(n.div_ceil(near_tasks));
        let cap = ((rounds / steps.max(1)) & !3).max(4);
        let per_thread = x4(n.div_ceil(threads.max(1)));
        even.min(cap).max(per_thread)
    }
}

/// Tunable stream parameters of one backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamParams {
    pub(crate) chunk: ChunkRule,
    // Minimum `lanes * steps` required for Rayon.
    pub(crate) mt_threshold_work: usize,
    // Minimum Rayon work items; a smaller pool lowers it to the pool size, never below 2.
    pub(crate) min_tasks: usize,
}

impl StreamParams {
    // Lanes per work item for `n` lanes, `steps` steps and `threads` pool workers.
    pub(crate) fn chunk_for(self, n: usize, steps: usize, threads: usize) -> usize {
        self.chunk.chunk_for(n, steps, threads)
    }

    // Rayon or the calling thread for this workload, before caller restrictions.
    // A pool of one worker never gets Rayon.
    pub(crate) fn parallel_for(self, n: usize, steps: usize, threads: usize) -> Parallel {
        if threads < 2 {
            return Parallel::Single;
        }
        let chunk = self.chunk_for(n, steps, threads);
        let min_tasks = self.min_tasks.min(threads).max(2);
        if n.saturating_mul(steps) < self.mt_threshold_work || n.div_ceil(chunk) < min_tasks {
            Parallel::Single
        } else {
            Parallel::Rayon { chunk }
        }
    }
}

// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
pub(super) const SCALAR_STREAM: StreamParams = StreamParams {
    chunk: ChunkRule::Adaptive {
        near_tasks: 8,
        rounds: 512,
    },
    mt_threshold_work: 4096,
    min_tasks: 4,
};
const _: () = assert!(matches!(
    SCALAR_STREAM.chunk,
    ChunkRule::Adaptive { near_tasks, rounds } if near_tasks > 0 && rounds > 0
));

// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
#[cfg(target_arch = "x86_64")]
pub(super) const AVX2_STREAM: StreamParams = StreamParams {
    chunk: ChunkRule::Adaptive {
        near_tasks: 8,
        rounds: 768,
    },
    mt_threshold_work: 6144,
    min_tasks: 4,
};
#[cfg(target_arch = "x86_64")]
const _: () = assert!(matches!(
    AVX2_STREAM.chunk,
    ChunkRule::Adaptive { near_tasks, rounds } if near_tasks > 0 && rounds > 0
));

// AVX2 BACKEND SELECTION
// Minimum lanes for one x4 kernel group.
#[cfg(target_arch = "x86_64")]
pub(super) const AVX2_MIN_LANES: usize = 4;

// Suggested `steps` by lane count. Same policy for PRNG and XOF.
// Each entry is `(max_n, steps)`; `tail` applies above the last entry.
// Larger `steps` amortizes call cost; mid-size batches use fewer steps to limit
// cache pressure. Above the table, `tail` amortizes lane state traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StepsPolicy {
    pub(super) table: &'static [(usize, usize)],
    pub(super) tail: usize,
}

impl StepsPolicy {
    pub(super) fn steps_for(self, n: usize) -> usize {
        self.table
            .iter()
            .find(|&&(max_n, _)| n <= max_n)
            .map_or(self.tail, |&(_, steps)| steps)
    }
}

// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
pub(super) const SCALAR_STEPS: StepsPolicy = StepsPolicy {
    table: &[
        (640, 512),
        (1280, 256),
        (2560, 128),
        (3413, 96),
        (5120, 64),
        (10_000, 32),
        (13_600, 24),
        (20_000, 16),
        (23_500, 12),
        (40_960, 8),
        (81_920, 4),
        (180_000, 1),
    ],
    tail: 4,
};

// Tuned on an Intel Core i5-11600K (6 cores / 12 threads, 12 MB L3).
#[cfg(target_arch = "x86_64")]
pub(super) const AVX2_STEPS: StepsPolicy = StepsPolicy {
    table: &[
        (640, 512),
        (1280, 256),
        (2560, 128),
        (3413, 96),
        (5120, 64),
        (10_240, 32),
        (20_000, 16),
        (32_768, 8),
        (80_000, 4),
        (118_000, 2),
        (190_000, 1),
    ],
    tail: 4,
};
