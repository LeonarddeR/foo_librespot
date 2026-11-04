//! Throwing and catching foobar2000's MSVC C++ exceptions from Rust.

pub mod catch;
pub mod kind;
pub(crate) mod rtti;
pub mod throw;
mod trampoline;

pub use catch::{HostException, try_host};
pub use kind::ExceptionKind;
pub use throw::throw;
