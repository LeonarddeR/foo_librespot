//! Rust bindings for writing foobar2000 2.25+ components without C++. The name is foobar2000
//! plus oxide, which is what rust is.

pub mod abi;
pub mod abort;
pub mod album_art;
pub mod audio_chunk;
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
pub mod config_store;
pub mod console;
pub mod cxx;
pub mod dialog;
pub mod error;
pub mod file_info;
pub mod file_info_impl;
pub mod filesystem;
pub mod filesystem_service;
pub mod guid;
pub mod host;
pub mod initquit;
pub mod input;
pub mod live_info;
pub mod main_thread;
pub mod playback;
pub mod playlist_loader;
pub mod popup;
pub mod preferences;
pub mod service;
pub mod shell;
pub mod strings;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod testing_support;

/// This version of Foxide2k.
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
