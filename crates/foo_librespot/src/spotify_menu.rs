//! Right-click > Spotify: opens the album, an artist, the radio or the show of the one Spotify
//! track or episode selected, and holds "Add to playlist…" ([`crate::playlist_menu`]).
//!
//! An open command shows only for the kind of item it opens from, a track or an episode, and is
//! greyed out when several Spotify items are selected. Local files in the selection are left
//! out. A bound key runs a command whatever the menu would show, so each command checks the
//! selection again when it runs; when that isn't one Spotify item of its kind, a popup says to
//! select one, and nothing is looked up. The menu reads only what foobar2000 stores and never
//! waits for Spotify; how a command shows is read from the selection's paths alone.
//!
//! These rules take the Spotify items and where they came from, so File > Spotify > Playing
//! track ([`crate::playing_menu`]) follows them for the playing track.

use std::ops::ControlFlow;

use foo_librespot_core::link::{ItemId, parse_item};
use foo_librespot_core::related::Related;
use foxide2k::abi::contextmenu::GROUP_ROOT;
use foxide2k::contextmenu::{ContextMenuCommand, ContextMenuGroup, Shown};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::metadb::{HandleList, Track};
use foxide2k::popup::PopupIcon;

use crate::artist_window;
use crate::errors::show_message;
use crate::related::open_related;

/// The Spotify submenu at the bottom of the right-click menu.
pub struct SpotifyGroup;

impl ContextMenuGroup for SpotifyGroup {
	const GUID: Guid = Guid::from_u128(0x4dbb5875_3462_42ed_a25d_9b23ebf7f056);
	const PARENT: Guid = GROUP_ROOT;
	const SORT_PRIORITY: f64 = 1000.0;
	const NAME: &'static str = "Spotify";
}

/// Opens the selected track's album.
pub struct OpenAlbum;

impl ContextMenuCommand for OpenAlbum {
	const GUID: Guid = Guid::from_u128(0xb19f992a_df97_49a8_8c92_5aa3e5af89f4);
	const NAME: &'static str = "Open album";
	const DESCRIPTION: &'static str = "Opens the selected Spotify track's album in a playlist tab.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 0.0;

	fn shown(selection: &HandleList<'_>) -> Shown {
		shown_for(selection, Kind::Track)
	}

	fn execute(selection: &HandleList<'_>) -> Result<()> {
		open(spotify_items(selection), Source::Selection, Self::NAME, Related::Album);
		Ok(())
	}
}

/// Opens the selected track's artist when Spotify names one, or lists its artists in the artist
/// window.
pub struct OpenArtist;

impl ContextMenuCommand for OpenArtist {
	const GUID: Guid = Guid::from_u128(0xc240e9cc_4c76_4ba1_9c64_85246758c4e5);
	const NAME: &'static str = "Open artist…";
	const DESCRIPTION: &'static str = "Opens the artist of the selected Spotify track in a playlist tab, or lists its artists to pick from.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 1.0;

	fn shown(selection: &HandleList<'_>) -> Shown {
		shown_for(selection, Kind::Track)
	}

	fn execute(selection: &HandleList<'_>) -> Result<()> {
		open_artists(spotify_items(selection), Source::Selection, Self::NAME);
		Ok(())
	}
}

/// Opens Spotify's radio for the selected track.
pub struct TrackRadio;

impl ContextMenuCommand for TrackRadio {
	const GUID: Guid = Guid::from_u128(0xd4b2e4d1_156b_479c_976b_9515de39d6e1);
	const NAME: &'static str = "Track radio";
	const DESCRIPTION: &'static str =
		"Opens Spotify's radio for the selected track in a playlist tab.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 2.0;

	fn shown(selection: &HandleList<'_>) -> Shown {
		shown_for(selection, Kind::Track)
	}

	fn execute(selection: &HandleList<'_>) -> Result<()> {
		open(spotify_items(selection), Source::Selection, Self::NAME, Related::Radio);
		Ok(())
	}
}

/// Opens the selected episode's show.
pub struct OpenShow;

impl ContextMenuCommand for OpenShow {
	const GUID: Guid = Guid::from_u128(0xfb318e3c_ecec_4269_a35d_5ffb0f4823ce);
	const NAME: &'static str = "Open show";
	const DESCRIPTION: &'static str =
		"Opens the selected Spotify episode's show in a playlist tab.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 3.0;

	fn shown(selection: &HandleList<'_>) -> Shown {
		shown_for(selection, Kind::Episode)
	}

	fn execute(selection: &HandleList<'_>) -> Result<()> {
		open(spotify_items(selection), Source::Selection, Self::NAME, Related::Show);
		Ok(())
	}
}

/// What an open command opens from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
	Track,
	Episode,
}

impl Kind {
	const fn of(item: ItemId) -> Self {
		match item {
			ItemId::Track(_) => Self::Track,
			ItemId::Episode(_) => Self::Episode,
		}
	}

	/// What opening `what` starts from: an episode for its show, a track for anything else.
	const fn opening(what: &Related) -> Self {
		match what {
			Related::Show => Self::Episode,
			Related::Album | Related::Artist | Related::Radio => Self::Track,
		}
	}
}

/// Where a command finds the Spotify items it works on.
#[derive(Clone, Copy)]
pub enum Source {
	/// The tracks a right-click menu, or a key bound to one of its commands, passes.
	Selection,
	/// The playing track.
	Playing,
}

impl Source {
	/// What a command says when this holds no one Spotify item of `kind`.
	const fn wants(self, kind: Kind) -> &'static str {
		match (self, kind) {
			(Self::Selection, Kind::Track) => "Select one Spotify track first.",
			(Self::Selection, Kind::Episode) => "Select one Spotify episode first.",
			(Self::Playing, Kind::Track) => "Play a Spotify track first.",
			(Self::Playing, Kind::Episode) => "Play a Spotify episode first.",
		}
	}

	/// What a command that takes tracks and episodes alike says when this holds no Spotify item.
	pub const fn wants_any(self) -> &'static str {
		match self {
			Self::Selection => "Select a Spotify track or episode first.",
			Self::Playing => "Play a Spotify track or episode first.",
		}
	}
}

/// A Spotify track or episode, and the foobar2000 track that holds what is stored for it.
pub type SpotifyItem = (ItemId, Track);

/// `track` with its Spotify item, when it is a Spotify track or episode whose path can be read.
pub fn spotify_item(track: Track) -> Option<SpotifyItem> {
	Some((parse_item(&track.path().ok()?)?, track))
}

/// The Spotify tracks and episodes in `selection`, in order; an item's path is read only when the
/// item is asked for. Local files, and items whose path can't be read, are left out.
pub fn spotify_items(selection: &HandleList<'_>) -> impl Iterator<Item = SpotifyItem> {
	selection.tracks().unwrap_or_default().into_iter().filter_map(spotify_item)
}

/// How a command that opens from `kind` shows for `selection`, read from the paths alone, as
/// [`shown_among`] says. Hidden when the selection can't be read.
fn shown_for(selection: &HandleList<'_>, kind: Kind) -> Shown {
	let mut tally = Tally::new(kind);
	let read = selection.try_for_each_path(|path| {
		parse_item(path).map_or(ControlFlow::Continue(()), |item| tally.add(item))
	});
	if read.is_ok() { tally.shown() } else { Shown::Hidden }
}

/// Hidden when none of the Spotify `items` is of `kind`, enabled when the only one is, and
/// greyed out among several. Reads `items` only until that is settled.
pub fn shown_among(items: impl IntoIterator<Item = ItemId>, kind: Kind) -> Shown {
	let mut tally = Tally::new(kind);
	let _ = items.into_iter().try_for_each(|item| tally.add(item));
	tally.shown()
}

/// How many Spotify items a command has seen, and whether one is of the kind it opens from.
struct Tally {
	kind: Kind,
	items: usize,
	of_kind: bool,
}

impl Tally {
	const fn new(kind: Kind) -> Self {
		Self { kind, items: 0, of_kind: false }
	}

	/// Counts `item`; breaks once no later item can change how the command shows.
	fn add(&mut self, item: ItemId) -> ControlFlow<()> {
		self.items += 1;
		self.of_kind |= Kind::of(item) == self.kind;
		if self.items > 1 && self.of_kind {
			ControlFlow::Break(())
		} else {
			ControlFlow::Continue(())
		}
	}

	const fn shown(&self) -> Shown {
		match (self.of_kind, self.items) {
			(false, _) => Shown::Hidden,
			(true, 1) => Shown::Enabled,
			(true, _) => Shown::Greyed,
		}
	}
}

/// The one Spotify item in `items`, when it is of `kind`, read up to the second item. Otherwise
/// a popup titled `command` says what `source` should hold.
fn the_one(
	items: impl IntoIterator<Item = SpotifyItem>,
	kind: Kind,
	source: Source,
	command: &str,
) -> Option<SpotifyItem> {
	let mut first_two: Vec<SpotifyItem> = items.into_iter().take(2).collect();
	if shown_among(first_two.iter().map(|(item, _)| *item), kind) == Shown::Enabled {
		return first_two.pop();
	}
	show_message(command, source.wants(kind), PopupIcon::Information);
	None
}

/// Opens `what` for the one Spotify item in `items`, taken from `source`, when it is of the kind
/// `what` opens from.
pub fn open(
	items: impl IntoIterator<Item = SpotifyItem>,
	source: Source,
	command: &str,
	what: Related,
) {
	if let Some((item, track)) = the_one(items, Kind::opening(&what), source, command) {
		open_related(item, what, title_of(item, &track), None);
	}
}

/// Asks Spotify for every artist of the one Spotify track in `items`, taken from `source`, as
/// [`OpenArtist`] does: one opens in its tab, several fill the artist window. When foobar2000
/// stores two or more for the track, the window says "Loading…" meanwhile.
pub fn open_artists(items: impl IntoIterator<Item = SpotifyItem>, source: Source, command: &str) {
	if let Some((item, track)) = the_one(items, Kind::Track, source, command) {
		let ticket = if stored_artists(&track).len() >= 2 {
			artist_window::show_loading()
		} else {
			artist_window::take_ticket()
		};
		open_related(item, Related::Artist, title_of(item, &track), Some(ticket));
	}
}

/// What a failure calls the track: its stored title, or its Spotify URI when it has none.
fn title_of(item: ItemId, track: &Track) -> String {
	track
		.meta("TITLE")
		.ok()
		.and_then(|titles| titles.into_iter().find(|title| !title.is_empty()))
		.unwrap_or_else(|| item.uri().to_uri())
}

/// The track's stored artists, in Spotify's order.
fn stored_artists(track: &Track) -> Vec<String> {
	let mut artists = track.meta("ARTIST").unwrap_or_default();
	artists.retain(|artist| !artist.is_empty());
	artists
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::link::{ItemId, LinkKind};
	use foo_librespot_core::related::{Found, Opened, Related};
	use foo_librespot_core::{CoreError, SpotifyId};
	use foxide2k::abi::contextmenu::{
		ENABLED_DEFAULT_ON, FLAG_DISABLED, FLAG_GRAYED, GROUP_ROOT, TYPE_COMMAND,
	};
	use foxide2k::contextmenu::{
		ContextMenuCommand, ContextMenuGroup, Shown, context_menu_command_service,
		context_menu_group_service,
	};
	use foxide2k::dialog::host::destroy;
	use foxide2k::guid::Guid;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{
		ContextMenuGroupHandle, ContextMenuHandle, FakeFields, FakeHandleList, pump_messages,
	};

	use super::{Kind, OpenAlbum, OpenArtist, OpenShow, SpotifyGroup, TrackRadio, shown_among};
	use crate::artist_window::{self, LIST, STATUS, WINDOW};
	use crate::playlist_menu::AddToPlaylist;
	use crate::testing::{
		BOTH_GETTYS, EPISODE, GETTY_TRACK, LOCAL, NO_FIELDS, ONE_ARTIST, TRACK_ID, entry,
		episode_id, getty_artists, getty_track, installed_api_and_backend, wait_until,
	};
	use crate::window::brought_up;

	const GREYED: u32 = FLAG_DISABLED | FLAG_GRAYED;

	/// Another Keith & Kristyn Getty track.
	const OTHER_TRACK: &str = "sptf://spotify:track:0iYaBOYiWTPt2A7JMNK6eO";

	fn select(tracks: &[(&str, FakeFields<'_>)]) -> FakeHandleList {
		*FakeHandleList::with_meta(tracks)
	}

	fn command<T: ContextMenuCommand>() -> ContextMenuHandle {
		ContextMenuHandle::new(context_menu_command_service::<T>())
	}

	/// Runs `T` as a key bound to it does, on `data`.
	fn press<T: ContextMenuCommand>(data: &FakeHandleList) {
		command::<T>().execute_simple_on(&Guid::NULL, data).unwrap();
	}

	/// Runs `T` from the right-click menu opened on `data`.
	fn choose<T: ContextMenuCommand>(data: &FakeHandleList) {
		command::<T>().instantiate(data).node().execute(data).unwrap();
	}

	/// The open entries the Spotify submenu shows for `data`, in menu order, with their flags.
	fn shown(data: &FakeHandleList) -> Vec<(String, u32)> {
		[
			command::<OpenAlbum>().instantiate(data).node().display(data),
			command::<OpenArtist>().instantiate(data).node().display(data),
			command::<TrackRadio>().instantiate(data).node().display(data),
			command::<OpenShow>().instantiate(data).node().display(data),
		]
		.into_iter()
		.flatten()
		.collect()
	}

	/// The popup a command shows when the selection isn't one Spotify `what`.
	fn select_one(command: &str, what: &str) -> (String, String, PopupIcon) {
		(command.to_owned(), format!("Select one Spotify {what} first."), PopupIcon::Information)
	}

	#[test]
	fn the_spotify_submenu_sits_at_the_bottom_of_the_root_menu() {
		let group = ContextMenuGroupHandle::new(context_menu_group_service::<SpotifyGroup>());
		assert_eq!(group.guid(), Guid::from_u128(0x4dbb5875_3462_42ed_a25d_9b23ebf7f056));
		assert_eq!(group.parent(), GROUP_ROOT);
		assert_eq!(group.sort_priority(), 1000.0);
		assert_eq!(group.name(), "Spotify");
	}

	#[test]
	fn each_entry_has_its_name_place_and_pinned_guid() {
		let entries = [
			(
				command::<OpenAlbum>(),
				0xb19f992a_df97_49a8_8c92_5aa3e5af89f4,
				"Open album",
				"Opens the selected Spotify track's album in a playlist tab.",
				0.0,
			),
			(
				command::<OpenArtist>(),
				0xc240e9cc_4c76_4ba1_9c64_85246758c4e5,
				"Open artist…",
				"Opens the artist of the selected Spotify track in a playlist tab, or lists its artists to pick from.",
				1.0,
			),
			(
				command::<TrackRadio>(),
				0xd4b2e4d1_156b_479c_976b_9515de39d6e1,
				"Track radio",
				"Opens Spotify's radio for the selected track in a playlist tab.",
				2.0,
			),
			(
				command::<OpenShow>(),
				0xfb318e3c_ecec_4269_a35d_5ffb0f4823ce,
				"Open show",
				"Opens the selected Spotify episode's show in a playlist tab.",
				3.0,
			),
		];
		for (entry, guid, name, description, sort_priority) in entries {
			assert_eq!(entry.guid(), Guid::from_u128(guid), "{name}");
			assert_eq!(entry.name(), name);
			assert_eq!(entry.description().as_deref(), Some(description), "{name}");
			assert_eq!(entry.parent(), SpotifyGroup::GUID, "{name}");
			assert_eq!(entry.sort_priority(), sort_priority, "{name}");
		}
	}

	#[test]
	fn add_to_playlist_keeps_its_guid_and_moves_into_the_spotify_submenu() {
		let item = command::<AddToPlaylist>();
		assert_eq!(item.guid(), Guid::from_u128(0x9dde669c_a508_460e_b0df_c4e8e681592b));
		assert_eq!(item.parent(), SpotifyGroup::GUID);
		assert_eq!(item.name(), "Add to playlist…");
	}

	#[test]
	fn a_track_shows_album_artist_and_radio_and_no_show() {
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		assert_eq!(
			shown(&data),
			[entry("Open album", 0), entry("Open artist…", 0), entry("Track radio", 0)]
		);
	}

	#[test]
	fn an_episode_shows_only_open_show() {
		let data = select(&[(EPISODE, &[("ARTIST", &["Sing! Podcast"])])]);
		assert_eq!(shown(&data), [entry("Open show", 0)]);
	}

	#[test]
	fn nothing_selected_shows_nothing() {
		let data = FakeHandleList::new(&[]);
		assert!(shown(&data).is_empty());
	}

	#[test]
	fn several_spotify_items_grey_the_open_commands() {
		let tracks = select(&[(GETTY_TRACK, BOTH_GETTYS), (OTHER_TRACK, BOTH_GETTYS)]);
		assert_eq!(
			shown(&tracks),
			[
				entry("Open album", GREYED),
				entry("Open artist…", GREYED),
				entry("Track radio", GREYED)
			]
		);
		let mixed = select(&[(GETTY_TRACK, BOTH_GETTYS), (EPISODE, NO_FIELDS)]);
		assert_eq!(
			shown(&mixed),
			[
				entry("Open album", GREYED),
				entry("Open artist…", GREYED),
				entry("Track radio", GREYED),
				entry("Open show", GREYED),
			]
		);
		let episodes = select(&[(EPISODE, NO_FIELDS), (EPISODE, NO_FIELDS)]);
		assert_eq!(shown(&episodes), [entry("Open show", GREYED)]);
	}

	#[test]
	fn local_files_dont_count() {
		let local = select(&[(LOCAL, BOTH_GETTYS)]);
		assert!(shown(&local).is_empty());
		let (_, fake) = installed_api_and_backend();
		let data = select(&[(LOCAL, NO_FIELDS), (GETTY_TRACK, BOTH_GETTYS), (LOCAL, NO_FIELDS)]);
		assert_eq!(
			shown(&data),
			[entry("Open album", 0), entry("Open artist…", 0), entry("Track radio", 0)]
		);
		press::<OpenAlbum>(&data);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Album)]);
	}

	#[test]
	fn a_track_with_several_stored_artists_shows_one_open_artist_command() {
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		let root = command::<OpenArtist>().instantiate(&data);
		let node = root.node();
		assert_eq!(node.kind(), TYPE_COMMAND);
		assert_eq!(node.display(&data), Some(entry("Open artist…", 0)));
		assert!(node.children().is_empty());
	}

	#[test]
	fn open_artist_for_fewer_than_two_stored_artists_asks_spotify_without_a_window() {
		let (_, fake) = installed_api_and_backend();
		choose::<OpenArtist>(&select(&[(GETTY_TRACK, ONE_ARTIST)]));
		press::<OpenArtist>(&select(&[(GETTY_TRACK, NO_FIELDS)]));
		assert_eq!(fake.related_calls(), vec![(getty_track(), Related::Artist); 2]);
		assert!(WINDOW.current().is_none(), "no window says Loading…");
	}

	#[test]
	fn each_command_looks_up_what_it_opens() {
		let (_, fake) = installed_api_and_backend();
		let track = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		command::<OpenAlbum>().instantiate(&track).node().execute(&track).unwrap();
		press::<TrackRadio>(&track);
		press::<OpenShow>(&select(&[(EPISODE, NO_FIELDS)]));
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
	fn a_failure_names_the_track_by_its_stored_title_or_else_its_uri() {
		let (api, fake) = installed_api_and_backend();
		fake.related_with(|_, _| Err(CoreError::Unavailable("gone".to_owned())));
		press::<OpenAlbum>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		wait_until(|| api.popups().len() == 1);
		press::<OpenAlbum>(&FakeHandleList::new(&[GETTY_TRACK]));
		wait_until(|| api.popups().len() == 2);
		let messages: Vec<String> =
			api.popups().into_iter().map(|(_, message, _)| message).collect();
		let by_uri = format!("spotify:track:{TRACK_ID}\n\ngone.");
		assert_eq!(messages, ["In Christ Alone\n\ngone.".to_owned(), by_uri]);
	}

	#[test]
	fn a_shortcut_on_two_tracks_says_select_one_and_looks_nothing_up() {
		let (api, fake) = installed_api_and_backend();
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS), (OTHER_TRACK, BOTH_GETTYS)]);
		press::<OpenAlbum>(&data);
		press::<OpenArtist>(&data);
		press::<TrackRadio>(&data);
		press::<OpenShow>(&data);
		assert_eq!(
			api.popups(),
			[
				select_one("Open album", "track"),
				select_one("Open artist…", "track"),
				select_one("Track radio", "track"),
				select_one("Open show", "episode"),
			]
		);
		assert!(fake.related_calls().is_empty());
		assert!(WINDOW.current().is_none());
	}

	#[test]
	fn open_album_on_an_episode_says_select_one_track() {
		let (api, fake) = installed_api_and_backend();
		press::<OpenAlbum>(&select(&[(EPISODE, NO_FIELDS)]));
		assert_eq!(api.popups(), [select_one("Open album", "track")]);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn open_show_on_a_track_says_select_one_episode() {
		let (api, fake) = installed_api_and_backend();
		press::<OpenShow>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		assert_eq!(api.popups(), [select_one("Open show", "episode")]);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn a_shortcut_without_a_spotify_item_says_select_one() {
		let (api, fake) = installed_api_and_backend();
		press::<TrackRadio>(&select(&[(LOCAL, BOTH_GETTYS)]));
		press::<OpenArtist>(&FakeHandleList::new(&[]));
		assert_eq!(
			api.popups(),
			[select_one("Track radio", "track"), select_one("Open artist…", "track")]
		);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn without_popups_the_console_says_select_one() {
		let (api, fake) = installed_api_and_backend();
		api.remove_services(foxide2k::abi::popup::IID);
		press::<OpenShow>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		assert_eq!(
			api.console_lines(),
			[
				"foo_librespot: Open show: Select one Spotify episode first. (foobar2000 has no popup_message)"
			]
		);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn how_a_command_shows_is_known_from_the_first_items_that_settle_it() {
		let (track, episode) = (getty_track(), ItemId::Episode(episode_id()));
		let unread = || std::iter::from_fn(|| -> Option<ItemId> { panic!("read too far") });
		let shown =
			|items: &[ItemId], kind| shown_among(items.iter().copied().chain(unread()), kind);
		assert_eq!(shown(&[track, track], Kind::Track), Shown::Greyed);
		assert_eq!(shown(&[episode, track], Kind::Track), Shown::Greyed);
		assert_eq!(shown_among([track], Kind::Track), Shown::Enabled);
		assert_eq!(shown_among([track], Kind::Episode), Shown::Hidden);
		assert_eq!(shown_among([episode, episode], Kind::Track), Shown::Hidden);
		assert_eq!(shown_among([], Kind::Episode), Shown::Hidden);
	}

	#[test]
	fn every_open_command_is_on_by_default_and_mappable() {
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		for open in [
			command::<OpenAlbum>(),
			command::<OpenArtist>(),
			command::<TrackRadio>(),
			command::<OpenShow>(),
		] {
			assert_eq!(open.enabled_state(), ENABLED_DEFAULT_ON, "{}", open.name());
			assert!(open.instantiate(&data).node().is_mappable_shortcut(), "{}", open.name());
		}
	}

	#[test]
	fn open_artist_shows_loading_for_two_stored_artists_then_lists_them() {
		let (api, fake) = installed_api_and_backend();
		choose::<OpenArtist>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		let dialog = WINDOW.current().expect("the artist window is open");
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(brought_up(), 1);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist)]);
		wait_until(|| api.run_main_thread() > 0);
		pump_messages();
		assert_eq!(artist_window::WINDOW.current(), Some(dialog));
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		destroy(dialog);
	}

	/// The artists of a track other than "In Christ Alone": Stuart Townend and Keith Getty.
	fn townend_and_keith() -> Vec<Opened> {
		let townend = SpotifyId::from_raw(&[0x53; 16]).unwrap();
		vec![Opened::new("Stuart Townend", LinkKind::Artist, townend), getty_artists().remove(0)]
	}

	/// Open artist… on a track that stores one artist, whose lookup runs without the window,
	/// then on "In Christ Alone", which stores two and shows "Loading…". Spotify lists several
	/// artists for each, and the first lookup answers first.
	#[test]
	fn the_newest_artist_lookup_wins_when_an_older_one_answers_first() {
		let (api, fake) = installed_api_and_backend();
		fake.related_with(|item, _| {
			let artists = if item == getty_track() { getty_artists() } else { townend_and_keith() };
			Ok(Found::Artists(artists))
		});
		choose::<OpenArtist>(&select(&[(OTHER_TRACK, ONE_ARTIST)]));
		wait_until(|| fake.related_answered() == 1);
		press::<OpenArtist>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		let dialog = WINDOW.current().expect("the artist window says Loading…");
		wait_until(|| fake.related_answered() == 2);
		assert_eq!(api.run_main_thread(), 2, "the older lookup's answer is settled first");
		pump_messages();
		assert_eq!(WINDOW.current(), Some(dialog));
		assert_eq!(dialog.list_texts(LIST), ["Keith Getty", "Kristyn Getty"]);
		destroy(dialog);
	}
}
