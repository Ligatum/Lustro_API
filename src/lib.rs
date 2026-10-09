//! Lustro V1 — Hash, PRNG and XOF engine.

pub mod constants;
pub mod errors;
pub mod types;

mod api;
mod core;
#[cfg(target_arch = "x86_64")]
mod core_avx2;
mod dispatch;
pub use api::lustro_api_version;
pub use hash::{
    hash128, hash128_many, hash128_many_into, hash256, hash256_many, hash256_many_into,
};
pub use prng::{LustroPrng, LustroPrngBatch};
pub use types::{Hash128, Hash256, Seed256, StreamId};
pub use xof::{LustroXof, LustroXofBatch};

pub mod hash;
pub mod prng;
pub mod xof;

#[cfg(feature = "python")]
mod python;

#[cfg(feature = "ffi")]
mod ffi;

#[cfg(feature = "python")]
use pyo3::prelude::*;

#[cfg(feature = "python")]
use pyo3::wrap_pyfunction;

/// Hash, PRNG and XOF engine.
#[cfg(feature = "python")]
#[pymodule]
#[pyo3(name = "lustro")]
fn lustro_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(python::hash::hash256_py, m)?)?;
    m.add_function(wrap_pyfunction!(python::hash::hash128_py, m)?)?;
    m.add_function(wrap_pyfunction!(python::hash::hash256_many_py, m)?)?;
    m.add_function(wrap_pyfunction!(python::hash::hash128_many_py, m)?)?;
    m.add_function(wrap_pyfunction!(python::hash::hash256_many_var_py, m)?)?;
    m.add_function(wrap_pyfunction!(python::hash::hash128_many_var_py, m)?)?;
    m.add_class::<python::prng::LustroPrngPy>()?;
    m.add_class::<python::prng::LustroPrngBatchPy>()?;
    m.add_class::<python::xof::LustroXofPy>()?;
    m.add_class::<python::xof::LustroXofBatchPy>()?;
    m.add_function(wrap_pyfunction!(python::lustro_api_version, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("SEED_LEN", types::LUSTRO_SEED_LEN)?;
    m.add("BLOCK_LEN", types::LUSTRO_BLOCK_LEN)?;
    m.add("HASH128_LEN", types::LUSTRO_HASH128_LEN)?;
    m.add("HASH256_LEN", types::LUSTRO_HASH256_LEN)?;
    m.add("SNAPSHOT_LEN", types::LUSTRO_SNAPSHOT_LEN)?;
    m.add(
        "BATCH_SNAPSHOT_HEADER_LEN",
        types::LUSTRO_BATCH_SNAPSHOT_HEADER_LEN,
    )?;
    m.add(
        "BATCH_SNAPSHOT_LANE_LEN",
        types::LUSTRO_BATCH_SNAPSHOT_LANE_LEN,
    )?;
    Ok(())
}
