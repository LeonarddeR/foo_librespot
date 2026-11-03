//! Rust bindings for writing foobar2000 2.x components without C++.

pub mod abi;
pub mod guid;
pub mod service;

/// The `FOOBAR2000_TARGET_VERSION` of the SDK these bindings are transcribed from (2.0).
pub const SDK_TARGET_VERSION: u32 = 81;

#[cfg(test)]
mod tests {
	#[test]
	fn targets_foobar2000_2_0_sdk() {
		assert_eq!(super::SDK_TARGET_VERSION, 81);
	}
}
