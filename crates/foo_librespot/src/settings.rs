//! The preferences foo_librespot keeps in foobar2000's settings store.

use foo_librespot_core::audio::format::Bitrate;
use oxibar::config_store;

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

#[cfg_attr(not(test), expect(dead_code, reason = "the preferences page (Task 8) uses it"))]
pub fn set_bitrate(bitrate: Bitrate) -> oxibar::error::Result<()> {
	config_store::set_int(BITRATE_KEY, kbps(bitrate))
}

/// The bitrate's item in the page's list: 96, 160 and 320 kbit/s are items 0, 1 and 2.
#[cfg_attr(not(test), expect(dead_code, reason = "the preferences page (Task 8) uses it"))]
pub fn bitrate_index(bitrate: Bitrate) -> usize {
	BITRATES.iter().position(|&each| each == bitrate).unwrap_or(BITRATES.len() - 1)
}

/// The bitrate of item `index`; an index past the end gives the default.
#[cfg_attr(not(test), expect(dead_code, reason = "the preferences page (Task 8) uses it"))]
pub fn bitrate_at(index: usize) -> Bitrate {
	BITRATES.get(index).copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::audio::format::Bitrate;
	use oxibar::testing::FakeApi;

	use super::{BITRATE_KEY, bitrate, bitrate_at, bitrate_index, set_bitrate};

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
			oxibar::config_store::set_int(BITRATE_KEY, stored).unwrap();
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
}
