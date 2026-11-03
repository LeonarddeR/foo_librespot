//! foo_oxitone as foobar2000 sees it: the entry point, component version and startup.

mod common;

use oxibar::abi::componentversion::IID as COMPONENTVERSION;
use oxibar::abi::initquit::IID as INITQUIT;
use oxibar::testing::{CountedService, component_version_strings, on_init};

#[test]
fn client_reports_version_81() {
	let (_, component) = common::load();
	assert_eq!(component.client_version(), 81);
}

#[test]
fn registers_exactly_one_componentversion() {
	let (_, component) = common::load();
	let versions =
		component.factories().into_iter().filter(|guid| *guid == COMPONENTVERSION).count();
	assert_eq!(versions, 1);
}

#[test]
fn componentversion_reports_name_version_about() {
	let (_, component) = common::load();
	let service =
		component.create_all(&COMPONENTVERSION).pop().expect("a componentversion service");
	let strings = component_version_strings(&service);
	assert_eq!(strings.name, "foo_oxitone");
	assert_eq!(strings.version, "0.1.0");
	assert!(strings.about.contains("oxitone://"), "{}", strings.about);
	assert_eq!(strings.file_name, "foo_oxitone");
}

#[test]
fn on_init_prints_loaded_line() {
	let (api, component) = common::load();
	let initquit = component.create_all(&INITQUIT).pop().expect("an initquit service");
	on_init(&initquit);
	assert!(
		api.console_lines().iter().any(|line| line == "foo_oxitone: loaded (OxiBar 0.1.0)"),
		"{:?}",
		api.console_lines()
	);
}

#[test]
fn instance_create_releases_previous_out() {
	let (_, component) = common::load();
	let (previous, releases) = CountedService::new();
	let created = component.create_with_previous(&COMPONENTVERSION, previous);
	assert_eq!(releases.get(), 1);
	assert!(created.query(&COMPONENTVERSION).is_some());
}
