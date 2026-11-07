//! Right-click > Spotify: opens the album, an artist, the radio or the show of the one Spotify
//! track or episode selected, and holds "Add to playlist".
//!
//! An open command shows only for the kind of item it opens from, a track or an episode, and is
//! greyed out when several Spotify items are selected. Local files in the selection are left
//! out. A bound key runs a command whatever the menu would show, so each command checks the
//! selection again when it runs; when that isn't one Spotify item of its kind, a popup says to
//! select one, and nothing is looked up. The menu reads only what foobar2000 stores and never
//! waits for Spotify.
//!
//! These rules take a list of Spotify items and where it came from, so File > Spotify > Playing
//! track ([`crate::playing_menu`]) follows them for the playing track.

use std::sync::Arc;

use foo_librespot_core::link::{ItemId, parse_item};
use foo_librespot_core::related::Related;
use foxide2k::abi::contextmenu::GROUP_ROOT;
use foxide2k::console;
use foxide2k::contextmenu::{
	ContextMenuCommand, ContextMenuGroup, ContextMenuItem, MenuNode, Shown,
};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::metadb::{HandleList, Track};
use foxide2k::popup::{self, PopupIcon};

use crate::artist_window;
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
		open(spotify_items(selection), Source::Selection, Self::NAME, Kind::Track, Related::Album);
		Ok(())
	}
}

/// Opens an artist of the selected track: a submenu of its stored artists when it has several,
/// or else one command that asks Spotify for them. The submenu's entries change from track to
/// track, so none can be bound to a key; [`OpenArtistShortcut`] can.
pub struct OpenArtist;

impl ContextMenuItem for OpenArtist {
	const GUID: Guid = Guid::from_u128(0x1c41af7c_af4c_42d2_8f16_54dc7b4f752c);
	const NAME: &'static str = "Open artist";
	const DESCRIPTION: &'static str =
		"Opens an artist of the selected Spotify track in a playlist tab.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 1.0;

	fn build(selection: &HandleList<'_>) -> Option<MenuNode> {
		let items = spotify_items(selection);
		match shown_among(&items, Kind::Track) {
			Shown::Hidden => None,
			Shown::Greyed => Some(command(Self::NAME.into(), false, || {
				tell(Self::NAME, Source::Selection.wants(Kind::Track));
			})),
			Shown::Enabled => {
				let (item, track) = &items[0];
				Some(artist_node(*item, track))
			}
		}
	}
}

/// The shortcut-only "Open artist…": opens the selected track's artist when Spotify names one,
/// or lists its artists in the artist window.
pub struct OpenArtistShortcut;

impl ContextMenuCommand for OpenArtistShortcut {
	const GUID: Guid = Guid::from_u128(0xc240e9cc_4c76_4ba1_9c64_85246758c4e5);
	const NAME: &'static str = "Open artist…";
	const DESCRIPTION: &'static str = "Opens the artist of the selected Spotify track in a playlist tab, or lists its artists to pick from.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 1.5;
	const SHORTCUT_ONLY: bool = true;

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
		open(spotify_items(selection), Source::Selection, Self::NAME, Kind::Track, Related::Radio);
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
		open(spotify_items(selection), Source::Selection, Self::NAME, Kind::Episode, Related::Show);
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
}

/// Where an open command finds the item it opens from.
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
}

/// A Spotify track or episode, and the foobar2000 track that holds what is stored for it.
pub type SpotifyItem = (ItemId, Track);

/// `track` with its Spotify item, when it is a Spotify track or episode whose path can be read.
pub fn spotify_item(track: Track) -> Option<SpotifyItem> {
	Some((parse_item(&track.path().ok()?)?, track))
}

/// The Spotify tracks and episodes in `selection`, in order. Local files, and items whose path
/// can't be read, are left out.
fn spotify_items(selection: &HandleList<'_>) -> Vec<SpotifyItem> {
	selection.tracks().unwrap_or_default().into_iter().filter_map(spotify_item).collect()
}

/// How a command that opens from `kind` shows for `selection`.
fn shown_for(selection: &HandleList<'_>, kind: Kind) -> Shown {
	shown_among(&spotify_items(selection), kind)
}

/// Hidden when none of the Spotify `items` is of `kind`, enabled when the only one is, and
/// greyed out among several.
pub fn shown_among(items: &[SpotifyItem], kind: Kind) -> Shown {
	if !items.iter().any(|(item, _)| Kind::of(*item) == kind) {
		Shown::Hidden
	} else if items.len() == 1 {
		Shown::Enabled
	} else {
		Shown::Greyed
	}
}

/// The one Spotify item in `items`, when it is of `kind`. Otherwise a popup titled `command` says
/// what `source` should hold.
fn the_one(
	mut items: Vec<SpotifyItem>,
	kind: Kind,
	source: Source,
	command: &str,
) -> Option<SpotifyItem> {
	if items.len() == 1 && Kind::of(items[0].0) == kind {
		return items.pop();
	}
	tell(command, source.wants(kind));
	None
}

/// Opens `what` for the one Spotify item in `items`, taken from `source`, when it is of `kind`.
pub fn open(items: Vec<SpotifyItem>, source: Source, command: &str, kind: Kind, what: Related) {
	if let Some((item, track)) = the_one(items, kind, source, command) {
		open_related(item, what, title_of(item, &track), None);
	}
}

/// Asks Spotify for every artist of the one Spotify track in `items`, taken from `source`, as
/// [`OpenArtistShortcut`] does: one opens in its tab, several fill the artist window. When
/// foobar2000 stores two or more for the track, the window says "Loading…" meanwhile.
pub fn open_artists(items: Vec<SpotifyItem>, source: Source, command: &str) {
	if let Some((item, track)) = the_one(items, Kind::Track, source, command) {
		let wait = (stored_artists(&track).len() >= 2).then(artist_window::show_loading);
		open_related(item, Related::Artist(None), title_of(item, &track), wait);
	}
}

/// What picking an entry of an artist submenu does.
pub type Pick = Box<dyn Fn() + Send + Sync>;

/// What the artist submenu of a track offers.
pub enum ArtistPicks {
	/// One entry, for a track that stores fewer than two artists, that does what
	/// [`OpenArtistShortcut`] does.
	Every(Pick),
	/// An entry per stored artist, in Spotify's order, named after it and opening it.
	Each(Vec<(Arc<str>, Pick)>),
}

/// What the artist submenu of `track`, the Spotify track `item`, offers.
pub fn artist_picks(item: ItemId, track: &Track) -> ArtistPicks {
	let title = title_of(item, track);
	let artists = stored_artists(track);
	if artists.len() < 2 {
		return ArtistPicks::Every(Box::new(move || {
			open_related(item, Related::Artist(None), title.clone(), None);
		}));
	}
	let picks = artists
		.into_iter()
		.map(|artist| {
			let (name, title) = (Arc::<str>::from(artist), title.clone());
			let artist = Related::Artist(Some(name.to_string()));
			let pick: Pick =
				Box::new(move || open_related(item, artist.clone(), title.clone(), None));
			(name, pick)
		})
		.collect();
	ArtistPicks::Each(picks)
}

/// The submenu of `track`'s stored artists when it has several, each opening that artist; or
/// else the command that does what [`OpenArtistShortcut`] does.
fn artist_node(item: ItemId, track: &Track) -> MenuNode {
	match artist_picks(item, track) {
		ArtistPicks::Every(pick) => command(OpenArtist::NAME.into(), true, pick),
		ArtistPicks::Each(picks) => MenuNode::Popup {
			name: OpenArtist::NAME.into(),
			children: picks.into_iter().map(|(name, pick)| command(name, true, pick)).collect(),
		},
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

/// A command named `name` that runs `run`.
fn command(name: Arc<str>, enabled: bool, run: impl Fn() + Send + Sync + 'static) -> MenuNode {
	MenuNode::Command {
		name,
		checked: false,
		enabled,
		run: Box::new(move || {
			run();
			Ok(())
		}),
	}
}

/// Shows `message` in a popup titled `title`, or in the console when there is no popup.
fn tell(title: &str, message: &str) {
	if let Err(error) = popup::show(title, message, PopupIcon::Information) {
		console::print(&format!("foo_librespot: {title}: {message} ({})", error.message));
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::CoreError;
	use foo_librespot_core::link::ItemId;
	use foo_librespot_core::related::Related;
	use foxide2k::abi::contextmenu::{
		ENABLED_DEFAULT_ON, ENABLED_FORCE_OFF, FLAG_DISABLED, FLAG_GRAYED, GROUP_ROOT,
		TYPE_COMMAND, TYPE_GROUP,
	};
	use foxide2k::contextmenu::{
		ContextMenuCommand, ContextMenuGroup, context_menu_command_service,
		context_menu_group_service, context_menu_service,
	};
	use foxide2k::dialog::host::{Dialog, destroy};
	use foxide2k::guid::Guid;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{
		ContextMenuGroupHandle, ContextMenuHandle, FakeApi, FakeFields, FakeHandleList,
		pump_messages,
	};

	use super::{OpenAlbum, OpenArtist, OpenArtistShortcut, OpenShow, SpotifyGroup, TrackRadio};
	use crate::artist_window::{self, LIST, STATUS, WINDOW};
	use crate::playlist_menu::AddToSpotifyPlaylist;
	use crate::testing::{FakeBackend, episode_id, installed_backend, track_id, wait_until};
	use crate::window::brought_up;

	const GREYED: u32 = FLAG_DISABLED | FLAG_GRAYED;

	/// "In Christ Alone".
	const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";
	/// Another Keith & Kristyn Getty track.
	const OTHER_TRACK: &str = "sptf://spotify:track:0iYaBOYiWTPt2A7JMNK6eO";
	const EPISODE: &str = "sptf://spotify:episode:4QomYTPgZ1mpVbUmxBsUpz";
	const LOCAL: &str = "file://C:\\a.mp3";

	/// What foobar2000 stores for "In Christ Alone".
	const BOTH_GETTYS: FakeFields<'static> =
		&[("ARTIST", &["Keith Getty", "Kristyn Getty"]), ("TITLE", &["In Christ Alone"])];
	const ONE_ARTIST: FakeFields<'static> =
		&[("ARTIST", &["Keith & Kristyn Getty"]), ("TITLE", &["In Christ Alone"])];
	const NO_FIELDS: FakeFields<'static> = &[];

	fn getty_track() -> ItemId {
		ItemId::Track(track_id())
	}

	fn select(tracks: &[(&str, FakeFields<'_>)]) -> FakeHandleList {
		*FakeHandleList::with_meta(tracks)
	}

	fn command<T: ContextMenuCommand>() -> ContextMenuHandle {
		ContextMenuHandle::new(context_menu_command_service::<T>())
	}

	fn open_artist() -> ContextMenuHandle {
		ContextMenuHandle::new(context_menu_service::<OpenArtist>())
	}

	/// Runs `T` as a key bound to it does, on `data`.
	fn press<T: ContextMenuCommand>(data: &FakeHandleList) {
		command::<T>().execute_simple_on(&Guid::NULL, data).unwrap();
	}

	/// The open entries the Spotify submenu shows for `data`, in menu order, with their flags.
	fn shown(data: &FakeHandleList) -> Vec<(String, u32)> {
		let artist = open_artist().instantiate(data);
		[
			command::<OpenAlbum>().instantiate(data).node().display(data),
			artist.node().display(data),
			command::<TrackRadio>().instantiate(data).node().display(data),
			command::<OpenShow>().instantiate(data).node().display(data),
		]
		.into_iter()
		.flatten()
		.collect()
	}

	fn entry(name: &str, flags: u32) -> (String, u32) {
		(name.to_owned(), flags)
	}

	/// The popup a command shows when the selection isn't one Spotify `what`.
	fn select_one(command: &str, what: &str) -> (String, String, PopupIcon) {
		(command.to_owned(), format!("Select one Spotify {what} first."), PopupIcon::Information)
	}

	fn setup() -> (&'static FakeApi, FakeBackend) {
		let api = FakeApi::new();
		api.install();
		(api, installed_backend())
	}

	/// The names the artist window lists.
	fn listed(dialog: Dialog) -> Vec<String> {
		(0..dialog.list_count(LIST)).map(|row| dialog.list_text(LIST, row)).collect()
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
				open_artist(),
				0x1c41af7c_af4c_42d2_8f16_54dc7b4f752c,
				"Open artist",
				"Opens an artist of the selected Spotify track in a playlist tab.",
				1.0,
			),
			(
				command::<OpenArtistShortcut>(),
				0xc240e9cc_4c76_4ba1_9c64_85246758c4e5,
				"Open artist…",
				"Opens the artist of the selected Spotify track in a playlist tab, or lists its artists to pick from.",
				1.5,
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
		let item = ContextMenuHandle::new(context_menu_service::<AddToSpotifyPlaylist>());
		assert_eq!(item.guid(), Guid::from_u128(0x9dde669c_a508_460e_b0df_c4e8e681592b));
		assert_eq!(item.parent(), SpotifyGroup::GUID);
		assert_eq!(item.name(), "Add to playlist");
	}

	#[test]
	fn a_track_shows_album_artist_and_radio_and_no_show() {
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		assert_eq!(
			shown(&data),
			[entry("Open album", 0), entry("Open artist", 0), entry("Track radio", 0)]
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
		let shortcut = command::<OpenArtistShortcut>().instantiate(&data);
		assert_eq!(shortcut.node().display(&data), None);
	}

	#[test]
	fn several_spotify_items_grey_the_open_commands() {
		let tracks = select(&[(GETTY_TRACK, BOTH_GETTYS), (OTHER_TRACK, BOTH_GETTYS)]);
		assert_eq!(
			shown(&tracks),
			[
				entry("Open album", GREYED),
				entry("Open artist", GREYED),
				entry("Track radio", GREYED)
			]
		);
		let root = open_artist().instantiate(&tracks);
		assert_eq!(root.node().kind(), TYPE_COMMAND);
		let mixed = select(&[(GETTY_TRACK, BOTH_GETTYS), (EPISODE, NO_FIELDS)]);
		assert_eq!(
			shown(&mixed),
			[
				entry("Open album", GREYED),
				entry("Open artist", GREYED),
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
		let (_, fake) = setup();
		let data = select(&[(LOCAL, NO_FIELDS), (GETTY_TRACK, BOTH_GETTYS), (LOCAL, NO_FIELDS)]);
		assert_eq!(
			shown(&data),
			[entry("Open album", 0), entry("Open artist", 0), entry("Track radio", 0)]
		);
		press::<OpenAlbum>(&data);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Album)]);
	}

	#[test]
	fn a_track_with_two_artists_gets_an_artist_submenu() {
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		let root = open_artist().instantiate(&data);
		let node = root.node();
		assert_eq!(node.kind(), TYPE_GROUP);
		assert_eq!(node.display(&data), Some(entry("Open artist", 0)));
		let children = node.children();
		let names: Vec<_> = children.iter().map(|child| child.display(&data).unwrap()).collect();
		assert_eq!(names, [entry("Keith Getty", 0), entry("Kristyn Getty", 0)]);
		assert!(
			children
				.iter()
				.all(|child| child.kind() == TYPE_COMMAND && !child.is_mappable_shortcut())
		);
	}

	#[test]
	fn picking_an_artist_looks_up_that_artist() {
		let (api, fake) = setup();
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		let root = open_artist().instantiate(&data);
		root.node().child("Kristyn Getty").unwrap().execute(&data).unwrap();
		assert_eq!(
			fake.related_calls(),
			[(getty_track(), Related::Artist(Some("Kristyn Getty".to_owned())))]
		);
		assert!(WINDOW.current().is_none());
		wait_until(|| api.run_main_thread() > 0);
		assert_eq!(api.location_requests(), [["sptf://spotify:artist:2kxP07DLgs4xlWz8YHlvfh"]]);
	}

	#[test]
	fn a_track_with_one_stored_artist_gets_a_command_that_asks_spotify() {
		let (_, fake) = setup();
		for fields in [ONE_ARTIST, NO_FIELDS] {
			let data = select(&[(GETTY_TRACK, fields)]);
			let root = open_artist().instantiate(&data);
			assert_eq!(root.node().kind(), TYPE_COMMAND);
			assert_eq!(root.node().display(&data), Some(entry("Open artist", 0)));
			root.node().execute(&data).unwrap();
		}
		assert_eq!(
			fake.related_calls(),
			[(getty_track(), Related::Artist(None)), (getty_track(), Related::Artist(None))]
		);
		assert!(WINDOW.current().is_none(), "no window says Loading…");
	}

	#[test]
	fn each_command_looks_up_what_it_opens() {
		let (_, fake) = setup();
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
		let (api, fake) = setup();
		fake.related_with(|_, _| Err(CoreError::Unavailable("gone".to_owned())));
		press::<OpenAlbum>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		wait_until(|| api.popups().len() == 1);
		press::<OpenAlbum>(&FakeHandleList::new(&[GETTY_TRACK]));
		wait_until(|| api.popups().len() == 2);
		let messages: Vec<String> =
			api.popups().into_iter().map(|(_, message, _)| message).collect();
		assert_eq!(
			messages,
			["In Christ Alone\n\ngone.", "spotify:track:2hQxLiI93xdjqENJuKeXjO\n\ngone."]
		);
	}

	#[test]
	fn a_shortcut_on_two_tracks_says_select_one_and_looks_nothing_up() {
		let (api, fake) = setup();
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS), (OTHER_TRACK, BOTH_GETTYS)]);
		press::<OpenAlbum>(&data);
		press::<OpenArtistShortcut>(&data);
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
		let (api, fake) = setup();
		press::<OpenAlbum>(&select(&[(EPISODE, NO_FIELDS)]));
		assert_eq!(api.popups(), [select_one("Open album", "track")]);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn open_show_on_a_track_says_select_one_episode() {
		let (api, fake) = setup();
		press::<OpenShow>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		assert_eq!(api.popups(), [select_one("Open show", "episode")]);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn a_shortcut_without_a_spotify_item_says_select_one() {
		let (api, fake) = setup();
		press::<TrackRadio>(&select(&[(LOCAL, BOTH_GETTYS)]));
		press::<OpenArtistShortcut>(&FakeHandleList::new(&[]));
		assert_eq!(
			api.popups(),
			[select_one("Track radio", "track"), select_one("Open artist…", "track")]
		);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn without_popups_the_console_says_select_one() {
		let (api, fake) = setup();
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
	fn the_greyed_open_artist_says_select_one_when_run() {
		let (api, fake) = setup();
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS), (OTHER_TRACK, BOTH_GETTYS)]);
		open_artist().instantiate(&data).node().execute(&data).unwrap();
		assert_eq!(api.popups(), [select_one("Open artist", "track")]);
		assert!(fake.related_calls().is_empty());
	}

	#[test]
	fn the_shortcut_only_open_artist_is_forced_off_and_mappable() {
		let shortcut = command::<OpenArtistShortcut>();
		assert_eq!(shortcut.enabled_state(), ENABLED_FORCE_OFF);
		assert_eq!(shortcut.name(), "Open artist…");
		let data = select(&[(GETTY_TRACK, BOTH_GETTYS)]);
		let root = shortcut.instantiate(&data);
		assert!(root.node().is_mappable_shortcut());
		assert_eq!(root.node().display(&data), Some(entry("Open artist…", 0)));
		for open in [command::<OpenAlbum>(), command::<TrackRadio>(), command::<OpenShow>()] {
			assert_eq!(open.enabled_state(), ENABLED_DEFAULT_ON, "{}", open.name());
			assert!(open.instantiate(&data).node().is_mappable_shortcut(), "{}", open.name());
		}
		assert_eq!(open_artist().enabled_state(), ENABLED_DEFAULT_ON);
	}

	#[test]
	fn open_artist_shortcut_shows_loading_for_two_stored_artists_then_lists_them() {
		let (api, fake) = setup();
		press::<OpenArtistShortcut>(&select(&[(GETTY_TRACK, BOTH_GETTYS)]));
		let dialog = WINDOW.current().expect("the artist window is open");
		assert_eq!(dialog.text(STATUS), "Loading…");
		assert_eq!(brought_up(), 1);
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist(None))]);
		wait_until(|| api.run_main_thread() > 0);
		pump_messages();
		assert_eq!(artist_window::WINDOW.current(), Some(dialog));
		assert_eq!(listed(dialog), ["Keith Getty", "Kristyn Getty"]);
		destroy(dialog);
	}

	#[test]
	fn open_artist_shortcut_for_one_stored_artist_asks_spotify_without_a_window() {
		let (_, fake) = setup();
		press::<OpenArtistShortcut>(&select(&[(GETTY_TRACK, ONE_ARTIST)]));
		assert!(WINDOW.current().is_none());
		assert_eq!(fake.related_calls(), [(getty_track(), Related::Artist(None))]);
	}
}
