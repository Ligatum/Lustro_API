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
fn lustro(m: &Bound<'_, PyModule>) -> PyResult<()> {
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
    Ok(())
}
