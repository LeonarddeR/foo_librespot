//! A test-tone foobar2000 component that proves the OxiBar bindings.

use oxibar::component::Factory;
use oxibar::initquit::{InitQuit, InitQuitService};
use oxibar::service::Singleton;

struct Startup;

impl InitQuit for Startup {
	fn on_init(&self) {
		oxibar::console::print(&format!("foo_oxitone: loaded (OxiBar {})", oxibar::VERSION));
	}
}

static STARTUP: Singleton<InitQuitService<Startup>> = Singleton::new(InitQuitService::new(Startup));

oxibar::component! {
	name: "foo_oxitone",
	version: env!("CARGO_PKG_VERSION"),
	about: "Test tone component built with OxiBar.\nPlays oxitone://<Hz> paths.",
	services: [Factory::init_quit(&STARTUP)],
}
