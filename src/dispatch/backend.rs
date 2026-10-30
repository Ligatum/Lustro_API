//! Dispatcher backends.
//! Detected CPU capabilities, the scalar or AVX2 choice and the lane kernels.

use super::*;

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
    pub(crate) fn name(self) -> &'static str {
        match self {
            Backend::Scalar => "scalar",
            #[cfg(target_arch = "x86_64")]
            Backend::Avx2(_) => "avx2",
        }
    }

    // Stream policy parameters of this backend.
    pub(crate) fn stream_params(self) -> StreamParams {
        match self {
            Backend::Scalar => SCALAR_STREAM,
            #[cfg(target_arch = "x86_64")]
            Backend::Avx2(_) => AVX2_STREAM,
        }
    }

    // Hash policy parameters of this backend.
    pub(super) fn hash_params(self) -> HashParams {
        match self {
            Backend::Scalar => HASH_PARAMS,
            #[cfg(target_arch = "x86_64")]
            Backend::Avx2(_) => HASH_AVX2_PARAMS,
        }
    }

    // As `hash_params`, for batches of messages of any lengths.
    pub(super) fn hash_var_params(self) -> HashParams {
        match self {
            Backend::Scalar => HASH_VAR_PARAMS,
            #[cfg(target_arch = "x86_64")]
            Backend::Avx2(_) => HASH_AVX2_VAR_PARAMS,
        }
    }

    // Suggested `steps` table of this backend.
    pub(super) fn steps_policy(self) -> StepsPolicy {
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
    pub(super) avx2: Option<Avx2Cap>,
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
pub(super) fn choose_backend(caps: Caps, n: usize) -> Backend {
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
pub(super) unsafe fn process_lanes(
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
