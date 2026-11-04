//! Choosing which of a track's audio files to stream.

use librespot_metadata::audio::{AudioFileFormat, AudioFiles};

use super::header::SPOTIFY_OGG_HEADER_END;
use crate::FileId;

/// The bitrate the user prefers. Spotify Premium offers all three.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bitrate {
	Kbps96,
	Kbps160,
	#[default]
	Kbps320,
}

/// The formats to try, best match first; librespot's order in `playback/src/player.rs`.
pub fn format_order(bitrate: Bitrate) -> [AudioFileFormat; 7] {
	use AudioFileFormat::{
		MP3_96, MP3_160, MP3_256, MP3_320, OGG_VORBIS_96, OGG_VORBIS_160, OGG_VORBIS_320,
	};
	match bitrate {
		Bitrate::Kbps96 => {
			[OGG_VORBIS_96, MP3_96, OGG_VORBIS_160, MP3_160, MP3_256, OGG_VORBIS_320, MP3_320]
		}
		Bitrate::Kbps160 => {
			[OGG_VORBIS_160, MP3_160, OGG_VORBIS_96, MP3_96, MP3_256, OGG_VORBIS_320, MP3_320]
		}
		Bitrate::Kbps320 => {
			[OGG_VORBIS_320, MP3_320, MP3_256, OGG_VORBIS_160, MP3_160, OGG_VORBIS_96, MP3_96]
		}
	}
}

/// The first file in [`format_order`] that the track has.
pub fn pick_file(files: &AudioFiles, bitrate: Bitrate) -> Option<(AudioFileFormat, FileId)> {
	format_order(bitrate)
		.into_iter()
		.find_map(|format| files.0.get(&format).map(|file_id| (format, *file_id)))
}

/// How fast librespot should expect to need data, from its table in `playback/src/player.rs`.
pub fn bytes_per_second(format: AudioFileFormat) -> usize {
	const KIB: usize = 1024;
	match format {
		AudioFileFormat::OGG_VORBIS_96 | AudioFileFormat::MP3_96 => 12 * KIB,
		AudioFileFormat::OGG_VORBIS_160
		| AudioFileFormat::MP3_160
		| AudioFileFormat::MP3_160_ENC
		| AudioFileFormat::AAC_160 => 20 * KIB,
		AudioFileFormat::OGG_VORBIS_320
		| AudioFileFormat::MP3_320
		| AudioFileFormat::AAC_320
		| AudioFileFormat::OTHER5 => 40 * KIB,
		AudioFileFormat::MP3_256 => 32 * KIB,
		AudioFileFormat::AAC_24
		| AudioFileFormat::XHE_AAC_24
		| AudioFileFormat::FLAC_FLAC_24BIT => 3 * KIB,
		AudioFileFormat::AAC_48 => 6 * KIB,
		AudioFileFormat::MP4_128 => 16 * KIB,
		AudioFileFormat::FLAC_FLAC => 112 * KIB,
		AudioFileFormat::XHE_AAC_12 => 3 * KIB / 2,
		AudioFileFormat::XHE_AAC_16 => 2 * KIB,
	}
}

/// Where the decodable stream starts: after Spotify's header for Ogg Vorbis, at 0 otherwise.
pub fn header_offset(format: AudioFileFormat) -> u64 {
	if AudioFiles::is_ogg_vorbis(format) { SPOTIFY_OGG_HEADER_END } else { 0 }
}

pub fn mime_type(format: AudioFileFormat) -> Option<&'static str> {
	AudioFiles::mime_type(format)
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use librespot_metadata::audio::{AudioFileFormat, AudioFiles};

	use super::{Bitrate, bytes_per_second, format_order, header_offset, pick_file};
	use crate::FileId;

	fn files(formats: &[AudioFileFormat]) -> AudioFiles {
		let map: HashMap<_, _> = formats
			.iter()
			.enumerate()
			.map(|(index, format)| {
				(*format, FileId::from_raw(&[u8::try_from(index).unwrap() + 1; 20]))
			})
			.collect();
		AudioFiles(map)
	}

	#[test]
	fn format_order_matches_librespot() {
		use AudioFileFormat::{
			MP3_96, MP3_160, MP3_256, MP3_320, OGG_VORBIS_96, OGG_VORBIS_160, OGG_VORBIS_320,
		};
		assert_eq!(
			format_order(Bitrate::Kbps96),
			[OGG_VORBIS_96, MP3_96, OGG_VORBIS_160, MP3_160, MP3_256, OGG_VORBIS_320, MP3_320]
		);
		assert_eq!(
			format_order(Bitrate::Kbps160),
			[OGG_VORBIS_160, MP3_160, OGG_VORBIS_96, MP3_96, MP3_256, OGG_VORBIS_320, MP3_320]
		);
		assert_eq!(
			format_order(Bitrate::Kbps320),
			[OGG_VORBIS_320, MP3_320, MP3_256, OGG_VORBIS_160, MP3_160, OGG_VORBIS_96, MP3_96]
		);
	}

	#[test]
	fn pick_file_prefers_the_requested_bitrate_then_falls_back() {
		let both = files(&[AudioFileFormat::OGG_VORBIS_160, AudioFileFormat::OGG_VORBIS_320]);
		assert_eq!(
			pick_file(&both, Bitrate::Kbps320).map(|(format, _)| format),
			Some(AudioFileFormat::OGG_VORBIS_320)
		);
		let low = files(&[AudioFileFormat::OGG_VORBIS_160]);
		assert_eq!(
			pick_file(&low, Bitrate::Kbps320),
			Some((AudioFileFormat::OGG_VORBIS_160, FileId::from_raw(&[1; 20])))
		);
		assert_eq!(pick_file(&files(&[AudioFileFormat::AAC_24]), Bitrate::Kbps320), None);
	}

	#[test]
	fn bytes_per_second_for_vorbis() {
		assert_eq!(bytes_per_second(AudioFileFormat::OGG_VORBIS_96), 12288);
		assert_eq!(bytes_per_second(AudioFileFormat::OGG_VORBIS_160), 20480);
		assert_eq!(bytes_per_second(AudioFileFormat::OGG_VORBIS_320), 40960);
	}

	#[test]
	fn header_offset_only_for_vorbis() {
		assert_eq!(header_offset(AudioFileFormat::OGG_VORBIS_320), 0xa7);
		assert_eq!(header_offset(AudioFileFormat::MP3_320), 0);
	}

	#[test]
	fn default_bitrate_is_320() {
		assert_eq!(Bitrate::default(), Bitrate::Kbps320);
	}
}
