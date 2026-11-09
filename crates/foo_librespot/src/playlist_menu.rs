//! Right-click Spotify tracks > Spotify > "Add to playlist…": opens the Add to playlist window
//! ([`crate::playlist_window`]) for the selected Spotify tracks and episodes. There the user adds
//! them to Liked Songs or to playlists the account can change, or removes them.
//!
//! The command shows whenever the selection holds a Spotify item, read from the paths alone. A
//! bound key runs it whatever the menu would show; when the selection holds no Spotify item, a
//! popup says to select one, and the window doesn't open.

use std::ops::ControlFlow;

use foo_librespot_core::link::parse_item;
use foxide2k::contextmenu::{ContextMenuCommand, ContextMenuGroup, Shown};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::metadb::HandleList;

use crate::playlist_window;
use crate::spotify_menu::{Source, SpotifyGroup, spotify_items};

/// The last entry of the right-click Spotify submenu.
pub struct AddToPlaylist;

impl ContextMenuCommand for AddToPlaylist {
	const GUID: Guid = Guid::from_u128(0x9dde669c_a508_460e_b0df_c4e8e681592b);
	const NAME: &'static str = "Add to playlist…";
	const DESCRIPTION: &'static str = "Adds the selected Spotify tracks to Liked Songs or your Spotify playlists, or removes them, in a window that saves on OK.";
	const PARENT: Guid = SpotifyGroup::GUID;
	const SORT_PRIORITY: f64 = 10.0;

	/// Enabled once a path names a Spotify item; hidden when none does or the selection can't be
	/// read.
	fn shown(selection: &HandleList<'_>) -> Shown {
		let found = selection.try_for_each_path(|path| match parse_item(path) {
			Some(_) => ControlFlow::Break(()),
			None => ControlFlow::Continue(()),
		});
		if matches!(found, Ok(ControlFlow::Break(()))) { Shown::Enabled } else { Shown::Hidden }
	}

	fn execute(selection: &HandleList<'_>) -> Result<()> {
		playlist_window::open(spotify_items(selection), Source::Selection, Self::NAME);
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::SpotifyId;
	use foo_librespot_core::library::{Edit, Target};
	use foo_librespot_core::link::{ItemId, item_path};
	use foxide2k::abi::contextmenu::ENABLED_DEFAULT_ON;
	use foxide2k::contextmenu::{ContextMenuCommand, ContextMenuGroup};
	use foxide2k::guid::Guid;
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::FakeHandleList;
	use foxide2k_ui::notify::{BN_CLICKED, IDOK};
	use foxide2k_ui::testing::send_command;

	use super::AddToPlaylist;
	use crate::playlist_window::{LIST, WINDOW};
	use crate::spotify_menu::{OpenAlbum, OpenArtist, OpenShow, SpotifyGroup, TrackRadio};
	use crate::testing::{
		LOCAL, choose, command, entry, episode, getty_track, installed_api_and_backend,
		library_holding, other_track, press, shared_playlist, ticks,
	};
	use crate::window::brought_up;

	/// A third Spotify track.
	fn t3() -> ItemId {
		ItemId::Track(SpotifyId::from_raw(&[0x78; 16]).unwrap())
	}

	/// A selection of `items`, then the local files `locals`.
	fn select(items: &[ItemId], locals: &[&str]) -> FakeHandleList {
		let paths: Vec<String> = items.iter().copied().map(item_path).collect();
		let paths: Vec<&str> =
			paths.iter().map(String::as_str).chain(locals.iter().copied()).collect();
		*FakeHandleList::new(&paths)
	}

	/// The entry the Spotify submenu shows for `data`, with its flags; `None` when hidden.
	fn shown(data: &FakeHandleList) -> Option<(String, u32)> {
		command::<AddToPlaylist>().instantiate(data).node().display(data)
	}

	#[test]
	fn add_to_playlist_is_the_last_spotify_entry_and_takes_a_key() {
		let add = command::<AddToPlaylist>();
		assert_eq!(add.guid(), Guid::from_u128(0x9dde669c_a508_460e_b0df_c4e8e681592b));
		assert_eq!(add.enabled_state(), ENABLED_DEFAULT_ON);
		assert!(add.instantiate(&select(&[getty_track()], &[])).node().is_mappable_shortcut());
		assert_eq!(add.name(), "Add to playlist…");
		assert_eq!(
			add.description().as_deref(),
			Some(
				"Adds the selected Spotify tracks to Liked Songs or your Spotify playlists, or removes them, in a window that saves on OK."
			)
		);
		assert_eq!(add.parent(), SpotifyGroup::GUID);
		assert_eq!(add.sort_priority(), 10.0);
		for open in [
			OpenAlbum::SORT_PRIORITY,
			OpenArtist::SORT_PRIORITY,
			TrackRadio::SORT_PRIORITY,
			OpenShow::SORT_PRIORITY,
		] {
			assert!(open < add.sort_priority());
		}
	}

	#[test]
	fn add_to_playlist_shows_for_any_spotify_selection_and_hides_otherwise() {
		let enabled = Some(entry("Add to playlist…", 0));
		assert_eq!(shown(&select(&[getty_track()], &[])), enabled);
		assert_eq!(shown(&select(&[getty_track(), other_track(), t3()], &[])), enabled);
		assert_eq!(shown(&select(&[episode(), getty_track()], &[])), enabled);
		let local_first = *FakeHandleList::new(&[LOCAL, &item_path(getty_track())]);
		assert_eq!(shown(&local_first), enabled);
		assert_eq!(shown(&select(&[], &[LOCAL, LOCAL])), None);
	}

	/// The playlist "Church / Sunday set" holds both tracks, so its row starts ticked only if the
	/// window counts nothing else; the change OK sends shows both, each once, in order.
	#[test]
	fn the_selection_reaches_the_window_once_per_item_in_order() {
		let (_, fake) = installed_api_and_backend();
		fake.set_library(library_holding(&[getty_track(), other_track()], &[], &[]));
		choose::<AddToPlaylist>(&select(&[other_track(), getty_track(), other_track()], &[LOCAL]));
		let dialog = WINDOW.current().expect("the Add to playlist window is open");
		assert_eq!(brought_up(), 1);
		assert_eq!(ticks(dialog), [false, true, false]);
		dialog.set_row_checked(LIST, 2, true);
		send_command(&dialog, IDOK, BN_CLICKED);
		assert_eq!(
			fake.changes(),
			[(
				Edit::Add,
				Target::Playlist(shared_playlist().id),
				vec![other_track(), getty_track()]
			)]
		);
	}

	#[test]
	fn a_key_with_no_spotify_selection_says_what_to_select() {
		let (api, fake) = installed_api_and_backend();
		fake.set_library(library_holding(&[], &[], &[]));
		press::<AddToPlaylist>(&select(&[], &[LOCAL]));
		press::<AddToPlaylist>(&select(&[], &[]));
		let select_one = (
			"Add to playlist…".to_owned(),
			"Select a Spotify track or episode first.".to_owned(),
			PopupIcon::Information,
		);
		assert_eq!(api.popups(), [select_one.clone(), select_one]);
		assert!(WINDOW.current().is_none());
		assert_eq!(brought_up(), 0);
	}
}
