//! A test-tone foobar2000 component that proves the Foxide2k bindings.

mod input;
mod tone;

use foxide2k::component::Factory;
use foxide2k::initquit::{InitQuit, InitQuitService};
use foxide2k::input::input_factory;
use foxide2k::service::Singleton;

struct Startup;

impl InitQuit for Startup {
	fn on_init(&self) {
		foxide2k::console::print(&format!("foo_oxitone: loaded (Foxide2k {})", foxide2k::VERSION));
	}
}

static STARTUP: Singleton<InitQuitService<Startup>> = Singleton::new(InitQuitService::new(Startup));

foxide2k::component! {
	name: "foo_oxitone",
	version: env!("CARGO_PKG_VERSION"),
	about: "Test tone component built with Foxide2k.\nPlays oxitone://<Hz> paths.",
	services: [Factory::init_quit(&STARTUP), input_factory::<input::ToneInput>()],
}
