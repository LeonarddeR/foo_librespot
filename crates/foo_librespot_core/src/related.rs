//! What a track or episode belongs to: its album, its artists, its show, and its radio.

use crate::link::{ItemId, Link, LinkKind};
use crate::metadata::TrackInfo;
use crate::{CoreError, SpotifyId, SpotifyUri};

/// What to find for a track or episode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Related {
	Album,
	/// The artist with this name, as the track lists it; every artist when `None`.
	Artist(Option<String>),
	/// The playlist Spotify makes from a track.
	Radio,
	/// An episode's show.
	Show,
}

/// A tab to open: its name and the link that fills it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
	pub name: String,
	pub link: Link,
}

/// What was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
	One(Opened),
	/// Every artist of a track with several, in the track's order, for the user to pick one.
	Artists(Vec<Opened>),
}

const NOT_A_TRACK: &str = "this is an episode, not a track";
const NOT_AN_EPISODE: &str = "this is a track, not an episode";
const NO_RADIO: &str = "Spotify's answer named no radio playlist";

/// `what` for `info`. Radio is not answered here.
pub fn related_in(info: &TrackInfo, what: &Related) -> Result<Found, CoreError> {
	let links = &info.links;
	let is_episode = matches!(info.id, ItemId::Episode(_));
	match what {
		Related::Show if !is_episode => Err(unavailable(NOT_AN_EPISODE)),
		Related::Show => {
			let show =
				links.show.ok_or_else(|| unavailable("Spotify names no show for this episode"))?;
			Ok(Found::One(opened(&info.album, LinkKind::Show, show)))
		}
		_ if is_episode => Err(unavailable(NOT_A_TRACK)),
		Related::Album => {
			let album =
				links.album.ok_or_else(|| unavailable("Spotify names no album for this track"))?;
			Ok(Found::One(opened(&info.album, LinkKind::Album, album)))
		}
		Related::Artist(Some(name)) => info
			.artists
			.iter()
			.zip(&links.artists)
			.find(|(artist, _)| *artist == name)
			.map(|(artist, id)| Found::One(opened(artist, LinkKind::Artist, *id)))
			.ok_or_else(|| {
				CoreError::Unavailable(format!("Spotify no longer lists {name} on this track"))
			}),
		Related::Artist(None) => {
			let mut artists: Vec<Opened> = info
				.artists
				.iter()
				.zip(&links.artists)
				.map(|(artist, id)| opened(artist, LinkKind::Artist, *id))
				.collect();
			match artists.len() {
				0 => Err(unavailable("Spotify names no artist for this track")),
				1 => Ok(Found::One(artists.remove(0))),
				_ => Ok(Found::Artists(artists)),
			}
		}
		Related::Radio => Err(unavailable("a track's radio isn't part of its metadata")),
	}
}

/// The track a radio is asked for, or why `item` has none.
pub(crate) fn radio_seed(item: ItemId) -> Result<SpotifyId, CoreError> {
	match item {
		ItemId::Track(id) => Ok(id),
		ItemId::Episode(_) => Err(unavailable(NOT_A_TRACK)),
	}
}

/// The playlist an answer to `get_radio_for_track` (Spotify's `seed_to_playlist`) names as its
/// first item. An answer whose first item is anything else names none.
pub fn radio_playlist(json: &[u8]) -> Result<SpotifyId, CoreError> {
	let answer: serde_json::Value =
		serde_json::from_slice(json).map_err(|_| unavailable(NO_RADIO))?;
	match answer["mediaItems"][0]["uri"].as_str().map(SpotifyUri::from_uri) {
		Some(Ok(SpotifyUri::Playlist { id, .. })) => Ok(id),
		_ => Err(unavailable(NO_RADIO)),
	}
}

fn opened(name: &str, kind: LinkKind, id: SpotifyId) -> Opened {
	Opened { name: name.to_owned(), link: Link::Id { kind, id } }
}

fn unavailable(reason: &str) -> CoreError {
	CoreError::Unavailable(reason.to_owned())
}

#[cfg(test)]
mod tests {
	use super::{Found, Opened, Related, radio_playlist, related_in};
	use crate::link::{ItemId, Link, LinkKind};
	use crate::metadata::{ItemLinks, TrackInfo};
	use crate::{CoreError, SpotifyId};

	/// "In Christ Alone" by Keith & Kristyn Getty.
	const TRACK: &str = "2hQxLiI93xdjqENJuKeXjO";
	/// Its album, "In Christ Alone".
	const ALBUM: &str = "4wPtS24asgBAG4VOmYYB9O";
	const KEITH: &str = "0I4Bk2s2BUJyykCwtxx8Xx";
	const KRISTYN: &str = "2kxP07DLgs4xlWz8YHlvfh";
	const EPISODE: &str = "4QomYTPgZ1mpVbUmxBsUpz";
	const SHOW: &str = "5CfCWKI5pZ28U0uOzXkDHe";
	const RADIO: &str = "37i9dQZF1E8O9hzsgLi5jr";

	fn id(base62: &str) -> SpotifyId {
		SpotifyId::from_base62(base62).unwrap()
	}

	fn getty() -> TrackInfo {
		TrackInfo {
			id: ItemId::Track(id(TRACK)),
			title: "In Christ Alone".to_owned(),
			artists: vec!["Keith Getty".to_owned(), "Kristyn Getty".to_owned()],
			album: "In Christ Alone".to_owned(),
			album_artists: vec!["Keith & Kristyn Getty".to_owned()],
			track_number: 4,
			disc_number: 1,
			total_tracks: 11,
			total_discs: 1,
			date: Some("2007-01-02".to_owned()),
			duration_ms: 279_000,
			covers: Vec::new(),
			has_audio: true,
			unavailable: None,
			replay_gain: None,
			genres: Vec::new(),
			description: None,
			external_url: None,
			isrc: None,
			explicit: false,
			composers: Vec::new(),
			conductors: Vec::new(),
			orchestras: Vec::new(),
			remixers: Vec::new(),
			label: None,
			copyrights: Vec::new(),
			barcode: None,
			release_type: None,
			missing_album_details: false,
			links: ItemLinks {
				album: Some(id(ALBUM)),
				artists: vec![id(KEITH), id(KRISTYN)],
				show: None,
			},
		}
	}

	/// An episode of "Sing! Podcast", whose name stands in for the album.
	fn episode() -> TrackInfo {
		TrackInfo {
			id: ItemId::Episode(id(EPISODE)),
			title: "Singing the Psalms".to_owned(),
			artists: vec!["Sing! Podcast".to_owned()],
			album: "Sing! Podcast".to_owned(),
			album_artists: vec!["Sing! Podcast".to_owned()],
			links: ItemLinks { show: Some(id(SHOW)), ..ItemLinks::default() },
			..getty()
		}
	}

	fn opened(name: &str, kind: LinkKind, base62: &str) -> Opened {
		Opened { name: name.to_owned(), link: Link::Id { kind, id: id(base62) } }
	}

	fn unavailable(result: Result<impl std::fmt::Debug, CoreError>) -> String {
		match result {
			Err(CoreError::Unavailable(reason)) => reason,
			other => panic!("expected Unavailable, got {other:?}"),
		}
	}

	#[test]
	fn album_opens_the_album_named_after_it() {
		assert_eq!(
			related_in(&getty(), &Related::Album).unwrap(),
			Found::One(opened("In Christ Alone", LinkKind::Album, ALBUM))
		);
	}

	#[test]
	fn a_picked_artist_opens_that_artist() {
		assert_eq!(
			related_in(&getty(), &Related::Artist(Some("Kristyn Getty".to_owned()))).unwrap(),
			Found::One(opened("Kristyn Getty", LinkKind::Artist, KRISTYN))
		);
	}

	#[test]
	fn a_picked_artist_spotify_no_longer_lists_fails() {
		let result = related_in(&getty(), &Related::Artist(Some("Someone Else".to_owned())));
		assert_eq!(unavailable(result), "Spotify no longer lists Someone Else on this track");
	}

	#[test]
	fn artist_without_a_pick_lists_every_artist() {
		assert_eq!(
			related_in(&getty(), &Related::Artist(None)).unwrap(),
			Found::Artists(vec![
				opened("Keith Getty", LinkKind::Artist, KEITH),
				opened("Kristyn Getty", LinkKind::Artist, KRISTYN),
			])
		);
		let solo = TrackInfo {
			artists: vec!["Keith Getty".to_owned()],
			links: ItemLinks { artists: vec![id(KEITH)], ..getty().links },
			..getty()
		};
		assert_eq!(
			related_in(&solo, &Related::Artist(None)).unwrap(),
			Found::One(opened("Keith Getty", LinkKind::Artist, KEITH))
		);
	}

	#[test]
	fn show_opens_an_episodes_show() {
		assert_eq!(
			related_in(&episode(), &Related::Show).unwrap(),
			Found::One(opened("Sing! Podcast", LinkKind::Show, SHOW))
		);
	}

	#[test]
	fn show_for_a_track_fails() {
		assert_eq!(
			unavailable(related_in(&getty(), &Related::Show)),
			"this is a track, not an episode"
		);
	}

	#[test]
	fn album_for_an_episode_fails() {
		assert_eq!(
			unavailable(related_in(&episode(), &Related::Album)),
			"this is an episode, not a track"
		);
	}

	#[test]
	fn radio_playlist_reads_the_first_playlist() {
		let answer =
			br#"{"total": 1,"mediaItems": [{"uri": "spotify:playlist:37i9dQZF1E8O9hzsgLi5jr"}]}"#;
		assert_eq!(radio_playlist(answer).unwrap(), id(RADIO));
	}

	#[test]
	fn radio_playlist_without_items_fails() {
		assert_eq!(
			unavailable(radio_playlist(br#"{"total":0,"mediaItems":[]}"#)),
			"Spotify's answer named no radio playlist"
		);
	}

	#[test]
	fn radio_playlist_that_isnt_a_playlist_fails() {
		let answer =
			br#"{"total": 1,"mediaItems": [{"uri": "spotify:track:2hQxLiI93xdjqENJuKeXjO"}]}"#;
		assert_eq!(unavailable(radio_playlist(answer)), "Spotify's answer named no radio playlist");
	}

	#[test]
	fn radio_playlist_whose_first_item_isnt_a_playlist_fails() {
		let answer = br#"{"total": 2,"mediaItems": [{"uri": "spotify:track:2hQxLiI93xdjqENJuKeXjO"},{"uri": "spotify:playlist:37i9dQZF1E8O9hzsgLi5jr"}]}"#;
		assert_eq!(unavailable(radio_playlist(answer)), "Spotify's answer named no radio playlist");
	}
}
