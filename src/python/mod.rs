//! Lustro V1 PYTHON interface.

pub mod hash;
pub mod prng;
pub mod xof;

use pyo3::prelude::*;

#[pyfunction]
pub fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}

// Maps SnapshotError to a PyValueError with a specific message per variant.
pub(crate) fn snapshot_error(err: crate::types::SnapshotError) -> pyo3::PyErr {
    use crate::types::SnapshotError;
    match err {
        SnapshotError::UnsupportedVersion => {
            pyo3::exceptions::PyValueError::new_err("unsupported snapshot version")
        }
        SnapshotError::InvalidKind => {
            pyo3::exceptions::PyValueError::new_err("snapshot kind does not match this type")
        }
        SnapshotError::InvalidCursor => {
            pyo3::exceptions::PyValueError::new_err("invalid snapshot cursor value")
        }
        SnapshotError::InvalidLength => {
            pyo3::exceptions::PyValueError::new_err("invalid snapshot length")
        }
    }
}