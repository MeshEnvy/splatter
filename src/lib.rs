//! Fresnel + FSPL coverage engine (library + optional Python extension).

pub mod dem;
pub mod engine;
pub mod hash;
pub mod kml;
pub mod lora;
pub mod ppm;
pub mod ray_cache;
pub mod session;
pub mod skadi_fetch;

#[cfg(feature = "extension-module")]
mod python;

pub use hash::{splat_input_sha256, Request as CovRequest, SPLAT_CACHE_SCHEMA_VERSION};
pub use session::Session;
