//! Lustro V1 — errors of batch construction and path derivation.

/// Why a path derivation was rejected by a `try_derive_path` function.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DerivePathError {
    /// `path` is empty.
    EmptyPath,
    /// Reserving the lane buffer failed (allocator refusal or capacity overflow).
    /// Batch functions only.
    Reserve(std::collections::TryReserveError),
}

impl core::fmt::Display for DerivePathError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DerivePathError::EmptyPath => write!(f, "path must not be empty"),
            DerivePathError::Reserve(_) => write!(f, "lane buffer reservation failed"),
        }
    }
}

impl std::error::Error for DerivePathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DerivePathError::EmptyPath => None,
            DerivePathError::Reserve(e) => Some(e),
        }
    }
}

/// Why a batch could not be built by `try_new_range`, `try_fork_many` or `try_clone`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchError {
    /// The lane count overflows `usize`.
    SizeOverflow,
    /// Reserving the lane buffer failed (allocator refusal or capacity overflow).
    Reserve(std::collections::TryReserveError),
}

impl core::fmt::Display for BatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BatchError::SizeOverflow => write!(f, "lane count overflows usize"),
            BatchError::Reserve(_) => write!(f, "lane buffer reservation failed"),
        }
    }
}

impl std::error::Error for BatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BatchError::SizeOverflow => None,
            BatchError::Reserve(e) => Some(e),
        }
    }
}

// Lane count of `fork_many`: `lanes * ids`.
#[inline]
pub(crate) fn fork_many_lane_count(lanes: usize, ids: usize) -> Result<usize, BatchError> {
    lanes.checked_mul(ids).ok_or(BatchError::SizeOverflow)
}
