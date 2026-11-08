//! Right-click Spotify tracks > Spotify > "Add to playlist": a submenu that adds the selected
//! tracks to Liked Songs or to a playlist the account can change, and removes them from one that
//! holds them all.
//!
//! The menu is built from the library as last read and never waits for Spotify. Each change runs
//! in the background and reports in the console; a failure also shows a popup.

use std::collections::HashSet;
use std::sync::Arc;

use foo_librespot_core::CoreError;
use foo_librespot_core::library::{
	Change, Edit, LIKED_SONGS, Library, LibraryState, Target, tracks_of,
};
use foo_librespot_core::link::{ItemId, parse_item};
use foxide2k::console;
use foxide2k::contextmenu::{ContextMenuGroup, ContextMenuItem, MenuNode};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::metadb::HandleList;
use foxide2k::popup;

use crate::backend::backend;
use crate::errors::report_failure;
use crate::first_use::ask_for_login;
use crate::playlist_save::report;
use crate::playlist_window::Planned;
use crate::spotify_menu::SpotifyGroup;

const NAME: &str = "Add to playlist";

/// The last entry of the right-click Spotify submenu.
pub struct AddToSpotifyPlaylist;

impl ContextMenuItem for AddToSpotifyPlaylist {
	const GUID: Guid = Guid::from_u128(0x9dde669c_a508_460e_b0df_c4e8e681592b);
	const NAME: &'static str = NAME;
	const DESCRIPTION: &'static str = "Adds the selected Spotify tracks to Liked Songs or one of your Spotify playlists, or removes them from one that has them all.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 10.0;

	fn build(selection: &HandleList<'_>) -> Option<MenuNode> {
		let mut seen = HashSet::new();
		let mut items = Vec::new();
		selection
			.for_each_path(|path| {
				if let Some(item) = parse_item(path)
					&& seen.insert(item)
				{
					items.push(item);
				}
			})
			.ok()?;
		if items.is_empty() {
			return None;
		}
		let children = match backend().library() {
			LibraryState::Ready(library) if library.is_complete() => {
				targets(&library, &items.into())
			}
			LibraryState::Loading | LibraryState::Ready(_) => {
				vec![greyed("Loading your playlists…")]
			}
			LibraryState::LoggedOut => vec![login_entry()],
			LibraryState::Failed(reason) => {
				console::print(&format!(
					"foo_librespot: couldn't load your Spotify playlists: {reason}"
				));
				vec![greyed("Couldn't load your playlists")]
			}
		};
		Some(MenuNode::Popup { name: NAME.into(), children })
	}
}

fn greyed(name: &str) -> MenuNode {
	MenuNode::command(name, false, Box::new(|| Ok(())))
}

fn login_entry() -> MenuNode {
	MenuNode::command(
		"Log in to Spotify…",
		true,
		Box::new(|| {
			ask_for_login();
			Ok(())
		}),
	)
}

/// Liked Songs, then each playlist the account can change. `items` holds each item once; every
/// entry shares it.
fn targets(library: &Library, items: &Arc<[ItemId]>) -> Vec<MenuNode> {
	let tracks = tracks_of(items).len();
	let liked = Pick {
		target: Target::LikedSongs,
		name: LIKED_SONGS.into(),
		checked: library.holds(Target::LikedSongs, items) == Some(true),
		items: Arc::clone(items),
		count: tracks,
	};
	let mut nodes = vec![liked.node(tracks > 0)];
	nodes.extend(library.editable().map(|playlist| {
		let target = Target::Playlist(playlist.id);
		Pick {
			target,
			name: playlist.display_name().into(),
			checked: library.holds(target, items) == Some(true),
			items: Arc::clone(items),
			count: items.len(),
		}
		.node(true)
	}));
	nodes
}

/// One entry of the submenu: where the selection would go, and whether it is all there already.
#[derive(Clone)]
struct Pick {
	target: Target,
	name: Arc<str>,
	checked: bool,
	items: Arc<[ItemId]>,
	/// How many tracks the question and the report speak of.
	count: usize,
}

impl Pick {
	fn node(self, enabled: bool) -> MenuNode {
		MenuNode::Command {
			name: Arc::clone(&self.name),
			checked: self.checked,
			enabled,
			run: Box::new(move || self.choose()),
		}
	}

	/// Adds the selection, or after a yes removes it from an entry that holds it all.
	fn choose(&self) -> Result<()> {
		if !self.checked {
			self.start(Edit::Add);
			return Ok(());
		}
		let question = if self.count == 1 {
			format!("Remove the selected track from {}?", self.name)
		} else {
			format!("Remove the {} selected tracks from {}?", self.count, self.name)
		};
		if popup::ask("Remove from Spotify playlist", &question)? {
			self.start(Edit::Remove);
		}
		Ok(())
	}

	/// Starts the change; it reports on the thread it ends on.
	fn start(&self, edit: Edit) {
		let pick = self.clone();
		backend().change(
			edit,
			self.target,
			&self.items,
			Box::new(move |result| pick.end(edit, result)),
		);
	}

	fn end(&self, edit: Edit, result: std::result::Result<Change, CoreError>) {
		match result {
			Ok(change) => {
				let planned = Planned { edit, target: self.target, name: self.name.to_string() };
				report(&planned, change);
			}
			Err(CoreError::Aborted) => {}
			Err(error) => self.fail(&error),
		}
	}

	fn fail(&self, error: &CoreError) {
		let failed = format!("can't change {}", self.name);
		report_failure(&failed, "Can't change the Spotify playlist", &self.name, error);
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::library::{Change, Edit, Library, LibraryState, Target};
	use foo_librespot_core::link::{ItemId, item_path};
	use foo_librespot_core::{CoreError, SpotifyId};
	use foxide2k::abi::contextmenu::{FLAG_CHECKED, FLAG_DISABLED, FLAG_GRAYED};
	use foxide2k::abi::popup::BUTTON_NO;
	use foxide2k::contextmenu::{
		ContextMenuCommand, ContextMenuGroup, ContextMenuItem, context_menu_service,
	};
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::{ContextMenuHandle, FakeApi, FakeHandleList, NodeHandle};

	use super::AddToSpotifyPlaylist;
	use crate::spotify_menu::{OpenAlbum, OpenArtist, OpenShow, SpotifyGroup, TrackRadio};
	use crate::testing::{
		FakeBackend, episode_id, installed_api_and_backend, installed_prompt, library_holding,
		sunday_set, sunday_set_id, track_id, wait_until,
	};

	const GREYED: u32 = FLAG_DISABLED | FLAG_GRAYED;

	fn t1() -> ItemId {
		ItemId::Track(track_id())
	}

	fn t2() -> ItemId {
		ItemId::Track(SpotifyId::from_raw(&[0x77; 16]).unwrap())
	}

	fn episode() -> ItemId {
		ItemId::Episode(episode_id())
	}

	fn item() -> ContextMenuHandle {
		ContextMenuHandle::new(context_menu_service::<AddToSpotifyPlaylist>())
	}

	fn list(paths: &[String]) -> FakeHandleList {
		*FakeHandleList::new(&paths.iter().map(String::as_str).collect::<Vec<_>>())
	}

	/// Opens the menu for `items`.
	fn open(items: &[ItemId]) -> (NodeHandle, FakeHandleList) {
		let data = list(&items.iter().copied().map(item_path).collect::<Vec<_>>());
		(item().instantiate(&data), data)
	}

	fn entries(root: &NodeHandle, data: &FakeHandleList) -> Vec<(String, u32)> {
		root.node().children().iter().map(|child| child.display(data).unwrap()).collect()
	}

	fn names(root: &NodeHandle, data: &FakeHandleList) -> Vec<String> {
		entries(root, data).into_iter().map(|(name, _)| name).collect()
	}

	fn pick(root: &NodeHandle, data: &FakeHandleList, name: &str) {
		root.node().child(name).unwrap().execute(data).unwrap();
	}

	fn printed(api: &FakeApi, line: &str) -> bool {
		api.console_lines().iter().any(|printed| printed == line)
	}

	fn setup(state: LibraryState) -> (&'static FakeApi, FakeBackend) {
		let (api, fake) = installed_api_and_backend();
		fake.set_library(state);
		(api, fake)
	}

	#[test]
	fn the_item_sits_last_in_the_spotify_submenu() {
		let item = item();
		assert_eq!(item.parent(), SpotifyGroup::GUID);
		assert_eq!(item.sort_priority(), 10.0);
		for open in [
			OpenAlbum::SORT_PRIORITY,
			OpenArtist::SORT_PRIORITY,
			TrackRadio::SORT_PRIORITY,
			OpenShow::SORT_PRIORITY,
		] {
			assert!(open < item.sort_priority());
		}
		assert_eq!(item.name(), "Add to playlist");
		assert_eq!(
			item.description().unwrap(),
			"Adds the selected Spotify tracks to Liked Songs or one of your Spotify playlists, or removes them from one that has them all."
		);
		assert_eq!(item.guid(), AddToSpotifyPlaylist::GUID);
	}

	#[test]
	fn the_item_is_hidden_without_spotify_tracks() {
		let (_, fake) = setup(library_holding(&[], &[], &[]));
		let data = list(&["file://C:\\a.mp3".to_owned()]);
		let root = item().instantiate(&data);
		assert!(root.node().display(&data).is_none());
		assert_eq!(fake.library_calls(), 0);
	}

	#[test]
	fn logged_out_the_submenu_offers_a_login() {
		let (_, fake) = setup(LibraryState::LoggedOut);
		let prompt = installed_prompt();
		let (root, data) = open(&[t1()]);
		assert_eq!(root.node().display(&data).unwrap().0, "Add to playlist");
		assert_eq!(entries(&root, &data), [("Log in to Spotify…".to_owned(), 0)]);
		pick(&root, &data, "Log in to Spotify…");
		assert_eq!(prompt.shown(), 1);
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn before_the_lists_are_known_the_submenu_says_loading() {
		let (_, fake) = setup(LibraryState::Loading);
		let (root, data) = open(&[t1()]);
		assert_eq!(entries(&root, &data), [("Loading your playlists…".to_owned(), GREYED)]);
		let partial = Library::from_parts(vec![sunday_set()], Vec::new(), None);
		fake.set_library(LibraryState::Ready(std::sync::Arc::new(partial)));
		let (root, data) = open(&[t1()]);
		assert_eq!(entries(&root, &data), [("Loading your playlists…".to_owned(), GREYED)]);
	}

	#[test]
	fn a_failed_fetch_says_so_greyed_out() {
		let (api, _) = setup(LibraryState::Failed("Spotify is unreachable".to_owned()));
		let (root, data) = open(&[t1()]);
		assert_eq!(entries(&root, &data), [("Couldn't load your playlists".to_owned(), GREYED)]);
		assert!(printed(
			api,
			"foo_librespot: couldn't load your Spotify playlists: Spotify is unreachable"
		));
	}

	#[test]
	fn the_submenu_lists_liked_songs_then_the_playlists_you_can_change() {
		setup(library_holding(&[], &[], &[]));
		let (root, data) = open(&[t1()]);
		assert_eq!(names(&root, &data), ["Liked Songs", "Church / Sunday set", "Shared"]);
		assert!(entries(&root, &data).iter().all(|(_, flags)| *flags == 0));
	}

	#[test]
	fn a_checkmark_shows_where_every_selected_track_already_is() {
		setup(library_holding(&[t1(), t2()], &[t1()], &[t1(), t2()]));
		let (root, data) = open(&[t1(), t2()]);
		assert_eq!(
			entries(&root, &data),
			[
				("Liked Songs".to_owned(), FLAG_CHECKED),
				("Church / Sunday set".to_owned(), FLAG_CHECKED),
				("Shared".to_owned(), 0),
			]
		);
	}

	#[test]
	fn liked_songs_is_greyed_for_episodes_only_and_checks_tracks_only() {
		setup(library_holding(&[], &[], &[t1()]));
		let (root, data) = open(&[episode()]);
		assert_eq!(entries(&root, &data)[0], ("Liked Songs".to_owned(), GREYED));
		let (root, data) = open(&[t1(), episode()]);
		assert_eq!(entries(&root, &data)[0], ("Liked Songs".to_owned(), FLAG_CHECKED));
		let (root, data) = open(&[t1(), t2()]);
		assert_eq!(entries(&root, &data)[0], ("Liked Songs".to_owned(), 0));
	}

	#[test]
	fn local_files_in_the_selection_are_left_out() {
		let (_, fake) = setup(library_holding(&[], &[], &[]));
		let data = list(&["file://C:\\a.mp3".to_owned(), item_path(t1())]);
		let root = item().instantiate(&data);
		pick(&root, &data, "Church / Sunday set");
		wait_until(|| !fake.changes().is_empty());
		let changes = fake.changes();
		assert_eq!(changes.len(), 1);
		assert_eq!(
			(changes[0].0, changes[0].1, changes[0].2.clone()),
			(Edit::Add, Target::Playlist(sunday_set_id()), vec![t1()])
		);
	}

	#[test]
	fn picking_an_unchecked_playlist_adds_the_selection() {
		let (api, fake) = setup(library_holding(&[], &[], &[]));
		fake.change_with(|_| Ok(Change { changed: 2, unchanged: 1, episodes_left_out: 0 }));
		let (root, data) = open(&[t1(), t2()]);
		pick(&root, &data, "Church / Sunday set");
		wait_until(|| {
			printed(
				api,
				"foo_librespot: added 2 tracks to Church / Sunday set; 1 was already there",
			)
		});
		assert!(api.questions().is_empty());
	}

	#[test]
	fn saving_to_liked_songs_says_what_was_left_out() {
		let (api, fake) = setup(library_holding(&[], &[], &[]));
		fake.change_with(|_| Ok(Change { changed: 1, unchanged: 2, episodes_left_out: 1 }));
		let (root, data) = open(&[t1(), episode()]);
		pick(&root, &data, "Liked Songs");
		wait_until(|| {
			printed(api, "foo_librespot: left out 1 episode: Liked Songs holds songs only")
		});
		assert!(printed(api, "foo_librespot: saved 1 track to Liked Songs; 2 were already there"));
		assert_eq!(fake.changes()[0].1, Target::LikedSongs);
	}

	#[test]
	fn picking_a_checked_playlist_asks_before_removing() {
		let (api, fake) = setup(library_holding(&[t1(), t2()], &[], &[]));
		let (root, data) = open(&[t1(), t2()]);
		api.answer_questions(false);
		pick(&root, &data, "Church / Sunday set");
		assert_eq!(
			api.questions(),
			[(
				"Remove from Spotify playlist".to_owned(),
				"Remove the 2 selected tracks from Church / Sunday set?".to_owned(),
				BUTTON_NO
			)]
		);
		assert!(fake.changes().is_empty());
		api.answer_questions(true);
		pick(&root, &data, "Church / Sunday set");
		wait_until(|| printed(api, "foo_librespot: removed 2 tracks from Church / Sunday set"));
		let changes = fake.changes();
		assert_eq!(changes.len(), 1);
		assert_eq!(changes[0].0, Edit::Remove);
	}

	#[test]
	fn the_question_for_one_track_is_singular() {
		let (api, _) = setup(library_holding(&[t1()], &[], &[]));
		let (root, data) = open(&[t1()]);
		pick(&root, &data, "Church / Sunday set");
		assert_eq!(api.questions()[0].1, "Remove the selected track from Church / Sunday set?");
	}

	#[test]
	fn the_question_for_liked_songs_counts_only_the_tracks() {
		let (api, _) = setup(library_holding(&[], &[], &[t1(), t2()]));
		let (root, data) = open(&[t1(), episode(), t2()]);
		pick(&root, &data, "Liked Songs");
		assert_eq!(api.questions()[0].1, "Remove the 2 selected tracks from Liked Songs?");
	}

	#[test]
	fn a_track_selected_twice_is_counted_once() {
		let (api, _) = setup(library_holding(&[t1(), t2()], &[], &[]));
		let (root, data) = open(&[t1(), t2(), t1()]);
		pick(&root, &data, "Church / Sunday set");
		assert_eq!(api.questions()[0].1, "Remove the 2 selected tracks from Church / Sunday set?");
	}

	#[test]
	fn leaving_out_several_episodes_says_episodes() {
		let (api, fake) = setup(library_holding(&[], &[], &[]));
		fake.change_with(|_| Ok(Change { changed: 1, unchanged: 0, episodes_left_out: 2 }));
		let (root, data) = open(&[t1(), episode()]);
		pick(&root, &data, "Liked Songs");
		wait_until(|| {
			printed(api, "foo_librespot: left out 2 episodes: Liked Songs holds songs only")
		});
	}

	#[test]
	fn a_failed_change_shows_a_popup_saying_why() {
		let (api, fake) = setup(library_holding(&[], &[], &[]));
		fake.change_with(|_| Err(CoreError::Unavailable("Spotify said no".to_owned())));
		let (root, data) = open(&[t1()]);
		pick(&root, &data, "Church / Sunday set");
		wait_until(|| !api.popups().is_empty());
		assert_eq!(
			api.popups(),
			[(
				"Can't change the Spotify playlist".to_owned(),
				"Church / Sunday set\n\nSpotify said no.".to_owned(),
				PopupIcon::Error
			)]
		);
		assert!(printed(api, "foo_librespot: can't change Church / Sunday set: Spotify said no"));
	}

	fn reported_a_failure(api: &FakeApi) -> bool {
		!api.popups().is_empty()
			|| api
				.console_lines()
				.iter()
				.any(|line| line.starts_with("foo_librespot: can't change"))
	}

	#[test]
	fn an_aborted_change_reports_nothing() {
		let (api, fake) = setup(library_holding(&[], &[], &[]));
		fake.change_with(|_| Err(CoreError::Aborted));
		let (root, data) = open(&[t1()]);
		pick(&root, &data, "Shared");
		wait_until(|| fake.changes().len() == 1);
		std::thread::sleep(std::time::Duration::from_millis(200));
		assert!(!reported_a_failure(api), "{:?} {:?}", api.popups(), api.console_lines());
	}

	#[test]
	fn the_change_runs_and_reports_off_the_main_thread() {
		let (api, fake) = setup(library_holding(&[], &[], &[]));
		let (root, data) = open(&[t1()]);
		pick(&root, &data, "Shared");
		assert_eq!(fake.changes().len(), 1);
		assert_ne!(fake.changes()[0].3, std::thread::current().id());
		wait_until(|| printed(api, "foo_librespot: added 1 track to Shared"));
	}
}
