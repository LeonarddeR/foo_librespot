//! Loading the built foo_oxitone DLL into Foxide2k's fake foobar2000.

use std::path::PathBuf;

use foxide2k::testing::{FakeApi, LoadedComponent};

/// The DLL under test: `FOO_OXITONE_DLL_PATH`, or the workspace's debug build.
pub fn dll_path() -> PathBuf {
	let path = std::env::var_os("FOO_OXITONE_DLL_PATH").map_or_else(
		|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/foo_oxitone.dll"),
		PathBuf::from,
	);
	assert!(path.exists(), "{} is missing: run `cargo build -p foo_oxitone` first", path.display());
	path
}

/// A fresh fake foobar2000 with foo_oxitone loaded into it.
pub fn load() -> (&'static FakeApi, LoadedComponent) {
	let api = FakeApi::new();
	let component = LoadedComponent::load(&dll_path(), api);
	(api, component)
}
