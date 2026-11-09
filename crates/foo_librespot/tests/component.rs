//! foo_librespot as foobar2000 sees it: the entry point, component version, startup and the
//! services it registers. None of this needs Spotify.

mod common;

use foxide2k::abi::componentversion::IID as COMPONENTVERSION;
use foxide2k::abi::initquit::IID as INITQUIT;
use foxide2k::testing::{component_version_strings, on_init};

#[test]
fn component_loads_and_reports_its_version() {
	let (_, component) = common::load();
	assert_eq!(component.client_version(), 81);
	let service =
		component.create_all(&COMPONENTVERSION).pop().expect("a componentversion service");
	let strings = component_version_strings(&service);
	assert_eq!(strings.name, "Spotify integration using librespot");
	assert_eq!(strings.version, env!("CARGO_PKG_VERSION"));
	assert!(
		strings.about.starts_with(
			"Plays Spotify tracks, albums, playlists and podcasts. Paste or drag a Spotify link to add them."
		),
		"{}",
		strings.about
	);
	assert!(!strings.about.contains("foobar2000"), "{}", strings.about);
	assert!(!strings.about.contains("SDK"), "{}", strings.about);
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
		api.console_lines()
			.iter()
			.any(|line| *line == format!("foo_librespot: loaded (Foxide2k {})", foxide2k::VERSION)),
		"{:?}",
		api.console_lines()
	);
}

#[test]
fn component_registers_all_services() {
	use foxide2k::abi::advconfig::ENTRY_IID as ADVCONFIG_ENTRY;
	use foxide2k::abi::album_art::EXTRACTOR_IID as ALBUM_ART;
	use foxide2k::abi::contextmenu::{
		GROUP_IID as CONTEXTMENU_GROUP, ITEM_IID as CONTEXTMENU_ITEM,
	};
	use foxide2k::abi::filesystem::IID as FILESYSTEM;
	use foxide2k::abi::input::INPUT_ENTRY;
	use foxide2k::abi::mainmenu::{COMMANDS_IID as MAINMENU_COMMANDS, GROUP_IID as MAINMENU_GROUP};
	use foxide2k::abi::playlist_loader::IID as PLAYLIST_LOADER;
	use foxide2k::abi::preferences::PAGE_IID as PREFERENCES_PAGE;

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
	assert_eq!(
		factories.iter().filter(|guid| **guid == ADVCONFIG_ENTRY).count(),
		2,
		"advconfig_entry"
	);
	for (iid, name, count) in [
		(MAINMENU_GROUP, "mainmenu_group", 3),
		(MAINMENU_COMMANDS, "mainmenu_commands", 7),
		(CONTEXTMENU_GROUP, "contextmenu_group", 1),
		(CONTEXTMENU_ITEM, "contextmenu_item", 5),
	] {
		assert_eq!(factories.iter().filter(|guid| **guid == iid).count(), count, "{name}");
	}
	assert_eq!(factories.len(), 25);
}

#[test]
fn advanced_preferences_hold_the_logging_checkbox() {
	use foxide2k::abi::advconfig::{BRANCH_IID, ENTRY_IID, GUID_BRANCH_TOOLS};
	use foxide2k::testing::AdvconfigHandle;

	let (_, component) = common::load();
	let entries: Vec<AdvconfigHandle> =
		component.create_all(&ENTRY_IID).into_iter().map(AdvconfigHandle::new).collect();
	assert_eq!(entries.len(), 2);
	let branch = entries.iter().find(|entry| entry.answers(&BRANCH_IID)).expect("no branch");
	let checkbox = entries.iter().find(|entry| !entry.answers(&BRANCH_IID)).expect("no checkbox");
	assert_eq!(branch.name(), "Spotify");
	assert_eq!(branch.parent(), GUID_BRANCH_TOOLS);
	assert_eq!(checkbox.name(), "Log details to the console, for bug reports");
	assert_eq!(checkbox.parent(), branch.guid());
}

#[test]
fn on_init_hints_at_foo_youtube_when_it_is_loaded() {
	use foxide2k::component::ComponentVersionService;
	use foxide2k::service::new_service;

	let (api, component) = common::load();
	api.add_service(
		COMPONENTVERSION,
		new_service(ComponentVersionService::new("foo_youtube", "1.0", "")),
	);
	on_init(&component.create_all(&INITQUIT).pop().expect("an initquit service"));
	let hint = "foo_librespot: foo_youtube may take open.spotify.com links before foo_librespot sees them. \
		Add the line \"deny spotify.com\" to foo_youtube's host filter in its preferences; spotify: links work either way.";
	assert!(api.console_lines().iter().any(|line| line == hint), "{:?}", api.console_lines());
}

#[test]
fn on_init_says_nothing_about_foo_youtube_without_it() {
	let (api, component) = common::load();
	on_init(&component.create_all(&INITQUIT).pop().expect("an initquit service"));
	assert!(!api.console_lines().iter().any(|line| line.contains("foo_youtube")));
}
