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

/// What foobar2000 shows about an audio file: its codec and nominal bitrate in kbit/s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileFormat {
	pub codec: &'static str,
	pub kbps: Option<u32>,
}

impl From<AudioFileFormat> for FileFormat {
	fn from(format: AudioFileFormat) -> Self {
		Self { codec: codec_name(format), kbps: kbps(format) }
	}
}

/// The nominal bitrate in kbit/s, as the format's name gives it; `None` for lossless FLAC.
pub const fn kbps(format: AudioFileFormat) -> Option<u32> {
	use AudioFileFormat as F;
	match format {
		F::XHE_AAC_12 => Some(12),
		F::XHE_AAC_16 => Some(16),
		F::AAC_24 | F::XHE_AAC_24 => Some(24),
		F::AAC_48 => Some(48),
		F::OGG_VORBIS_96 | F::MP3_96 => Some(96),
		F::MP4_128 => Some(128),
		F::OGG_VORBIS_160 | F::MP3_160 | F::MP3_160_ENC | F::AAC_160 => Some(160),
		F::MP3_256 => Some(256),
		F::OGG_VORBIS_320 | F::MP3_320 | F::AAC_320 | F::OTHER5 => Some(320),
		F::FLAC_FLAC | F::FLAC_FLAC_24BIT => None,
	}
}

/// The codec as foobar2000 names it in its `codec` field.
pub const fn codec_name(format: AudioFileFormat) -> &'static str {
	use AudioFileFormat as F;
	match format {
		F::OGG_VORBIS_96 | F::OGG_VORBIS_160 | F::OGG_VORBIS_320 => "Vorbis",
		F::MP3_96 | F::MP3_160 | F::MP3_160_ENC | F::MP3_256 | F::MP3_320 => "MP3",
		F::AAC_24 | F::AAC_48 | F::AAC_160 | F::AAC_320 | F::MP4_128 | F::OTHER5 => "AAC",
		F::XHE_AAC_12 | F::XHE_AAC_16 | F::XHE_AAC_24 => "xHE-AAC",
		F::FLAC_FLAC | F::FLAC_FLAC_24BIT => "FLAC",
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

	use super::{
		Bitrate, bytes_per_second, codec_name, format_order, header_offset, kbps, pick_file,
	};
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
	fn kbps_is_the_nominal_bitrate() {
		assert_eq!(kbps(AudioFileFormat::OGG_VORBIS_96), Some(96));
		assert_eq!(kbps(AudioFileFormat::OGG_VORBIS_160), Some(160));
		assert_eq!(kbps(AudioFileFormat::OGG_VORBIS_320), Some(320));
		assert_eq!(kbps(AudioFileFormat::MP3_256), Some(256));
		assert_eq!(kbps(AudioFileFormat::FLAC_FLAC), None);
	}

	#[test]
	fn codec_names_as_foobar2000_shows_them() {
		assert_eq!(codec_name(AudioFileFormat::OGG_VORBIS_320), "Vorbis");
		assert_eq!(codec_name(AudioFileFormat::MP3_160), "MP3");
		assert_eq!(codec_name(AudioFileFormat::AAC_320), "AAC");
		assert_eq!(codec_name(AudioFileFormat::FLAC_FLAC), "FLAC");
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
