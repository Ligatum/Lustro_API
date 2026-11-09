//! Lustro V1 — errors of batch construction and path derivation.

/// Why a path derivation was rejected by a `try_derive_path` or `try_fork_path` function.
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

/// Why a batch operation failed in `try_new`, `try_new_range`, `try_from_streams`, `try_fork`, `try_fork_many`, `try_fork_range`, `try_fork_batch`, `try_fork_batch_range`, `try_clone`, `try_blocks`, `try_export_snapshot` or `try_to_le_bytes`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchError {
    /// A computed size overflows `usize`: the lane count of `try_fork_many`
    /// (`len() * ids.len()`) or the block count of `try_blocks` (`len() * steps`).
    SizeOverflow,
    /// Reserving a buffer failed (allocator refusal or capacity overflow).
    Reserve(std::collections::TryReserveError),
    /// The number of identifiers given to `try_fork` differs from the batch length.
    LengthMismatch { expected: usize, actual: usize },
    /// Stream `index` given to `try_from_streams` is inside a block: part of its current
    /// block is unread and a lane cannot hold it.
    PartialBlock { index: usize },
}

impl core::fmt::Display for BatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BatchError::SizeOverflow => write!(f, "size overflows usize"),
            BatchError::Reserve(_) => write!(f, "buffer reservation failed"),
            BatchError::LengthMismatch { expected, actual } => {
                write!(f, "expected {expected} identifiers, got {actual}")
            }
            BatchError::PartialBlock { index } => {
                write!(f, "stream {index} is inside a block")
            }
        }
    }
}

impl std::error::Error for BatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BatchError::SizeOverflow
            | BatchError::LengthMismatch { .. }
            | BatchError::PartialBlock { .. } => None,
            BatchError::Reserve(e) => Some(e),
        }
    }
}

// Lane count of `fork_many`: `lanes * ids`.
#[inline]
pub(crate) fn fork_many_lane_count(lanes: usize, ids: usize) -> Result<usize, BatchError> {
    lanes.checked_mul(ids).ok_or(BatchError::SizeOverflow)
}
