pub mod constants;
pub mod errors;
pub mod types;

mod core;
mod dispatch;
mod api;
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

#[cfg(feature = "python")]
#[pymodule]
fn lustro(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<python::hash::LustroHashPy>()?;
    m.add_class::<python::prng::LustroPrngPy>()?;
    m.add_class::<python::prng::LustroPrngBatchPy>()?;
    m.add_class::<python::xof::LustroXofPy>()?;
    m.add_class::<python::xof::LustroXofBatchPy>()?;
    m.add_function(wrap_pyfunction!(python::lustro_api_version, m)?)?;
    Ok(())
}