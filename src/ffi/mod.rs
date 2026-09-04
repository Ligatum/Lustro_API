//! Lustro V1 FFI — C-compatible interface.
//! All functions are #[no_mangle] extern "C".
//! Stateful modules use opaque pointers managed by Rust allocator.

pub mod types;
pub mod hash;
pub mod prng;
pub mod xof;

#[no_mangle]
pub extern "C" fn lustro_api_version() -> u32 {
    crate::api::LUSTRO_API_VERSION
}