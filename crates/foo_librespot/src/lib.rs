//! foo_librespot: a foobar2000 component that plays Spotify links through librespot.

mod art;
mod backend;
mod errors;
mod fields;
mod input;
mod links;
mod logging;
#[cfg(test)]
mod testing;

use oxibar::album_art::album_art_factory;
use oxibar::component::Factory;
use oxibar::filesystem_service::filesystem_factory;
use oxibar::initquit::{InitQuit, InitQuitService};
use oxibar::input::input_factory;
use oxibar::playlist_loader::playlist_loader_factory;
use oxibar::service::Singleton;

const ABOUT: &str = "Plays Spotify tracks, albums and playlists from their links. Needs Spotify Premium.

Built on librespot (MIT licence), an unofficial Spotify client: Spotify can change its service at any time in a way that stops it working.

Contains interface definitions from the foobar2000 SDK, copyright (c) Peter Pawlowski, used under the SDK licence.";

struct Startup;

impl InitQuit for Startup {
	fn on_init(&self) {
		logging::init();
		log::info!("loaded (OxiBar {})", oxibar::VERSION);
	}

	fn on_quit(&self) {
		backend::shutdown();
	}
}

static STARTUP: Singleton<InitQuitService<Startup>> = Singleton::new(InitQuitService::new(Startup));

oxibar::component! {
	name: "foo_librespot",
	version: env!("CARGO_PKG_VERSION"),
	about: ABOUT,
	services: [
		Factory::init_quit(&STARTUP),
		input_factory::<input::SpotifyInput>(),
		filesystem_factory::<links::SpotifyFilesystem>(),
		playlist_loader_factory::<links::SpotifyLoader>(),
		album_art_factory::<art::SpotifyArt>(),
	],
}
