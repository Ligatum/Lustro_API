//! Dispatcher thread pool.
//! The Rayon pool and the chunked helpers that run on it.

#[cfg(feature = "parallel")]
use super::*;
#[cfg(feature = "parallel")]
use rayon::ThreadPoolBuilder;
#[cfg(feature = "parallel")]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(feature = "parallel")]
use std::sync::OnceLock;

// ==========================================
// RAYON POOL
// ==========================================

#[cfg(feature = "parallel")]
static RAYON_POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();

// PID of the process that owns pool initialization. 0 = unclaimed.
#[cfg(feature = "parallel")]
static POOL_OWNER: AtomicU32 = AtomicU32::new(0);

// Initializes the dispatcher's Rayon pool once.
// A process whose parent already claimed the pool (`POOL_OWNER` holds the parent's PID) uses
// the single-threaded fallback. A child forked before any claim may build its own pool.
#[cfg(feature = "parallel")]
pub(super) fn init_pool() {
    let pid = std::process::id();
    let _ = POOL_OWNER.compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire);
    if POOL_OWNER.load(Ordering::Acquire) != pid {
        return;
    }
    RAYON_POOL.get_or_init(|| {
        let workers = runtime_config().workers;
        ThreadPoolBuilder::new().num_threads(workers).build().ok()
    });
}

// None if the pool could not be built or `POOL_OWNER` holds another process's PID (the
// parent claimed the pool before `fork`). Callers then use the calling thread.
#[cfg(feature = "parallel")]
pub(super) fn get_or_init_pool() -> Option<&'static rayon::ThreadPool> {
    init_pool();
    if POOL_OWNER.load(Ordering::Acquire) != std::process::id() {
        return None;
    }
    RAYON_POOL.get().and_then(|opt| opt.as_ref())
}

// Runs `f(start, chunk)` over `items` in chunks on the Rayon pool when `items` has at least
// `min_total` elements and the process settings allow threads; otherwise once on the calling
// thread over the whole slice. `f` must not call into Python.
#[cfg(feature = "python")]
pub(crate) fn for_chunks_mut<T: Send>(
    items: &mut [T],
    min_total: usize,
    f: impl Fn(usize, &mut [T]) + Sync,
) {
    #[cfg(feature = "parallel")]
    if items.len() >= min_total && runtime_config().overrides().parallel != ParallelPref::Single {
        if let Some(pool) = get_or_init_pool() {
            let chunk = items
                .len()
                .div_ceil(pool.current_num_threads().max(1) * 4)
                .max(1);
            pool.install(|| {
                items
                    .par_chunks_mut(chunk)
                    .enumerate()
                    .for_each(|(i, c)| f(i * chunk, c));
            });
            return;
        }
    }
    #[cfg(not(feature = "parallel"))]
    let _ = min_total;
    f(0, items);
}

// Runs `f` over `0..n` in index-ordered ranges on the Rayon pool when `n` is at least
// `min_total` and the process settings allow threads; otherwise once over the whole range
// on the calling thread. The results come back in range order.
#[cfg(feature = "ffi")]
pub(crate) fn map_ranges<R: Send>(
    n: usize,
    min_total: usize,
    f: impl Fn(std::ops::Range<usize>) -> R + Sync,
) -> Vec<R> {
    #[cfg(feature = "parallel")]
    if n >= min_total && runtime_config().overrides().parallel != ParallelPref::Single {
        if let Some(pool) = get_or_init_pool() {
            let chunk = n.div_ceil(pool.current_num_threads().max(1) * 4).max(1);
            return pool.install(|| {
                (0..n.div_ceil(chunk))
                    .into_par_iter()
                    .map(|i| f(i * chunk..((i + 1) * chunk).min(n)))
                    .collect()
            });
        }
    }
    #[cfg(not(feature = "parallel"))]
    let _ = min_total;
    vec![f(0..n)]
}
