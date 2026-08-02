//! Language bindings: C FFI and Python (PyO3).

#[cfg(feature = "ffi")]
pub mod ffi;
#[cfg(feature = "python")]
pub mod python;
