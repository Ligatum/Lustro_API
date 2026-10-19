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

/// Why a batch operation failed in `try_new_range`, `try_fork_many`, `try_clone` or `try_blocks`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchError {
    /// A computed size overflows `usize`: the lane count of `try_fork_many`
    /// (`len() * ids.len()`) or the block count of `try_blocks` (`len() * steps`).
    SizeOverflow,
    /// Reserving a buffer failed (allocator refusal or capacity overflow).
    Reserve(std::collections::TryReserveError),
}

impl core::fmt::Display for BatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BatchError::SizeOverflow => write!(f, "size overflows usize"),
            BatchError::Reserve(_) => write!(f, "buffer reservation failed"),
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
