//! The preferences foo_librespot keeps in foobar2000's settings store.

use foo_librespot_core::audio::format::Bitrate;
use foo_librespot_core::resolve::ArtistTracks;
use foxide2k::config_store;

/// The preferred bitrate in kbit/s: 96, 160 or 320.
pub const BITRATE_KEY: &str = "foo_librespot.bitrate";

const BITRATES: [Bitrate; 3] = [Bitrate::Kbps96, Bitrate::Kbps160, Bitrate::Kbps320];

const fn kbps(bitrate: Bitrate) -> i64 {
	match bitrate {
		Bitrate::Kbps96 => 96,
		Bitrate::Kbps160 => 160,
		Bitrate::Kbps320 => 320,
	}
}

/// The preferred bitrate. Anything but 96, 160 or 320 in the store gives the default.
pub fn bitrate() -> Bitrate {
	let stored = config_store::get_int(BITRATE_KEY, kbps(Bitrate::default()));
	BITRATES.into_iter().find(|&bitrate| kbps(bitrate) == stored).unwrap_or_default()
}

pub fn set_bitrate(bitrate: Bitrate) -> foxide2k::error::Result<()> {
	config_store::set_int(BITRATE_KEY, kbps(bitrate))
}

/// The bitrate's item in the page's list: 96, 160 and 320 kbit/s are items 0, 1 and 2.
pub fn bitrate_index(bitrate: Bitrate) -> usize {
	BITRATES.iter().position(|&each| each == bitrate).unwrap_or(BITRATES.len() - 1)
}

/// The bitrate of item `index`; an index past the end gives the default.
pub fn bitrate_at(index: usize) -> Bitrate {
	BITRATES.get(index).copied().unwrap_or_default()
}

/// What an artist link adds: 0 for the popular tracks, 1 for the whole discography.
pub const ARTIST_TRACKS_KEY: &str = "foo_librespot.artist_tracks";

const ARTIST_CHOICES: [ArtistTracks; 2] = [ArtistTracks::Popular, ArtistTracks::Discography];

const fn stored(choice: ArtistTracks) -> i64 {
	match choice {
		ArtistTracks::Popular => 0,
		ArtistTracks::Discography => 1,
	}
}

/// What an artist link adds. Anything but 0 or 1 in the store gives the default, the whole
/// discography.
pub fn artist_tracks() -> ArtistTracks {
	let value = config_store::get_int(ARTIST_TRACKS_KEY, stored(ArtistTracks::default()));
	ARTIST_CHOICES.into_iter().find(|&choice| stored(choice) == value).unwrap_or_default()
}

pub fn set_artist_tracks(choice: ArtistTracks) -> foxide2k::error::Result<()> {
	config_store::set_int(ARTIST_TRACKS_KEY, stored(choice))
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::audio::format::Bitrate;
	use foxide2k::testing::FakeApi;

	use foo_librespot_core::resolve::ArtistTracks;

	use super::{
		ARTIST_TRACKS_KEY, BITRATE_KEY, artist_tracks, bitrate, bitrate_at, bitrate_index,
		set_artist_tracks, set_bitrate,
	};

	#[test]
	fn default_bitrate_is_320() {
		FakeApi::new().install();
		assert_eq!(bitrate(), Bitrate::Kbps320);
	}

	#[test]
	fn bitrate_round_trips() {
		let api = FakeApi::new();
		api.install();
		set_bitrate(Bitrate::Kbps160).unwrap();
		assert_eq!(api.config_int(BITRATE_KEY), Some(160));
		assert_eq!(bitrate(), Bitrate::Kbps160);
		set_bitrate(Bitrate::Kbps96).unwrap();
		assert_eq!(bitrate(), Bitrate::Kbps96);
	}

	#[test]
	fn unknown_stored_values_give_the_default() {
		FakeApi::new().install();
		for stored in [0, 128, 256, -1, 9999] {
			foxide2k::config_store::set_int(BITRATE_KEY, stored).unwrap();
			assert_eq!(bitrate(), Bitrate::Kbps320, "{stored}");
		}
	}

	#[test]
	fn combo_indices_map_both_ways() {
		let bitrates = [Bitrate::Kbps96, Bitrate::Kbps160, Bitrate::Kbps320];
		for (index, bitrate) in bitrates.into_iter().enumerate() {
			assert_eq!(bitrate_index(bitrate), index);
			assert_eq!(bitrate_at(index), bitrate);
		}
		assert_eq!(bitrate_at(7), Bitrate::Kbps320);
	}

	#[test]
	fn artist_links_add_the_whole_discography_by_default() {
		FakeApi::new().install();
		assert_eq!(artist_tracks(), ArtistTracks::Discography);
	}

	#[test]
	fn the_artist_choice_round_trips() {
		let api = FakeApi::new();
		api.install();
		set_artist_tracks(ArtistTracks::Popular).unwrap();
		assert_eq!(api.config_int(ARTIST_TRACKS_KEY), Some(0));
		assert_eq!(artist_tracks(), ArtistTracks::Popular);
		set_artist_tracks(ArtistTracks::Discography).unwrap();
		assert_eq!(api.config_int(ARTIST_TRACKS_KEY), Some(1));
		assert_eq!(artist_tracks(), ArtistTracks::Discography);
	}

	#[test]
	fn unknown_artist_choices_give_the_default() {
		FakeApi::new().install();
		for stored in [-1, 2, 9999] {
			foxide2k::config_store::set_int(ARTIST_TRACKS_KEY, stored).unwrap();
			assert_eq!(artist_tracks(), ArtistTracks::Discography, "{stored}");
		}
	}
}
