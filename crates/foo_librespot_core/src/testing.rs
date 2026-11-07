//! What the tests of several modules share.

use crate::link::ItemId;
use crate::metadata::{ItemLinks, TrackInfo};

/// Item `id` with plain metadata: "Track" by "Artist" on "Album", disc 1, one second long and
/// playable, with nothing else known.
pub fn plain_info(id: ItemId) -> TrackInfo {
	TrackInfo {
		id,
		title: "Track".to_owned(),
		artists: vec!["Artist".to_owned()],
		album: "Album".to_owned(),
		album_artists: vec!["Artist".to_owned()],
		track_number: 0,
		disc_number: 1,
		total_tracks: 0,
		total_discs: 0,
		date: None,
		duration_ms: 1000,
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
		links: ItemLinks::default(),
	}
}
