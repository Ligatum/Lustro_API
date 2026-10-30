//! Dispatcher runtime configuration.
//! Environment variables and `configure`: thread count and SIMD, resolved once at first use.

use super::*;
use crate::types::{ConfigError, RuntimeOptions};
use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::sync::OnceLock;

// ==========================================
// RUNTIME CONFIG
// ==========================================

pub(super) const ENV_DISABLE_MT: &str = "LUSTRO_DISABLE_MT";
pub(super) const ENV_DISABLE_SIMD: &str = "LUSTRO_DISABLE_SIMD";
pub(super) const ENV_NUM_THREADS: &str = "LUSTRO_NUM_THREADS";

// A flag variable is set by `1`, `true`, `on` or `yes`, and not set by `0`, `false`, `off`,
// `no`, an empty value or no variable (case and surrounding whitespace are ignored). Any
// other value, one that is not valid UTF-8 included, sets it: disabling is the safe direction.
pub(super) fn env_flag(get: &impl Fn(&str) -> Option<OsString>, name: &str) -> bool {
    let Some(value) = get(name) else {
        return false;
    };
    let Some(text) = value.to_str() else {
        return true;
    };
    let text = text.trim();
    !(text.is_empty()
        || ["0", "false", "off", "no"]
            .iter()
            .any(|f| text.eq_ignore_ascii_case(f)))
}

// Process-wide settings, fixed once: by `configure()`, or else read from the environment
// at the first use of the dispatcher (batch calls, `suggested_steps()`, `runtime_info()`).
// Later changes to the environment have no effect.
// They select how batches run; for the same inputs and `steps` the output bytes do not
// depend on them. `suggested_steps()` follows the backend, so `LUSTRO_DISABLE_SIMD` can
// change it.
//
// - `LUSTRO_DISABLE_MT`: set (see `env_flag`) = never enter the pool.
// - `LUSTRO_DISABLE_SIMD`: set = scalar backend for stream and hash batches.
// - `LUSTRO_NUM_THREADS=N`: pool size, capped at the logical CPU count.
// - A pool of one worker (set, or one logical CPU) also disables MT.
//
// A field set in `RuntimeOptions` replaces the variables of the same purpose:
// `threads` both `LUSTRO_NUM_THREADS` and `LUSTRO_DISABLE_MT`, `allow_simd`
// `LUSTRO_DISABLE_SIMD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RuntimeConfig {
    pub(super) disable_mt: bool,
    pub(super) disable_simd: bool,
    // Pool size: `threads` if set, else the logical CPU count at parse time.
    pub(super) workers: usize,
}

impl RuntimeConfig {
    // Resolves runtime settings: `options` first, then the environment. `logical` is the
    // logical CPU count.
    pub(super) fn parse(
        options: RuntimeOptions,
        get: impl Fn(&str) -> Option<OsString>,
        logical: usize,
    ) -> Self {
        let (threads, disable_mt) = match options.threads {
            // The environment is not consulted for the pool, `LUSTRO_DISABLE_MT` included.
            Some(n) => {
                let n = n.min(NonZeroUsize::new(logical).unwrap_or(NonZeroUsize::MIN));
                (Some(n), n.get() == 1)
            }
            None => {
                let threads = get(ENV_NUM_THREADS)
                    .and_then(|v| v.to_str().and_then(|s| s.trim().parse::<usize>().ok()))
                    .filter(|&n| n > 0)
                    .map(|n| n.min(logical.max(1)))
                    .and_then(NonZeroUsize::new);
                let disable_mt =
                    env_flag(&get, ENV_DISABLE_MT) || threads.map(NonZeroUsize::get) == Some(1);
                (threads, disable_mt)
            }
        };

        let workers = threads.map_or(logical.max(1), NonZeroUsize::get);

        RuntimeConfig {
            disable_mt: disable_mt || workers < 2,
            disable_simd: options
                .allow_simd
                .map_or_else(|| env_flag(&get, ENV_DISABLE_SIMD), |allow| !allow),
            workers,
        }
    }

    // Build plan restrictions from the runtime flags.
    pub(super) fn overrides(self) -> Overrides {
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

pub(super) fn logical_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

static RUNTIME_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

// Settings of `cell`; if still unset, taken from the environment.
pub(super) fn config_in(
    cell: &OnceLock<RuntimeConfig>,
    get: impl Fn(&str) -> Option<OsString>,
    logical: usize,
) -> RuntimeConfig {
    *cell.get_or_init(|| RuntimeConfig::parse(RuntimeOptions::new(), get, logical))
}

// Fixes the settings of `cell` from `options` unless they are already fixed.
// `OnceLock` decides a race with `config_in`: the loser keeps the winner's value.
pub(super) fn configure_in(
    cell: &OnceLock<RuntimeConfig>,
    options: RuntimeOptions,
    get: impl Fn(&str) -> Option<OsString>,
    logical: usize,
) -> Result<(), ConfigError> {
    cell.set(RuntimeConfig::parse(options, get, logical))
        .map_err(|_| ConfigError::AlreadyConfigured)
}

// Process settings; the environment is read on first use only.
pub(super) fn runtime_config() -> RuntimeConfig {
    if let Some(&cfg) = RUNTIME_CONFIG.get() {
        return cfg;
    }
    config_in(
        &RUNTIME_CONFIG,
        |name| std::env::var_os(name),
        logical_cpus(),
    )
}

pub(crate) fn configure(options: RuntimeOptions) -> Result<(), ConfigError> {
    configure_in(
        &RUNTIME_CONFIG,
        options,
        |name| std::env::var_os(name),
        logical_cpus(),
    )
}
