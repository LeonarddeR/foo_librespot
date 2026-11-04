//! foo_librespot: a foobar2000 component that plays Spotify links through librespot.

mod account;
mod art;
mod backend;
mod errors;
mod fields;
mod first_use;
mod input;
mod lifecycle;
mod links;
mod logging;
mod login;
mod login_window;
mod prefs;
mod settings;
#[cfg(test)]
mod testing;
mod worker;

use foxide2k::album_art::album_art_factory;
use foxide2k::component::Factory;
use foxide2k::filesystem_service::filesystem_factory;
use foxide2k::initquit::{InitQuit, InitQuitService};
use foxide2k::input::input_factory;
use foxide2k::playlist_loader::playlist_loader_factory;
use foxide2k::preferences::preferences_page_factory;
use foxide2k::service::Singleton;

const ABOUT: &str = "Plays Spotify tracks, albums and playlists from their links. Needs Spotify Premium.

Built on librespot (MIT licence), an unofficial Spotify client: Spotify can change its service at any time in a way that stops it working.

Contains interface definitions from the foobar2000 SDK, copyright (c) Peter Pawlowski, used under the SDK licence.";

struct Startup;

impl InitQuit for Startup {
	fn on_init(&self) {
		logging::init();
		log::info!("loaded (Foxide2k {})", foxide2k::VERSION);
		if let Some(hint) = errors::youtube_hint(&foxide2k::host::host().loaded_components()) {
			foxide2k::console::print(&format!("foo_librespot: {hint}"));
		}
	}

	fn on_quit(&self) {
		backend::shutdown();
	}
}

static STARTUP: Singleton<InitQuitService<Startup>> = Singleton::new(InitQuitService::new(Startup));

foxide2k::component! {
	name: "foo_librespot",
	version: env!("CARGO_PKG_VERSION"),
	about: ABOUT,
	services: [
		Factory::init_quit(&STARTUP),
		input_factory::<input::SpotifyInput>(),
		filesystem_factory::<links::SpotifyFilesystem>(),
		playlist_loader_factory::<links::SpotifyLoader>(),
		album_art_factory::<art::SpotifyArt>(),
		preferences_page_factory::<prefs::SpotifyPage>(),
	],
}
