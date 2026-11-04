//! foo_librespot as foobar2000 sees it: the entry point, component version, startup and the
//! services it registers. None of this needs Spotify.

mod common;

use oxibar::abi::componentversion::IID as COMPONENTVERSION;
use oxibar::abi::initquit::IID as INITQUIT;
use oxibar::testing::{component_version_strings, on_init};

#[test]
fn component_loads_and_reports_its_version() {
	let (_, component) = common::load();
	assert_eq!(component.client_version(), 81);
	let service =
		component.create_all(&COMPONENTVERSION).pop().expect("a componentversion service");
	let strings = component_version_strings(&service);
	assert_eq!(strings.name, "foo_librespot");
	assert_eq!(strings.version, "0.1.0");
	assert!(strings.about.contains("Spotify Premium"), "{}", strings.about);
	assert!(strings.about.contains("librespot"), "{}", strings.about);
	assert_eq!(strings.file_name, "foo_librespot");
}

#[test]
fn on_init_prints_loaded_line() {
	let (api, component) = common::load();
	let initquit = component.create_all(&INITQUIT).pop().expect("an initquit service");
	on_init(&initquit);
	assert!(
		api.console_lines().iter().any(|line| line == "foo_librespot: loaded (OxiBar 0.1.0)"),
		"{:?}",
		api.console_lines()
	);
}

#[test]
fn component_registers_all_services() {
	use oxibar::abi::album_art::EXTRACTOR_IID as ALBUM_ART;
	use oxibar::abi::filesystem::IID as FILESYSTEM;
	use oxibar::abi::input::INPUT_ENTRY;
	use oxibar::abi::playlist_loader::IID as PLAYLIST_LOADER;
	use oxibar::abi::preferences::PAGE_IID as PREFERENCES_PAGE;

	let (_, component) = common::load();
	let factories = component.factories();
	for (iid, name) in [
		(COMPONENTVERSION, "componentversion"),
		(INITQUIT, "initquit"),
		(INPUT_ENTRY, "input_entry"),
		(FILESYSTEM, "filesystem"),
		(PLAYLIST_LOADER, "playlist_loader"),
		(ALBUM_ART, "album_art_extractor"),
		(PREFERENCES_PAGE, "preferences_page"),
	] {
		assert_eq!(factories.iter().filter(|guid| **guid == iid).count(), 1, "{name}");
	}
	assert_eq!(factories.len(), 7);
}
