//! Opening a Spotify link in a playlist tab of its own.
//!
//! The tab is filled as pasting the link would fill it, and appears once the tracks have
//! arrived. It carries the link's Spotify URI as a tab property, so opening the same link again
//! brings that tab to the front; opening it while its tracks load starts nothing.

use std::sync::{Mutex, MutexGuard, PoisonError};

use foo_librespot_core::link::Link;
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::metadb::HandleList;
use foxide2k::playlist;

use crate::links::canonical;

/// The tab property that keeps the Spotify URI of the link a tab was opened from.
pub const TAB_PROPERTY: Guid = Guid::from_u128(0x0c10e611_31ed_4e6e_b689_6a93937de1bc);

/// The URIs of the links whose tracks are on their way to a tab.
static LOADING: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn loading() -> MutexGuard<'static, Vec<String>> {
	LOADING.lock().unwrap_or_else(PoisonError::into_inner)
}

fn done_loading(uri: &str) {
	loading().retain(|loading| loading != uri);
}

/// Brings the tab holding `link` to the front. Without one, loads `link` as pasting it would,
/// unless that load runs already; the tab, named `name`, appears once the tracks have arrived.
/// Main thread only.
pub fn open_in_tab(name: &str, link: &Link) -> Result<()> {
	let uri = link.uri();
	if let Some(index) = playlist::find_by_property(&TAB_PROPERTY, uri.as_bytes())? {
		return playlist::activate(index);
	}
	{
		let mut loading = loading();
		if loading.contains(&uri) {
			return Ok(());
		}
		loading.push(uri.clone());
	}
	let (name, done_uri, aborted_uri) = (name.to_owned(), uri.clone(), uri.clone());
	let loaded = playlist::load_locations(
		&[canonical(link)],
		Box::new(move |items| {
			done_loading(&done_uri);
			fill(&name, &done_uri, items)
		}),
		Box::new(move || done_loading(&aborted_uri)),
	);
	if loaded.is_err() {
		done_loading(&uri);
	}
	loaded
}

/// Appends `items` to the tab holding `uri`. Without one, makes a tab named `name` that holds
/// it, with `items`, and brings it to the front; no `items` make no tab.
fn fill(name: &str, uri: &str, items: &HandleList<'_>) -> Result<()> {
	if let Some(index) = playlist::find_by_property(&TAB_PROPERTY, uri.as_bytes())? {
		return playlist::append(index, items);
	}
	if items.is_empty()? {
		return Ok(());
	}
	let index = playlist::create(name)?;
	playlist::set_property(index, &TAB_PROPERTY, uri.as_bytes())?;
	playlist::append(index, items)?;
	playlist::activate(index)
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::SpotifyId;
	use foo_librespot_core::link::{Link, LinkKind};
	use foxide2k::guid::Guid;
	use foxide2k::testing::FakeApi;

	use super::{TAB_PROPERTY, open_in_tab};
	use crate::testing::{sunday_set, sunday_set_id};

	const GETTY_TRACK: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

	fn installed_api() -> &'static FakeApi {
		let api = FakeApi::new();
		api.install();
		api
	}

	fn sunday_uri() -> String {
		format!("spotify:playlist:{}", sunday_set_id().to_base62())
	}

	/// Opens the Sunday set playlist in its tab.
	fn open_sunday() {
		open_in_tab("Sunday set", &sunday_set().link()).unwrap();
	}

	/// "In Christ Alone" by Keith & Kristyn Getty.
	fn album() -> Link {
		let id = SpotifyId::from_base62("4wPtS24asgBAG4VOmYYB9O").unwrap();
		Link::Id { kind: LinkKind::Album, id }
	}

	#[test]
	fn the_tab_property_keeps_its_guid() {
		assert_eq!(TAB_PROPERTY, Guid::from_u128(0x0c10e611_31ed_4e6e_b689_6a93937de1bc));
	}

	#[test]
	fn opening_a_playlist_loads_its_link_and_makes_no_tab_yet() {
		let api = installed_api();
		open_sunday();
		assert_eq!(api.location_requests(), [[format!("sptf://{}", sunday_uri())]]);
		assert!(api.tabs().is_empty());
	}

	#[test]
	fn the_tracks_arrive_in_a_new_active_tab_named_and_tagged_after_the_playlist() {
		let api = installed_api();
		api.add_tab("Default");
		open_sunday();
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		let tabs = api.tabs();
		assert_eq!(tabs.len(), 2);
		assert_eq!(tabs[1].name, "Sunday set");
		assert_eq!(tabs[1].properties, [(TAB_PROPERTY, sunday_uri().into_bytes())]);
		assert_eq!(tabs[1].paths, [GETTY_TRACK]);
		assert_eq!(api.active_tab(), Some(1));
	}

	#[test]
	fn a_load_with_no_tracks_makes_no_tab_and_can_be_tried_again() {
		let api = installed_api();
		open_sunday();
		api.finish_locations(0, &[]).unwrap();
		assert!(api.tabs().is_empty());
		open_sunday();
		assert_eq!(api.location_requests().len(), 2);
	}

	#[test]
	fn a_cancelled_load_leaves_no_tab_and_can_be_tried_again() {
		let api = installed_api();
		open_sunday();
		api.cancel_locations(0);
		assert!(api.tabs().is_empty());
		open_sunday();
		assert_eq!(api.location_requests().len(), 2);
	}

	#[test]
	fn a_second_open_during_the_load_starts_no_second_import() {
		let api = installed_api();
		open_sunday();
		open_sunday();
		assert_eq!(api.location_requests().len(), 1);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().len(), 1);
	}

	#[test]
	fn a_second_open_of_the_same_album_during_its_load_starts_no_second_load() {
		let api = installed_api();
		open_in_tab("In Christ Alone", &album()).unwrap();
		open_in_tab("In Christ Alone", &album()).unwrap();
		assert_eq!(api.location_requests(), [["sptf://spotify:album:4wPtS24asgBAG4VOmYYB9O"]]);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().len(), 1);
	}

	#[test]
	fn opening_a_playlist_that_has_a_tab_switches_to_it() {
		let api = installed_api();
		open_sunday();
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		let opened = api.tabs().len() - 1;
		api.add_tab("Other");
		foxide2k::playlist::activate(opened + 1).unwrap();
		let tabs_before = api.tabs().len();
		open_sunday();
		assert_eq!(api.tabs().len(), tabs_before);
		assert_eq!(api.location_requests().len(), 1);
		assert_eq!(api.active_tab(), Some(opened));
	}

	/// Adds a tab that holds the Sunday set's URI, as one made while its tracks load would.
	fn add_sunday_tab(api: &FakeApi) -> usize {
		let index = api.add_tab("Sunday set");
		foxide2k::playlist::set_property(index, &TAB_PROPERTY, sunday_uri().as_bytes()).unwrap();
		index
	}

	#[test]
	fn a_tab_made_during_the_load_gets_the_tracks() {
		let api = installed_api();
		open_sunday();
		let index = add_sunday_tab(api);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		assert_eq!(api.tabs().len(), 1);
		assert_eq!(api.tabs()[index].paths, [GETTY_TRACK]);
	}

	#[test]
	fn a_tab_closed_during_the_load_is_made_anew() {
		let api = installed_api();
		open_sunday();
		let index = add_sunday_tab(api);
		api.remove_tab(index);
		api.finish_locations(0, &[GETTY_TRACK]).unwrap();
		let tabs = api.tabs();
		assert_eq!(tabs.len(), 1);
		assert_eq!(tabs[0].name, "Sunday set");
		assert_eq!(tabs[0].paths, [GETTY_TRACK]);
	}
}
