//! ReplayGain values for Spotify tracks, in the form foobar2000 stores them.

use protobuf::CodedInputStream;
use protobuf::rt::WireType;

use crate::audio::header::NormalisationData;

/// The loudness ReplayGain 2 brings every track to, in LUFS.
pub const REFERENCE_LUFS: f32 = -18.0;

/// Added to the gains in Spotify's file header to get ReplayGain 2 gains. Measured on three
/// Keith & Kristyn Getty tracks with ffmpeg's EBU R128 filter: the header gain was 1.2 to 1.8 dB
/// below ReplayGain 2 (-18 LUFS), so the header values are only an approximation, used when
/// Spotify's metadata has no loudness for a track.
pub const HEADER_OFFSET_DB: f64 = 1.5;

/// The ReplayGain fields foobar2000 stores. Peaks are linear, 1.0 being full scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayGain {
	pub track_gain_db: f32,
	pub track_peak: f32,
	pub album_gain_db: f32,
	pub album_peak: f32,
}

impl From<NormalisationData> for ReplayGain {
	#[allow(clippy::cast_possible_truncation, reason = "the values came from f32")]
	fn from(data: NormalisationData) -> Self {
		Self {
			track_gain_db: (data.track_gain_db + HEADER_OFFSET_DB) as f32,
			track_peak: data.track_peak as f32,
			album_gain_db: (data.album_gain_db + HEADER_OFFSET_DB) as f32,
			album_peak: data.album_peak as f32,
		}
	}
}

/// One of Spotify's loudness measurements, from its `AUDIO_FILES` metadata: integrated loudness
/// in LUFS (EBU R128) and true peak in dBTP. Checked against ffmpeg's EBU R128 filter on three
/// tracks, they agree to 0.05 dB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
	pub loudness_db: f32,
	pub true_peak_db: f32,
}

impl Loudness {
	/// Protobuf 3 sends an unset message as zeros, which no real track measures.
	fn is_measured(self) -> bool {
		self.loudness_db.is_finite()
			&& self.true_peak_db.is_finite()
			&& (self.loudness_db != 0.0 || self.true_peak_db != 0.0)
	}
}

/// ReplayGain 2 values from Spotify's measurements. Without an album measurement the album
/// values repeat the track's, which is what foobar2000 falls back to anyway.
pub fn from_loudness(track: Loudness, album: Option<Loudness>) -> ReplayGain {
	let album = album.unwrap_or(track);
	ReplayGain {
		track_gain_db: REFERENCE_LUFS - track.loudness_db,
		track_peak: linear(track.true_peak_db),
		album_gain_db: REFERENCE_LUFS - album.loudness_db,
		album_peak: linear(album.true_peak_db),
	}
}

fn linear(db: f32) -> f32 {
	10f32.powf(db / 20.0)
}

/// The ReplayGain values in an `AudioFilesExtensionResponse` from Spotify's extended metadata,
/// or `None` when it has no usable track loudness. librespot doesn't compile that message, and
/// only two of its fields matter, so they are read straight from the protobuf wire format:
/// field 2 is the track's `NormalizationParams`, field 3 the album's, each holding
/// `loudness_db` (1) and `true_peak_db` (2) as floats.
pub fn parse_audio_files(bytes: &[u8]) -> Option<ReplayGain> {
	let (track, album) = read_extension(bytes)?;
	let track = track.filter(|loudness| loudness.is_measured())?;
	Some(from_loudness(track, album.filter(|loudness| loudness.is_measured())))
}

fn read_extension(bytes: &[u8]) -> Option<(Option<Loudness>, Option<Loudness>)> {
	let mut input = CodedInputStream::from_bytes(bytes);
	let (mut track, mut album) = (None, None);
	while let Some(tag) = input.read_raw_tag_or_eof().ok()? {
		match (tag >> 3, tag & 7) {
			(2, 2) => track = Some(read_loudness(&input.read_bytes().ok()?)?),
			(3, 2) => album = Some(read_loudness(&input.read_bytes().ok()?)?),
			(_, wire) => input.skip_field(WireType::new(wire)?).ok()?,
		}
	}
	Some((track, album))
}

fn read_loudness(bytes: &[u8]) -> Option<Loudness> {
	let mut input = CodedInputStream::from_bytes(bytes);
	let mut loudness = Loudness { loudness_db: 0.0, true_peak_db: 0.0 };
	while let Some(tag) = input.read_raw_tag_or_eof().ok()? {
		match (tag >> 3, tag & 7) {
			(1, 5) => loudness.loudness_db = input.read_float().ok()?,
			(2, 5) => loudness.true_peak_db = input.read_float().ok()?,
			(_, wire) => input.skip_field(WireType::new(wire)?).ok()?,
		}
	}
	Some(loudness)
}

#[cfg(test)]
mod tests {
	use protobuf::CodedOutputStream;

	use super::{Loudness, ReplayGain, from_loudness, parse_audio_files};
	use crate::audio::header::NormalisationData;

	fn params(loudness: f32, peak: f32) -> Vec<u8> {
		let mut bytes = Vec::new();
		let mut out = CodedOutputStream::vec(&mut bytes);
		out.write_float(1, loudness).unwrap();
		out.write_float(2, peak).unwrap();
		out.flush().unwrap();
		drop(out);
		bytes
	}

	/// An `AudioFilesExtensionResponse` as Spotify sends it: files, track and album loudness, id.
	fn extension(track: Option<(f32, f32)>, album: Option<(f32, f32)>) -> Vec<u8> {
		let mut bytes = Vec::new();
		let mut out = CodedOutputStream::vec(&mut bytes);
		out.write_bytes(1, &[0x0a, 0x00]).unwrap();
		if let Some((loudness, peak)) = track {
			out.write_bytes(2, &params(loudness, peak)).unwrap();
		}
		if let Some((loudness, peak)) = album {
			out.write_bytes(3, &params(loudness, peak)).unwrap();
		}
		out.write_bytes(4, b"audio id").unwrap();
		out.flush().unwrap();
		drop(out);
		bytes
	}

	fn close(actual: f32, expected: f32) -> bool {
		(actual - expected).abs() < 1e-4
	}

	#[test]
	fn gain_is_minus_18_minus_loudness_and_peak_is_linear() {
		let gain = from_loudness(
			Loudness { loudness_db: -11.38, true_peak_db: 0.109 },
			Some(Loudness { loudness_db: -9.49, true_peak_db: 0.61 }),
		);
		assert!(close(gain.track_gain_db, -6.62), "{gain:?}");
		assert!(close(gain.track_peak, 1.012_629), "{gain:?}");
		assert!(close(gain.album_gain_db, -8.51), "{gain:?}");
		assert!(close(gain.album_peak, 1.072_76), "{gain:?}");
	}

	#[test]
	fn missing_album_loudness_uses_the_track() {
		let gain = from_loudness(Loudness { loudness_db: -12.3, true_peak_db: -0.38 }, None);
		assert_eq!(gain.album_gain_db, gain.track_gain_db);
		assert_eq!(gain.album_peak, gain.track_peak);
	}

	#[test]
	fn parses_the_audio_files_extension() {
		let gain =
			parse_audio_files(&extension(Some((-11.38, 0.109)), Some((-9.49, 0.61)))).unwrap();
		assert!(close(gain.track_gain_db, -6.62) && close(gain.album_gain_db, -8.51), "{gain:?}");
	}

	#[test]
	fn extension_without_track_loudness_has_no_replay_gain() {
		assert_eq!(parse_audio_files(&extension(None, Some((-9.49, 0.61)))), None);
	}

	#[test]
	fn zero_or_non_finite_loudness_counts_as_missing() {
		assert_eq!(parse_audio_files(&extension(Some((0.0, 0.0)), None)), None);
		assert_eq!(parse_audio_files(&extension(Some((f32::NAN, 0.0)), None)), None);
		assert_eq!(parse_audio_files(&extension(Some((-11.0, f32::INFINITY)), None)), None);
	}

	#[test]
	fn garbage_has_no_replay_gain() {
		assert_eq!(parse_audio_files(&[0xff, 0xff, 0xff, 0xff]), None);
	}

	#[test]
	fn replaygain_adds_the_calibrated_offset_to_gains_and_keeps_peaks() {
		let data = NormalisationData {
			track_gain_db: -6.5,
			track_peak: 0.98,
			album_gain_db: -7.25,
			album_peak: 1.0,
		};
		assert_eq!(
			ReplayGain::from(data),
			ReplayGain {
				track_gain_db: -5.0,
				track_peak: 0.98,
				album_gain_db: -5.75,
				album_peak: 1.0
			}
		);
	}
}
