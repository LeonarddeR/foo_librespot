//! Rust bindings for writing foobar2000 2.x components without C++.

pub mod abi;
#[allow(
	non_snake_case,
	non_camel_case_types,
	non_upper_case_globals,
	dead_code,
	clippy::all,
	clippy::pedantic,
	clippy::nursery
)]
mod bindings;
pub mod boundary;
pub mod component;
pub mod console;
pub mod cxx;
pub mod error;
pub mod guid;
pub mod host;
pub mod initquit;
pub mod service;
pub mod strings;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod testing_support;

/// This version of OxiBar.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The `FOOBAR2000_TARGET_VERSION` of the SDK these bindings are transcribed from (2.0).
pub const SDK_TARGET_VERSION: u32 = 81;

#[cfg(test)]
mod tests {
	#[test]
	fn targets_foobar2000_2_0_sdk() {
		assert_eq!(super::SDK_TARGET_VERSION, 81);
	}
}
