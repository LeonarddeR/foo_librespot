//! Album covers for Spotify tracks, and covers for episodes.

use foxide2k::abort::Abort;
use foxide2k::album_art::{AlbumArt, ArtKind};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::input::Input;

use crate::backend::backend;
use crate::errors::host_error;
use crate::input::{SpotifyInput, item_id, item_or_error};

/// The largest album cover Spotify has, as the front cover. Spotify has no other kinds.
pub struct SpotifyArt;

impl AlbumArt for SpotifyArt {
	const INPUT_GUID: Guid = SpotifyInput::GUID;

	fn is_our_path(path: &str, _extension: &str) -> bool {
		item_id(path).is_some()
	}

	fn query(path: &str, kind: ArtKind, abort: &Abort) -> Result<Option<Vec<u8>>> {
		if kind != ArtKind::FrontCover {
			return Ok(None);
		}
		backend().cover(item_or_error(path)?, &|| abort.is_aborting()).map_err(host_error)
	}
}

#[cfg(test)]
mod tests {
	use foxide2k::abi::album_art::{ARTIST, COVER_BACK, COVER_FRONT};
	use foxide2k::album_art::album_art_service;
	use foxide2k::cxx::ExceptionKind;
	use foxide2k::input::Input;
	use foxide2k::testing::AlbumArtHandle;

	use super::SpotifyArt;

	use crate::input::SpotifyInput;
	use crate::testing::{EPISODE_ID, TRACK_ID, installed_backend};

	fn path() -> String {
		format!("sptf://spotify:track:{TRACK_ID}")
	}

	fn extractor() -> AlbumArtHandle {
		AlbumArtHandle::new(album_art_service::<SpotifyArt>())
	}

	#[test]
	fn art_is_paired_with_the_input() {
		assert_eq!(extractor().guid(), SpotifyInput::GUID);
	}

	#[test]
	fn front_cover_comes_from_the_backend() {
		let fake = installed_backend();
		fake.set_cover(b"\xff\xd8 jpeg".to_vec());
		let instance = extractor().open(&path(), None).unwrap();
		assert_eq!(instance.query(&COVER_FRONT).unwrap(), b"\xff\xd8 jpeg");
	}

	#[test]
	fn other_kinds_are_not_found_without_asking_spotify() {
		let fake = installed_backend();
		fake.set_cover(b"\xff\xd8 jpeg".to_vec());
		let instance = extractor().open(&path(), None).unwrap();
		for kind in [COVER_BACK, ARTIST] {
			assert_eq!(instance.query(&kind).unwrap_err().kind, ExceptionKind::AlbumArtNotFound);
		}
		assert_eq!(fake.calls(), 0);
	}

	#[test]
	fn a_track_without_a_cover_is_not_found() {
		installed_backend();
		let instance = extractor().open(&path(), None).unwrap();
		assert_eq!(instance.query(&COVER_FRONT).unwrap_err().kind, ExceptionKind::AlbumArtNotFound);
	}

	#[test]
	fn art_for_foreign_paths_is_not_ours() {
		let extractor = extractor();
		assert!(extractor.is_our_path(&path(), ""));
		assert!(!extractor.is_our_path("file://C:\\a.mp3", "mp3"));
		assert!(!extractor.is_our_path("sptf://spotify:album:4wPtS24asgBAG4VOmYYB9O", ""));
		assert!(extractor.is_our_path(&format!("sptf://spotify:episode:{EPISODE_ID}"), ""));
		assert!(!extractor.is_our_path("sptf://spotify:show:5CfCWKI5pZ28U0uOzXkDHe", ""));
	}
}
