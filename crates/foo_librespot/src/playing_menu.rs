//! File > Spotify > Playing track: opens the album, an artist, the radio or the show of the
//! Spotify track or episode playing now.
//!
//! The commands follow the right-click rules ([`crate::spotify_menu`]) for the playing track
//! alone, except that they are greyed out rather than hidden when nothing plays or the playing
//! track isn't from Spotify. Open artist builds its entry, a submenu or a command, each time the
//! menu opens. A bound key runs a command whatever the menu would show, so each command reads
//! the playing track again when it runs; when that isn't a Spotify item of its kind, a popup
//! says to play one, and nothing is looked up.

use std::sync::Arc;

use foo_librespot_core::link::ItemId;
use foo_librespot_core::related::Related;
use foxide2k::abi::mainmenu::SORT_PRIORITY_BASE;
use foxide2k::contextmenu::Shown;
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::mainmenu::{MainMenuCommand, MainMenuDynamicCommand, MainMenuNode, MainMenuPopup};
use foxide2k::metadb::Track;
use foxide2k::playback;

use crate::open_playlist::SpotifyMenu;
use crate::spotify_menu::{
	ArtistPicks, Kind, Source, SpotifyItem, artist_picks, open, open_artists, shown_among,
	spotify_item,
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
		shown(&playing(), Kind::Track)
	}

	fn execute() -> Result<()> {
		open(playing(), Source::Playing, Self::NAME, Kind::Track, Related::Album);
		Ok(())
	}
}

/// The Open artist command that asks Spotify for every artist.
const EVERY_ARTIST: Guid = Guid::from_u128(0x5dcdad74_570f_4798_b8c1_55d891328120);

/// The GUID of the stored artist at `index` in the Open artist submenu: a fixed GUID with
/// `index` in its last byte.
const fn artist_guid(index: u8) -> Guid {
	let mut guid = Guid::from_u128(0x6cebbc10_071b_4103_8289_c104cdff9e51);
	guid.data4[7] = index;
	guid
}

/// Opens an artist of the playing track: a submenu of its stored artists when it has several,
/// or else one command that asks Spotify for them. A key bound to it does what the
/// shortcut-only right-click "Open artist…" does.
pub struct PlayingArtist;

impl MainMenuCommand for PlayingArtist {
	const GUID: Guid = Guid::from_u128(0x00fbed21_419c_4b8a_905f_b0047d78b2ac);
	const PARENT: Guid = PlayingTrack::GUID;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE + 1;
	const NAME: &'static str = "Open artist";
	const DESCRIPTION: &'static str = "Opens an artist of the playing Spotify track in a playlist tab, or lists its artists to pick from.";

	fn shown() -> Shown {
		shown(&playing(), Kind::Track)
	}

	fn execute() -> Result<()> {
		open_artists(playing(), Source::Playing, Self::NAME);
		Ok(())
	}
}

impl MainMenuDynamicCommand for PlayingArtist {
	fn build() -> MainMenuNode {
		let items = playing();
		match shown(&items, Kind::Track) {
			Shown::Enabled => {
				let (item, track) = &items[0];
				artist_node(*item, track)
			}
			Shown::Hidden | Shown::Greyed => {
				command(Self::NAME.into(), EVERY_ARTIST, false, || {
					open_artists(playing(), Source::Playing, Self::NAME);
				})
			}
		}
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
		shown(&playing(), Kind::Track)
	}

	fn execute() -> Result<()> {
		open(playing(), Source::Playing, Self::NAME, Kind::Track, Related::Radio);
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
		shown(&playing(), Kind::Episode)
	}

	fn execute() -> Result<()> {
		open(playing(), Source::Playing, Self::NAME, Kind::Episode, Related::Show);
		Ok(())
	}
}

/// The playing track, alone, when it is a Spotify track or episode; empty when nothing plays,
/// the playing track isn't from Spotify, or foobar2000 can't say what plays. Main thread only.
fn playing() -> Vec<SpotifyItem> {
	playback::now_playing().ok().flatten().and_then(spotify_item).into_iter().collect()
}

/// How a command that opens from `kind` shows for the `playing` items: greyed out when there is
/// none, or else as the right-click rules say.
fn shown(playing: &[SpotifyItem], kind: Kind) -> Shown {
	if playing.is_empty() { Shown::Greyed } else { shown_among(playing, kind) }
}

/// The submenu of `track`'s stored artists when it has several, each opening that artist; or
/// else the command that asks Spotify for every artist. The submenu lists the first 256, one
/// for each value of the GUID's last byte.
fn artist_node(item: ItemId, track: &Track) -> MainMenuNode {
	match artist_picks(item, track) {
		ArtistPicks::Every(pick) => command(PlayingArtist::NAME.into(), EVERY_ARTIST, true, pick),
		ArtistPicks::Each(picks) => MainMenuNode::Group {
			name: PlayingArtist::NAME.into(),
			children: picks
				.into_iter()
				.zip(0..=u8::MAX)
				.map(|((name, pick), index)| command(name, artist_guid(index), true, pick))
				.collect(),
		},
	}
}

/// A command named `name` that runs `run`.
fn command(
	name: Arc<str>,
	guid: Guid,
	enabled: bool,
	run: impl Fn() + Send + Sync + 'static,
) -> MainMenuNode {
	MainMenuNode::Command {
		name,
		guid,
		enabled,
		run: Box::new(move || {
			run();
			Ok(())
		}),
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::link::ItemId;
	use foo_librespot_core::related::Related;
	use foxide2k::abi::mainmenu::{FLAG_DISABLED, SORT_PRIORITY_BASE, TYPE_COMMAND, TYPE_GROUP};
	use foxide2k::dialog::host::destroy;
	use foxide2k::guid::Guid;
	use foxide2k::mainmenu::{
		MainMenuCommand, MainMenuPopup, mainmenu_command_service, mainmenu_dynamic_service,
		mainmenu_popup_service,
	};
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{FakeApi, FakeFields, MainMenuHandle, MainMenuNodeHandle};

	use super::{PlayingAlbum, PlayingArtist, PlayingRadio, PlayingShow, PlayingTrack};
	use crate::artist_window::{STATUS, WINDOW};
	use crate::open_playlist::{OpenPlaylist, SpotifyMenu};
	use crate::testing::{
		FakeBackend, episode_id, installed_backend, installed_screen, track_id, wait_until,
	};

	/// "In Christ Alone".
	const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";
	const EPISODE: &str = "sptf://spotify:episode:4QomYTPgZ1mpVbUmxBsUpz";
	const LOCAL: &str = "file://C:\\a.mp3";

	/// What foobar2000 stores for "In Christ Alone".
	const BOTH_GETTYS: FakeFields<'static> =
		&[("ARTIST", &["Keith Getty", "Kristyn Getty"]), ("TITLE", &["In Christ Alone"])];
	const ONE_ARTIST: FakeFields<'static> =
		&[("ARTIST", &["Keith & Kristyn Getty"]), ("TITLE", &["In Christ Alone"])];
	const NO_FIELDS: FakeFields<'static> = &[];

	/// The GUID of the Open artist command that asks Spotify for every artist.
	const EVERY_ARTIST: Guid = Guid::from_u128(0x5dcdad74_570f_4798_b8c1_55d891328120);
	/// The GUIDs of the first and second stored artists in the Open artist submenu.
	const FIRST_ARTIST: Guid = Guid::from_u128(0x6cebbc10_071b_4103_8289_c104cdff9e00);
	const SECOND_ARTIST: Guid = Guid::from_u128(0x6cebbc10_071b_4103_8289_c104cdff9e01);

	fn getty_track() -> ItemId {
		ItemId::Track(track_id())
	}

	fn command<T: MainMenuCommand>() -> MainMenuHandle {
		MainMenuHandle::new(mainmenu_command_service::<T>())
	}

	fn open_artist() -> MainMenuHandle {
		MainMenuHandle::new(mainmenu_dynamic_service::<PlayingArtist>())
	}

	/// Runs `T` as a key bound to it does.
	fn press<T: MainMenuCommand>() {
		command::<T>().execute(0, None).unwrap();
	}

	/// Runs Open artist as a key bound to it does.
	fn press_open_artist() {
		open_artist().execute(0, None).unwrap();
	}

	/// The entries Playing track shows, in menu order, with their flags.
	fn shown() -> Vec<(String, u32)> {
		[
			command::<PlayingAlbum>().display(0),
			artist_shown(),
			command::<PlayingRadio>().display(0),
			command::<PlayingShow>().display(0),
		]
		.into_iter()
		.flatten()
		.collect()
	}

	/// How Open artist shows: its own display, which the entry it builds agrees with when it
	/// shows.
	fn artist_shown() -> Option<(String, u32)> {
		let shown = open_artist().display(0);
		if let Some(display) = &shown {
			assert_eq!(&open_artist().instantiate(0).display(), display, "the built entry");
		}
		shown
	}

	fn entry(name: &str, flags: u32) -> (String, u32) {
		(name.to_owned(), flags)
	}

	/// The popup a command shows when the playing track isn't a Spotify `what`.
	fn play_one(command: &str, what: &str) -> (String, String, PopupIcon) {
		(command.to_owned(), format!("Play a Spotify {what} first."), PopupIcon::Information)
	}

	fn installed_api() -> &'static FakeApi {
		let api = FakeApi::new();
		api.install();
		api
	}

	fn setup() -> (&'static FakeApi, FakeBackend) {
		(installed_api(), installed_backend())
	}

	fn every_command_greyed() -> [(String, u32); 4] {
		[
			entry("Open album", FLAG_DISABLED),
			entry("Open artist", FLAG_DISABLED),
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
				open_artist(),
				0x00fbed21_419c_4b8a_905f_b0047d78b2ac,
				"Open artist",
				"Opens an artist of the playing Spotify track in a playlist tab, or lists its artists to pick from.",
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
		];
		for (entry, guid, name, description, sort_priority) in entries {
			assert_eq!(entry.command_count(), 1, "{name}");
			assert_eq!(entry.command(0), Guid::from_u128(guid), "{name}");
			assert_eq!(entry.command_name(0), name);
			assert_eq!(entry.description(0).as_deref(), Some(description), "{name}");
			assert_eq!(entry.parent(), PlayingTrack::GUID, "{name}");
			assert_eq!(entry.sort_priority(), sort_priority, "{name}");
		}
		assert!(open_artist().is_dynamic(0));
	}

	#[test]
	fn nothing_playing_greys_every_command() {
		installed_api();
		assert_eq!(shown(), every_command_greyed());
		let root = open_artist().instantiate(0);
		assert_eq!(root.kind(), TYPE_COMMAND);
		assert_eq!(root.guid(), EVERY_ARTIST);
	}

	#[test]
	fn a_local_file_playing_greys_every_command() {
		let api = installed_api();
		api.set_now_playing(Some((LOCAL, BOTH_GETTYS)));
		assert_eq!(shown(), every_command_greyed());
		assert_eq!(open_artist().instantiate(0).kind(), TYPE_COMMAND);
	}

	#[test]
	fn a_playing_track_enables_album_artist_and_radio_and_hides_show() {
		let api = installed_api();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		assert_eq!(
			shown(),
			[entry("Open album", 0), entry("Open artist", 0), entry("Track radio", 0)]
		);
	}

	#[test]
	fn a_playing_episode_enables_show_and_hides_album_artist_and_radio() {
		let api = installed_api();
		api.set_now_playing(Some((EPISODE, &[("ARTIST", &["Sing! Podcast"])])));
		assert_eq!(shown(), [entry("Open show", 0)]);
		let root = open_artist().instantiate(0);
		assert_eq!(root.kind(), TYPE_COMMAND);
		assert_eq!(root.display(), entry("Open artist", FLAG_DISABLED));
		assert_eq!(root.guid(), EVERY_ARTIST);
	}

	#[test]
	fn a_playing_track_with_two_artists_gets_an_artist_submenu() {
		let api = installed_api();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		let root = open_artist().instantiate(0);
		assert_eq!(root.kind(), TYPE_GROUP);
		assert_eq!(root.display(), entry("Open artist", 0));
		let children = root.children();
		let names: Vec<_> = children.iter().map(MainMenuNodeHandle::display).collect();
		assert_eq!(names, [entry("Keith Getty", 0), entry("Kristyn Getty", 0)]);
		let guids: Vec<_> = children.iter().map(MainMenuNodeHandle::guid).collect();
		assert_eq!(guids, [FIRST_ARTIST, SECOND_ARTIST]);
		assert!(children.iter().all(|child| child.kind() == TYPE_COMMAND));
	}

	#[test]
	fn picking_one_looks_it_up() {
		let (api, fake) = setup();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		assert!(open_artist().dynamic_execute(0, &SECOND_ARTIST).unwrap());
		assert_eq!(
			fake.related_calls(),
			[(getty_track(), Related::Artist(Some("Kristyn Getty".to_owned())))]
		);
		wait_until(|| api.run_main_thread() > 0);
		assert_eq!(api.location_requests(), [["sptf://spotify:artist:2kxP07DLgs4xlWz8YHlvfh"]]);
		open_artist().instantiate(0).child(0).unwrap().execute(None).unwrap();
		assert_eq!(
			fake.related_calls().last(),
			Some(&(getty_track(), Related::Artist(Some("Keith Getty".to_owned()))))
		);
		assert!(WINDOW.current().is_none());
	}

	#[test]
	fn a_playing_track_with_one_stored_artist_gets_a_command_that_asks_spotify() {
		let (api, fake) = setup();
		installed_screen();
		for fields in [ONE_ARTIST, NO_FIELDS] {
			api.set_now_playing(Some((GETTY_TRACK, fields)));
			let root = open_artist().instantiate(0);
			assert_eq!(root.kind(), TYPE_COMMAND);
			assert_eq!(root.display(), entry("Open artist", 0));
			assert_eq!(root.guid(), EVERY_ARTIST);
			root.execute(None).unwrap();
		}
		assert!(open_artist().dynamic_execute(0, &EVERY_ARTIST).unwrap());
		assert_eq!(fake.related_calls(), vec![(getty_track(), Related::Artist(None)); 3]);
		assert!(WINDOW.current().is_none(), "no window says Loading…");
	}

	#[test]
	fn each_command_looks_up_what_it_opens() {
		let (api, fake) = setup();
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
				(ItemId::Episode(episode_id()), Related::Show),
			]
		);
	}

	#[test]
	fn open_artist_by_its_shortcut_shows_loading_for_two_stored_artists() {
		let (api, fake) = setup();
		let screen = installed_screen();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		press_open_artist();
		let dialog = WINDOW.current().expect("the artist window is open");
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(screen.brought_up(), 1);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist(None))]);
		destroy(dialog);
	}

	#[test]
	fn open_artist_by_its_shortcut_for_one_stored_artist_asks_spotify_without_a_window() {
		let (api, fake) = setup();
		installed_screen();
		api.set_now_playing(Some((GETTY_TRACK, ONE_ARTIST)));
		press_open_artist();
		assert!(WINDOW.current().is_none());
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist(None))]);
	}

	#[test]
	fn a_shortcut_with_nothing_playing_says_play_a_spotify_track_first() {
		let (api, fake) = setup();
		press::<PlayingAlbum>();
		press_open_artist();
		press::<PlayingRadio>();
		press::<PlayingShow>();
		assert!(open_artist().dynamic_execute(0, &EVERY_ARTIST).unwrap());
		assert_eq!(
			api.popups(),
			[
				play_one("Open album", "track"),
				play_one("Open artist", "track"),
				play_one("Track radio", "track"),
				play_one("Open show", "episode"),
				play_one("Open artist", "track"),
			]
		);
		assert!(fake.related_calls().is_empty());
		assert!(WINDOW.current().is_none());
	}

	#[test]
	fn a_shortcut_for_the_other_kind_says_what_to_play() {
		let (api, fake) = setup();
		api.set_now_playing(Some((GETTY_TRACK, BOTH_GETTYS)));
		press::<PlayingShow>();
		api.set_now_playing(Some((EPISODE, NO_FIELDS)));
		press::<PlayingAlbum>();
		press_open_artist();
		press::<PlayingRadio>();
		open_artist().instantiate(0).execute(None).unwrap();
		assert_eq!(
			api.popups(),
			[
				play_one("Open show", "episode"),
				play_one("Open album", "track"),
				play_one("Open artist", "track"),
				play_one("Track radio", "track"),
				play_one("Open artist", "track"),
			]
		);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn a_shortcut_while_a_local_file_plays_says_play_a_spotify_track_first() {
		let (api, fake) = setup();
		api.set_now_playing(Some((LOCAL, BOTH_GETTYS)));
		press::<PlayingAlbum>();
		press::<PlayingShow>();
		assert_eq!(
			api.popups(),
			[play_one("Open album", "track"), play_one("Open show", "episode")]
		);
		assert!(fake.related_calls().is_empty());
	}
}
