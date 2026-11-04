//! Loading the foo_librespot DLL this test build made into Foxide2k's fake foobar2000.

use foxide2k::testing::{FakeApi, LoadedComponent};

/// A fresh fake foobar2000 with foo_librespot loaded into it. `FOO_LIBRESPOT_DLL_PATH` names
/// another DLL to test.
pub fn load() -> (&'static FakeApi, LoadedComponent) {
	LoadedComponent::load_built("foo_librespot", "FOO_LIBRESPOT_DLL_PATH")
}
