//! The custom header Spotify puts in front of its Ogg Vorbis files.

use std::io::{self, Read, Seek, SeekFrom};

use symphonia::core::io::MediaSource;

/// Spotify inserts a custom Ogg packet at the start of its Ogg Vorbis files, holding the values
/// other files keep in Vorbis comments. Decoders balk at it, so decoding starts after it.
pub const SPOTIFY_OGG_HEADER_END: u64 = 0xa7;

/// Where the loudness values sit in Spotify's header.
const NORMALISATION_OFFSET: u64 = 144;

/// What ReplayGain gain foobar2000 gets for Spotify's track gain of 0 dB. Calibrated against
/// foobar2000's own ReplayGain scanner in the phase 2 check.
pub const REPLAYGAIN_OFFSET_DB: f64 = 0.0;

/// Shows a stream as if it started at `offset`, hiding Spotify's header from the decoder.
/// Same behaviour as librespot's private `Subfile` in `playback/src/player.rs`.
pub struct Subfile<T> {
	stream: T,
	offset: u64,
	length: u64,
}

impl<T: Read + Seek> Subfile<T> {
	/// `length` is the length of the whole stream, header included.
	pub fn new(mut stream: T, offset: u64, length: u64) -> io::Result<Self> {
		stream.seek(SeekFrom::Start(offset))?;
		Ok(Self { stream, offset, length })
	}
}

impl<T: Read + Seek> Read for Subfile<T> {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		self.stream.read(buf)
	}
}

impl<T: Read + Seek> Seek for Subfile<T> {
	fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
		let pos = match pos {
			SeekFrom::Start(offset) => SeekFrom::Start(offset + self.offset),
			SeekFrom::End(offset) => {
				let target = i128::from(self.length) + i128::from(offset);
				if target < i128::from(self.offset) {
					return Err(io::Error::new(
						io::ErrorKind::InvalidInput,
						"seek before the start",
					));
				}
				pos
			}
			SeekFrom::Current(_) => pos,
		};
		let position = self.stream.seek(pos)?;
		position
			.checked_sub(self.offset)
			.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))
	}
}

impl<T: Read + Seek + Send + Sync> MediaSource for Subfile<T> {
	fn is_seekable(&self) -> bool {
		true
	}

	fn byte_len(&self) -> Option<u64> {
		Some(self.length.saturating_sub(self.offset))
	}
}

/// Spotify's loudness values for a track, from its header.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalisationData {
	pub track_gain_db: f64,
	pub track_peak: f64,
	pub album_gain_db: f64,
	pub album_peak: f64,
}

impl NormalisationData {
	/// Reads the four little-endian `f32` values at offset 144 of a decrypted Spotify Ogg file.
	pub fn parse<T: Read + Seek>(mut file: T) -> io::Result<Self> {
		file.seek(SeekFrom::Start(NORMALISATION_OFFSET))?;
		let mut buf = [0u8; 16];
		file.read_exact(&mut buf)?;
		let value = |index: usize| {
			let start = index * 4;
			f64::from(f32::from_le_bytes([
				buf[start],
				buf[start + 1],
				buf[start + 2],
				buf[start + 3],
			]))
		};
		Ok(Self {
			track_gain_db: value(0),
			track_peak: value(1),
			album_gain_db: value(2),
			album_peak: value(3),
		})
	}
}

/// The ReplayGain fields foobar2000 stores.
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
			track_gain_db: (data.track_gain_db + REPLAYGAIN_OFFSET_DB) as f32,
			track_peak: data.track_peak as f32,
			album_gain_db: (data.album_gain_db + REPLAYGAIN_OFFSET_DB) as f32,
			album_peak: data.album_peak as f32,
		}
	}
}

#[cfg(test)]
mod tests {
	use std::io::{Cursor, Read, Seek, SeekFrom};

	use super::{NormalisationData, ReplayGain, SPOTIFY_OGG_HEADER_END, Subfile};

	#[test]
	fn normalisation_is_read_from_offset_144() {
		let mut file = vec![0u8; 200];
		for (index, value) in [-6.5f32, 0.98, -7.25, 1.0].into_iter().enumerate() {
			let start = 144 + index * 4;
			file[start..start + 4].copy_from_slice(&value.to_le_bytes());
		}
		let data = NormalisationData::parse(Cursor::new(file)).unwrap();
		assert_eq!(
			data,
			NormalisationData {
				track_gain_db: -6.5,
				track_peak: f64::from(0.98f32),
				album_gain_db: -7.25,
				album_peak: 1.0,
			}
		);
	}

	#[test]
	fn short_file_is_an_error_not_a_panic() {
		let error = NormalisationData::parse(Cursor::new(vec![0u8; 100])).unwrap_err();
		assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
	}

	#[test]
	fn subfile_hides_the_header() {
		let bytes: Vec<u8> = (0..=255).collect();
		let mut subfile = Subfile::new(Cursor::new(bytes), SPOTIFY_OGG_HEADER_END, 256).unwrap();
		let mut byte = [0u8; 1];
		subfile.read_exact(&mut byte).unwrap();
		assert_eq!(byte[0], 0xa7);
		assert_eq!(subfile.seek(SeekFrom::Start(0)).unwrap(), 0);
		subfile.seek(SeekFrom::Start(10)).unwrap();
		subfile.read_exact(&mut byte).unwrap();
		assert_eq!(byte[0], 0xb1);
		assert_eq!(subfile.seek(SeekFrom::End(0)).unwrap(), 256 - 0xa7);
	}

	#[test]
	fn replaygain_copies_gains_and_peaks() {
		let data = NormalisationData {
			track_gain_db: -6.5,
			track_peak: 0.98,
			album_gain_db: -7.25,
			album_peak: 1.0,
		};
		assert_eq!(
			ReplayGain::from(data),
			ReplayGain {
				track_gain_db: -6.5,
				track_peak: 0.98,
				album_gain_db: -7.25,
				album_peak: 1.0
			}
		);
	}
}
