pub mod codec;
pub mod enums;
pub mod services;
pub mod stack;

pub use enums::{ErrorClass, ErrorCode, PropertyIdentifier};

#[cfg(not(test))]
pub mod pyo3_bindings;

#[cfg(not(test))]
use pyo3::prelude::*;

/// Python extension module entry point.
#[cfg(not(test))]
#[pymodule]
fn libbacnet(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    pyo3_bindings::register(py, m)
}
