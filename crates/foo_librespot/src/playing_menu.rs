//! File > Spotify > Playing track: opens the album, an artist, the radio or the show of the
//! Spotify track or episode playing now, or adds it to a playlist.
//!
//! The commands follow the right-click rules ([`crate::spotify_menu`]) for the playing track
//! alone, except that they are greyed out rather than hidden when nothing plays or the playing
//! track isn't from Spotify. "Add to playlist…" takes a track or an episode alike. A bound key
//! runs a command whatever the menu would show, so each command reads the playing track again
//! when it runs; when that isn't a Spotify item of its kind, a popup says to play one instead.

use foo_librespot_core::related::Related;
use foxide2k::abi::mainmenu::SORT_PRIORITY_BASE;
use foxide2k::contextmenu::Shown;
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::mainmenu::{MainMenuCommand, MainMenuPopup};
use foxide2k::playback;

use crate::open_playlist::SpotifyMenu;
use crate::playlist_window;
use crate::spotify_menu::{
	Kind, Source, SpotifyItem, open, open_artists, shown_among, spotify_item,
};

/// The File > Spotify > Playing track submenu, after Open playlist….
pub struct PlayingTrack;

impl MainMenuPopup for PlayingTrack {
	const GUID: Guid = Guid::from_u128(0x1120bc99_c585_4c5e_a299_9ede596b39e2);
	const PARENT: Guid = SpotifyMenu::GUID;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE + 1;
	const NAME: &'static str = "Playing track";
}

/// Opens the playing track's album.
pub struct PlayingAlbum;

impl MainMenuCommand for PlayingAlbum {
	const GUID: Guid = Guid::from_u128(0x11865a76_0fc0_43ac_8bc9_d348e57ecc9e);
	const PARENT: Guid = PlayingTrack::GUID;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE;
	const NAME: &'static str = "Open album";
	const DESCRIPTION: &'static str = "Opens the playing Spotify track's album in a playlist tab.";

	fn shown() -> Shown {
		shown(playing().as_ref(), Kind::Track)
	}

	fn execute() -> Result<()> {
		open(playing(), Source::Playing, Self::NAME, Related::Album);
		Ok(())
	}
}

/// Opens the playing track's artist when Spotify names one, or lists its artists in the artist
/// window, as the right-click "Open artist…" does.
pub struct PlayingArtist;

impl MainMenuCommand for PlayingArtist {
	const GUID: Guid = Guid::from_u128(0x00fbed21_419c_4b8a_905f_b0047d78b2ac);
	const PARENT: Guid = PlayingTrack::GUID;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE + 1;
	const NAME: &'static str = "Open artist…";
	const DESCRIPTION: &'static str = "Opens the artist of the playing Spotify track in a playlist tab, or lists its artists to pick from.";

	fn shown() -> Shown {
		shown(playing().as_ref(), Kind::Track)
	}

	fn execute() -> Result<()> {
		open_artists(playing(), Source::Playing, Self::NAME);
		Ok(())
	}
}

/// Opens Spotify's radio for the playing track.
pub struct PlayingRadio;

impl MainMenuCommand for PlayingRadio {
	const GUID: Guid = Guid::from_u128(0x41fb8968_b48e_4eca_b351_e44de55db978);
	const PARENT: Guid = PlayingTrack::GUID;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE + 2;
	const NAME: &'static str = "Track radio";
	const DESCRIPTION: &'static str =
		"Opens Spotify's radio for the playing track in a playlist tab.";

	fn shown() -> Shown {
		shown(playing().as_ref(), Kind::Track)
	}

	fn execute() -> Result<()> {
		open(playing(), Source::Playing, Self::NAME, Related::Radio);
		Ok(())
	}
}

/// Opens the playing episode's show.
pub struct PlayingShow;

impl MainMenuCommand for PlayingShow {
	const GUID: Guid = Guid::from_u128(0x69006f81_9584_40b4_b221_dbac816f45da);
	const PARENT: Guid = PlayingTrack::GUID;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE + 3;
	const NAME: &'static str = "Open show";
	const DESCRIPTION: &'static str = "Opens the playing Spotify episode's show in a playlist tab.";

	fn shown() -> Shown {
		shown(playing().as_ref(), Kind::Episode)
	}

	fn execute() -> Result<()> {
		open(playing(), Source::Playing, Self::NAME, Related::Show);
		Ok(())
	}
}

/// Opens the Add to playlist window for the playing track or episode, as the right-click "Add to
/// playlist…" does for the selection.
pub struct PlayingAddToPlaylist;

impl MainMenuCommand for PlayingAddToPlaylist {
	const GUID: Guid = Guid::from_u128(0x5586b6ac_c3a2_4a6c_9b98_773c8aca259b);
	const PARENT: Guid = PlayingTrack::GUID;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE + 4;
	const NAME: &'static str = "Add to playlist…";
	const DESCRIPTION: &'static str = "Adds the playing Spotify track to Liked Songs or your Spotify playlists, or removes it, in a window that saves on OK.";

	fn shown() -> Shown {
		if playing().is_some() { Shown::Enabled } else { Shown::Greyed }
	}

	fn execute() -> Result<()> {
		playlist_window::open(playing(), Source::Playing, Self::NAME);
		Ok(())
	}
}

/// The playing track, when it is a Spotify track or episode; `None` when nothing plays, the
/// playing track isn't from Spotify, or foobar2000 can't say what plays. Main thread only.
fn playing() -> Option<SpotifyItem> {
	playback::now_playing().ok().flatten().and_then(spotify_item)
}

/// How a command that opens from `kind` shows for the `playing` item: greyed out when there is
/// none, or else as the right-click rules say.
fn shown(playing: Option<&SpotifyItem>, kind: Kind) -> Shown {
	playing.map_or(Shown::Greyed, |(item, _)| shown_among([*item], kind))
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::related::Related;
	use foxide2k::abi::mainmenu::{COMMANDS_V2_IID, FLAG_DISABLED, SORT_PRIORITY_BASE};
	use foxide2k::dialog::host::destroy;
	use foxide2k::guid::Guid;
	use foxide2k::mainmenu::{
		MainMenuCommand, MainMenuPopup, mainmenu_command_service, mainmenu_popup_service,
	};
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{MainMenuHandle, pump_messages};

	use super::{
		PlayingAddToPlaylist, PlayingAlbum, PlayingArtist, PlayingRadio, PlayingShow, PlayingTrack,
	};
	use crate::artist_window::{STATUS, WINDOW};
	use crate::open_playlist::{OpenPlaylist, SpotifyMenu};
	use crate::playlist_window;
	use crate::testing::{
		BOTH_GETTYS, EPISODE, GETTY_TRACK, LOCAL, NO_FIELDS, ONE_ARTIST, entry, episode,
		getty_track, installed_api, installed_api_and_backend, library_holding,
	};
	use crate::window::brought_up;

	fn command<T: MainMenuCommand>() -> MainMenuHandle {
		MainMenuHandle::new(mainmenu_command_service::<T>())
	}

	/// Runs `T` as the menu or a key bound to it does.
	fn press<T: MainMenuCommand>() {
		command::<T>().execute(0, None).unwrap();
	}

	/// The entries Playing track shows, in menu order, with their flags.
	fn shown() -> Vec<(String, u32)> {
		[
			command::<PlayingAlbum>().display(0),
			command::<PlayingArtist>().display(0),
			command::<PlayingRadio>().display(0),
			command::<PlayingShow>().display(0),
		]
		.into_iter()
		.flatten()
		.collect()
	}

	/// The popup a command shows when the playing track isn't a Spotify `what`.
	fn play_one(command: &str, what: &str) -> (String, String, PopupIcon) {
		(command.to_owned(), format!("Play a Spotify {what} first."), PopupIcon::Information)
	}

	fn every_command_greyed() -> [(String, u32); 4] {
		[
			entry("Open album", FLAG_DISABLED),
			entry("Open artist…", FLAG_DISABLED),
			entry("Track radio", FLAG_DISABLED),
			entry("Open show", FLAG_DISABLED),
		]
	}

	#[test]
	fn the_playing_track_submenu_sits_after_open_playlist() {
		let popup = MainMenuHandle::new(mainmenu_popup_service::<PlayingTrack>());
		assert_eq!(popup.guid(), Guid::from_u128(0x1120bc99_c585_4c5e_a299_9ede596b39e2));
		assert_eq!(popup.parent(), SpotifyMenu::GUID);
		assert_eq!(popup.name(), "Playing track");
		assert_eq!(popup.sort_priority(), SORT_PRIORITY_BASE + 1);
		assert!(popup.sort_priority() > OpenPlaylist::SORT_PRIORITY);
		assert_eq!(PlayingTrack::GUID, popup.guid());
	}

	#[test]
	fn each_command_has_its_name_place_and_pinned_guid() {
		let entries = [
			(
				command::<PlayingAlbum>(),
				0x11865a76_0fc0_43ac_8bc9_d348e57ecc9e,
				"Open album",
				"Opens the playing Spotify track's album in a playlist tab.",
				SORT_PRIORITY_BASE,
			),
			(
				command::<PlayingArtist>(),
				0x00fbed21_419c_4b8a_905f_b0047d78b2ac,
				"Open artist…",
				"Opens the artist of the playing Spotify track in a playlist tab, or lists its artists to pick from.",
				SORT_PRIORITY_BASE + 1,
			),
			(
				command::<PlayingRadio>(),
				0x41fb8968_b48e_4eca_b351_e44de55db978,
				"Track radio",
				"Opens Spotify's radio for the playing track in a playlist tab.",
				SORT_PRIORITY_BASE + 2,
			),
			(
				command::<PlayingShow>(),
				0x69006f81_9584_40b4_b221_dbac816f45da,
				"Open show",
				"Opens the playing Spotify episode's show in a playlist tab.",
				SORT_PRIORITY_BASE + 3,
			),
			(
				command::<PlayingAddToPlaylist>(),
				0x5586b6ac_c3a2_4a6c_9b98_773c8aca259b,
				"Add to playlist…",
				"Adds the playing Spotify track to Liked Songs or your Spotify playlists, or removes it, in a window that saves on OK.",
				SORT_PRIORITY_BASE + 4,
			),
		];
		for (entry, guid, name, description, sort_priority) in entries {
			assert_eq!(entry.command_count(), 1, "{name}");
			assert_eq!(entry.command(0), Guid::from_u128(guid), "{name}");
			assert_eq!(entry.command_name(0), name);
			assert_eq!(entry.description(0).as_deref(), Some(description), "{name}");
			assert_eq!(entry.parent(), PlayingTrack::GUID, "{name}");
			assert_eq!(entry.sort_priority(), sort_priority, "{name}");
		}
	}

	#[test]
	fn open_artist_is_a_plain_command_even_for_several_stored_artists() {
		let api = installed_api();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		let open_artist = command::<PlayingArtist>();
		assert!(!open_artist.answers(&COMMANDS_V2_IID), "not built as the menu opens");
		assert_eq!(open_artist.command_count(), 1);
		assert_eq!(open_artist.display(0), Some(entry("Open artist…", 0)));
	}

	#[test]
	fn nothing_playing_greys_every_command() {
		installed_api();
		assert_eq!(shown(), every_command_greyed());
	}

	#[test]
	fn a_local_file_playing_greys_every_command() {
		let api = installed_api();
		api.set_now_playing(Some((LOCAL, BOTH_GETTYS)));
		assert_eq!(shown(), every_command_greyed());
	}

	#[test]
	fn a_playing_track_enables_album_artist_and_radio_and_hides_show() {
		let api = installed_api();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		assert_eq!(
			shown(),
			[entry("Open album", 0), entry("Open artist…", 0), entry("Track radio", 0)]
		);
	}

	#[test]
	fn a_playing_episode_enables_show_and_hides_album_artist_and_radio() {
		let api = installed_api();
		api.set_now_playing(Some((EPISODE, &[("ARTIST", &["Sing! Podcast"])])));
		assert_eq!(shown(), [entry("Open show", 0)]);
	}

	#[test]
	fn open_artist_for_fewer_than_two_stored_artists_asks_spotify_without_a_window() {
		let (api, fake) = installed_api_and_backend();
		for fields in [ONE_ARTIST, NO_FIELDS] {
			api.set_now_playing(Some((GETTY_TRACK, fields)));
			press::<PlayingArtist>();
		}
		assert_eq!(fake.related_calls(), vec![(getty_track(), Related::Artist); 2]);
		assert!(WINDOW.current().is_none(), "no window says Loading…");
	}

	#[test]
	fn each_command_looks_up_what_it_opens() {
		let (api, fake) = installed_api_and_backend();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		press::<PlayingAlbum>();
		press::<PlayingRadio>();
		api.set_now_playing(Some((EPISODE, NO_FIELDS)));
		press::<PlayingShow>();
		assert_eq!(
			fake.related_calls(),
			[
				(getty_track(), Related::Album),
				(getty_track(), Related::Radio),
				(episode(), Related::Show),
			]
		);
	}

	#[test]
	fn open_artist_shows_loading_for_two_stored_artists() {
		let (api, fake) = installed_api_and_backend();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		press::<PlayingArtist>();
		let dialog = WINDOW.current().expect("the artist window is open");
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(brought_up(), 1);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist)]);
		destroy(dialog);
	}

	#[test]
	fn a_shortcut_with_nothing_playing_says_play_a_spotify_track_first() {
		let (api, fake) = installed_api_and_backend();
		press::<PlayingAlbum>();
		press::<PlayingArtist>();
		press::<PlayingRadio>();
		press::<PlayingShow>();
		assert_eq!(
			api.popups(),
			[
				play_one("Open album", "track"),
				play_one("Open artist…", "track"),
				play_one("Track radio", "track"),
				play_one("Open show", "episode"),
			]
		);
		assert!(fake.related_calls().is_empty());
		assert!(WINDOW.current().is_none());
	}

	#[test]
	fn a_shortcut_for_the_other_kind_says_what_to_play() {
		let (api, fake) = installed_api_and_backend();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		press::<PlayingShow>();
		api.set_now_playing(Some((EPISODE, NO_FIELDS)));
		press::<PlayingAlbum>();
		press::<PlayingArtist>();
		press::<PlayingRadio>();
		assert_eq!(
			api.popups(),
			[
				play_one("Open show", "episode"),
				play_one("Open album", "track"),
				play_one("Open artist…", "track"),
				play_one("Track radio", "track"),
			]
		);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn a_shortcut_while_a_local_file_plays_says_play_a_spotify_track_first() {
		let (api, fake) = installed_api_and_backend();
		api.set_now_playing(Some((LOCAL, BOTH_GETTYS)));
		press::<PlayingAlbum>();
		press::<PlayingShow>();
		assert_eq!(
			api.popups(),
			[play_one("Open album", "track"), play_one("Open show", "episode")]
		);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn add_to_playlist_for_the_playing_item_opens_the_window() {
		let (api, fake) = installed_api_and_backend();
		fake.set_library(library_holding(&[getty_track()], &[], &[]));
		let add = command::<PlayingAddToPlaylist>();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		assert_eq!(add.display(0), Some(entry("Add to playlist…", 0)));
		press::<PlayingAddToPlaylist>();
		let dialog = playlist_window::WINDOW.current().expect("the Add to playlist window is open");
		assert_eq!(brought_up(), 1);
		let list = playlist_window::LIST;
		assert_eq!(dialog.check_list_texts(list), ["Liked Songs", "Church / Sunday set", "Shared"]);
		assert!(dialog.is_row_checked(list, 1), "Sunday set holds the playing track");
		api.set_now_playing(Some((EPISODE, NO_FIELDS)));
		assert_eq!(add.display(0), Some(entry("Add to playlist…", 0)));
		press::<PlayingAddToPlaylist>();
		pump_messages();
		assert_eq!(brought_up(), 2);
		assert_eq!(
			dialog.check_list_texts(list),
			["Church / Sunday set", "Shared"],
			"Liked Songs holds songs only"
		);
		destroy(dialog);
	}

	#[test]
	fn with_nothing_playing_it_is_greyed_and_a_key_says_what_to_play() {
		let (api, _) = installed_api_and_backend();
		let add = command::<PlayingAddToPlaylist>();
		let greyed = Some(entry("Add to playlist…", FLAG_DISABLED));
		assert_eq!(add.display(0), greyed);
		press::<PlayingAddToPlaylist>();
		api.set_now_playing(Some((LOCAL, BOTH_GETTYS)));
		assert_eq!(add.display(0), greyed);
		press::<PlayingAddToPlaylist>();
		let play = play_one("Add to playlist…", "track or episode");
		assert_eq!(api.popups(), [play.clone(), play]);
		assert!(playlist_window::WINDOW.current().is_none());
		assert_eq!(brought_up(), 0);
	}
}
