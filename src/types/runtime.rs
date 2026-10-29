//! Lustro V1 — runtime settings and their errors.

/// Execution settings of this process, as configured.
/// Reading them does not start the thread pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RuntimeInfo {
    /// Backend selected for batches of streams and hashes (fixed and variable length):
    /// `"scalar"` or `"avx2"`. AVX2 is used from 4 lanes (or messages) up. Smaller
    /// batches, fixed-length hashes of empty messages, single hashes and single streams
    /// run scalar.
    pub backend: &'static str,
    /// Configured pool size, not a count of active workers. 0 = batches run on the calling
    /// thread (disabled by the settings or built without `parallel`). A nonzero value does
    /// not guarantee a running pool: if it cannot be built, batches run on the calling thread.
    pub threads: usize,
}

/// Process-wide execution settings for [`configure`](crate::configure).
///
/// A field that is set replaces the environment variable of the same purpose;
/// a field that is not set keeps following the environment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeOptions {
    pub(crate) threads: Option<core::num::NonZeroUsize>,
    pub(crate) allow_simd: Option<bool>,
}

impl RuntimeOptions {
    /// Options that change nothing: every setting follows the environment.
    pub const fn new() -> Self {
        RuntimeOptions {
            threads: None,
            allow_simd: None,
        }
    }

    /// Pool size, capped at the logical CPU count. `1` (after the cap) disables the
    /// pool. Replaces `LUSTRO_NUM_THREADS` and `LUSTRO_DISABLE_MT`.
    #[must_use]
    pub const fn threads(mut self, threads: core::num::NonZeroUsize) -> Self {
        self.threads = Some(threads);
        self
    }

    /// `false` selects the scalar backend. `true` lets the dispatcher use AVX2 where the
    /// CPU and the batch size allow it; it is not a guarantee. Replaces
    /// `LUSTRO_DISABLE_SIMD`.
    #[must_use]
    pub const fn allow_simd(mut self, allow: bool) -> Self {
        self.allow_simd = Some(allow);
        self
    }
}

/// Why [`configure`](crate::configure) was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// The settings are already fixed, by an earlier `configure` or by the first use of
    /// the dispatcher (batch calls, `suggested_steps()`, `runtime_info()`).
    AlreadyConfigured,
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ConfigError::AlreadyConfigured => write!(f, "runtime settings are already fixed"),
        }
    }
}

impl std::error::Error for ConfigError {}
