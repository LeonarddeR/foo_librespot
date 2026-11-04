//! Decoding Spotify's audio to 44.1 kHz stereo PCM, with seeks that land on the exact frame.
//!
//! This uses symphonia directly instead of librespot's `SymphoniaDecoder`, which reports
//! positions only in whole milliseconds; foobar2000 expects a seek to land on the exact frame.

use std::io;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::CoreError;

pub const SAMPLE_RATE: u32 = 44_100;
pub const CHANNELS: usize = 2;

/// How far before a seek target decoding restarts. The Vorbis decoder outputs nothing for the
/// first packet after a reset, so start at least one packet (at most 1024 frames) early.
const PREROLL_FRAMES: u64 = 4096;

/// librespot reads ahead in 64 KiB steps; the stream buffer must be a power of two above 32 KiB.
const STREAM_BUFFER_BYTES: usize = 64 * 1024;

/// Decodes one track to interleaved stereo `f64` frames.
pub struct PcmDecoder {
	format: Box<dyn FormatReader>,
	decoder: Box<dyn Decoder>,
	track_id: u32,
	total_frames: Option<u64>,
	/// The frame the next `read` returns first.
	position: u64,
	/// Decoded samples at and after `position` not yet returned.
	pending: Vec<f64>,
	pending_start: usize,
	samples: Option<SampleBuffer<f64>>,
	ended: bool,
	/// An error that came after some frames of a read; the next read returns it.
	failed: Option<CoreError>,
}

impl PcmDecoder {
	pub fn new(source: Box<dyn MediaSource>, mime_type: Option<&str>) -> Result<Self, CoreError> {
		let stream = MediaSourceStream::new(
			source,
			MediaSourceStreamOptions { buffer_len: STREAM_BUFFER_BYTES },
		);
		let mut hint = Hint::new();
		if let Some(mime_type) = mime_type {
			hint.mime_type(mime_type);
		}
		let format_options = FormatOptions { enable_gapless: true, ..FormatOptions::default() };
		let probed = symphonia::default::get_probe()
			.format(&hint, stream, &format_options, &MetadataOptions::default())
			.map_err(setup_error)?;
		let format = probed.format;
		let track =
			format.default_track().ok_or_else(|| CoreError::Decode("no audio track".to_owned()))?;
		let params = track.codec_params.clone();
		let track_id = track.id;
		let channels = params.channels.map(symphonia::core::audio::Channels::count);
		if params.sample_rate != Some(SAMPLE_RATE) || channels != Some(CHANNELS) {
			return Err(CoreError::Decode(format!(
				"expected {SAMPLE_RATE} Hz stereo, got {} Hz with {} channels",
				params.sample_rate.map_or_else(|| "unknown".to_owned(), |rate| rate.to_string()),
				channels.map_or_else(|| "unknown".to_owned(), |count| count.to_string()),
			)));
		}
		let decoder = symphonia::default::get_codecs()
			.make(&params, &DecoderOptions::default())
			.map_err(setup_error)?;
		Ok(Self {
			format,
			decoder,
			track_id,
			total_frames: params.n_frames,
			position: 0,
			pending: Vec::new(),
			pending_start: 0,
			samples: None,
			ended: false,
			failed: None,
		})
	}

	pub fn total_frames(&self) -> Option<u64> {
		self.total_frames
	}

	/// The frame the next [`read`](Self::read) starts at.
	pub fn position(&self) -> u64 {
		self.position
	}

	/// Replaces `out` with up to `max_frames` interleaved stereo frames. Returns how many frames
	/// it holds; 0 means the track has ended, and stays 0 until the next seek.
	pub fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError> {
		out.clear();
		if let Some(error) = self.failed.take() {
			return Err(error);
		}
		let wanted = max_frames * CHANNELS;
		while out.len() < wanted {
			if self.pending_start < self.pending.len() {
				let take = (wanted - out.len()).min(self.pending.len() - self.pending_start);
				out.extend_from_slice(&self.pending[self.pending_start..self.pending_start + take]);
				self.pending_start += take;
				self.position += (take / CHANNELS) as u64;
			} else if self.ended {
				break;
			} else {
				match self.decode_next() {
					Ok(true) => {}
					Ok(false) => {
						self.ended = true;
						break;
					}
					// The frames already in `out` count as read; the error comes with the next read.
					Err(error) if !out.is_empty() => {
						self.failed = Some(error);
						break;
					}
					Err(error) => return Err(error),
				}
			}
		}
		Ok(out.len() / CHANNELS)
	}

	/// Moves to `frame`, so the next read starts exactly there. At or past the end, the next read
	/// returns 0.
	pub fn seek(&mut self, frame: u64) -> Result<(), CoreError> {
		self.failed = None;
		self.pending.clear();
		self.pending_start = 0;
		self.position = frame;
		if self.total_frames.is_some_and(|total| frame >= total) {
			self.ended = true;
			return Ok(());
		}
		self.ended = false;
		let ts = frame.saturating_sub(PREROLL_FRAMES);
		match self
			.format
			.seek(SeekMode::Accurate, SeekTo::TimeStamp { ts, track_id: self.track_id })
		{
			Ok(_) => {}
			Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
				self.ended = true;
			}
			Err(error) => return Err(stream_error(error)),
		}
		self.decoder.reset();
		Ok(())
	}

	/// Decodes packets until one holds frames at or after `position`, and keeps those frames.
	/// Returns false at the end of the track.
	fn decode_next(&mut self) -> Result<bool, CoreError> {
		loop {
			let packet = match self.format.next_packet() {
				Ok(packet) => packet,
				Err(SymphoniaError::IoError(error))
					if error.kind() == io::ErrorKind::UnexpectedEof =>
				{
					return Ok(false);
				}
				Err(error) => return Err(stream_error(error)),
			};
			if packet.track_id() != self.track_id {
				continue;
			}
			let decoded = match self.decoder.decode(&packet) {
				Ok(decoded) => decoded,
				// A damaged packet is skipped, as librespot does, rather than ending the track.
				Err(SymphoniaError::DecodeError(message)) => {
					log::warn!("skipping a damaged audio packet: {message}");
					continue;
				}
				Err(error) => return Err(stream_error(error)),
			};
			let frames = decoded.frames() as u64;
			let start = packet.ts();
			if frames == 0 || start + frames <= self.position {
				continue;
			}
			let samples = match &mut self.samples {
				Some(samples) if samples.capacity() >= decoded.capacity() * CHANNELS => samples,
				slot => slot.insert(SampleBuffer::new(decoded.capacity() as u64, *decoded.spec())),
			};
			samples.copy_interleaved_ref(decoded);
			// Normally `start == position`; after a seek the packet starts before the target.
			self.position = self.position.max(start);
			let skip = usize::try_from(self.position - start).unwrap_or(usize::MAX) * CHANNELS;
			self.pending.clear();
			self.pending.extend_from_slice(&samples.samples()[skip.min(samples.len())..]);
			self.pending_start = 0;
			return Ok(true);
		}
	}
}

/// Errors while opening a stream: running out of data means the stream is broken.
fn setup_error(error: SymphoniaError) -> CoreError {
	match error {
		SymphoniaError::IoError(error) if error.kind() != io::ErrorKind::UnexpectedEof => {
			CoreError::Io(error)
		}
		other => CoreError::Decode(other.to_string()),
	}
}

/// Errors while reading: I/O errors, such as a stalled download, stay I/O errors.
fn stream_error(error: SymphoniaError) -> CoreError {
	match error {
		SymphoniaError::IoError(error) => CoreError::Io(error),
		other => CoreError::Decode(other.to_string()),
	}
}

#[cfg(test)]
mod tests {
	use std::io::{self, Cursor, Read, Seek, SeekFrom};

	use symphonia::core::io::MediaSource;

	use super::{CHANNELS, PcmDecoder};
	use crate::CoreError;
	use crate::audio::header::{SPOTIFY_OGG_HEADER_END, Subfile};

	const CHIRP: &[u8] = include_bytes!("../../tests/fixtures/chirp.ogg");
	const MONO_22K: &[u8] = include_bytes!("../../tests/fixtures/mono22k.ogg");

	fn open(source: Box<dyn MediaSource>) -> PcmDecoder {
		PcmDecoder::new(source, Some("audio/ogg")).unwrap()
	}

	fn chirp() -> PcmDecoder {
		open(Box::new(Cursor::new(CHIRP)))
	}

	fn read_all(decoder: &mut PcmDecoder) -> Vec<f64> {
		let mut all = Vec::new();
		let mut buffer = Vec::new();
		while decoder.read(&mut buffer, 4096).unwrap() > 0 {
			all.extend_from_slice(&buffer);
		}
		all
	}

	fn assert_matches_from(expected: &[f64], frame: u64, actual: &[f64]) {
		let start = usize::try_from(frame).unwrap() * CHANNELS;
		let expected = &expected[start..start + actual.len()];
		for (index, (want, got)) in expected.iter().zip(actual).enumerate() {
			assert!(
				(want - got).abs() < 1e-4,
				"sample {index} after frame {frame}: {want} vs {got}"
			);
		}
	}

	#[test]
	fn decodes_the_whole_fixture() {
		let mut decoder = chirp();
		let total = decoder.total_frames().unwrap();
		let all = read_all(&mut decoder);
		assert!(total >= 88_200, "{total}");
		assert_eq!(all.len() as u64, total * CHANNELS as u64);
		let peak = |channel: usize| {
			all.iter()
				.skip(channel)
				.step_by(CHANNELS)
				.fold(0.0f64, |peak, sample| peak.max(sample.abs()))
		};
		assert!(peak(0) > 0.2, "left peak {}", peak(0));
		assert!(peak(1) < 1e-3, "right peak {}", peak(1));
	}

	#[test]
	fn reads_at_most_max_frames() {
		let mut decoder = chirp();
		let mut buffer = Vec::new();
		let frames = decoder.read(&mut buffer, 1000).unwrap();
		assert!(frames > 0 && frames <= 1000, "{frames}");
		assert_eq!(buffer.len(), frames * CHANNELS);
	}

	#[test]
	fn seek_is_sample_accurate() {
		let all = read_all(&mut chirp());
		for frame in [0, 1, 4410, 44_100, 66_150] {
			let mut decoder = chirp();
			decoder.seek(frame).unwrap();
			assert_eq!(decoder.position(), frame);
			let mut buffer = Vec::new();
			let frames = decoder.read(&mut buffer, 4096).unwrap();
			assert_eq!(frames, 4096, "after seeking to {frame}");
			assert_matches_from(&all, frame, &buffer);
		}
	}

	#[test]
	fn seek_to_the_end_reads_zero() {
		let mut decoder = chirp();
		let total = decoder.total_frames().unwrap();
		decoder.seek(total).unwrap();
		assert_eq!(decoder.read(&mut Vec::new(), 4096).unwrap(), 0);
	}

	#[test]
	fn seek_past_the_end_reads_zero() {
		let mut decoder = chirp();
		let total = decoder.total_frames().unwrap();
		decoder.seek(total + 10_000).unwrap();
		assert_eq!(decoder.read(&mut Vec::new(), 4096).unwrap(), 0);
	}

	#[test]
	fn backward_seek_after_the_end_works() {
		let all = read_all(&mut chirp());
		let mut decoder = chirp();
		read_all(&mut decoder);
		decoder.seek(44_100).unwrap();
		let mut buffer = Vec::new();
		assert_eq!(decoder.read(&mut buffer, 4096).unwrap(), 4096);
		assert_matches_from(&all, 44_100, &buffer);
	}

	#[test]
	fn skips_the_spotify_header() {
		let plain = read_all(&mut chirp());
		let mut file = vec![0u8; usize::try_from(SPOTIFY_OGG_HEADER_END).unwrap()];
		file.extend_from_slice(CHIRP);
		let length = file.len() as u64;
		let subfile = Subfile::new(Cursor::new(file), SPOTIFY_OGG_HEADER_END, length).unwrap();
		assert_eq!(read_all(&mut open(Box::new(subfile))), plain);
	}

	#[test]
	fn rejects_anything_but_44100_stereo() {
		let result = PcmDecoder::new(Box::new(Cursor::new(MONO_22K)), Some("audio/ogg"));
		assert!(
			matches!(&result, Err(CoreError::Decode(message)) if message.contains("44100")),
			"{:?}",
			result.err()
		);
	}

	/// Times out once `limit` bytes have been read, like librespot's streaming file when the
	/// network stalls.
	struct StallingReader {
		inner: Cursor<&'static [u8]>,
		remaining: usize,
		/// A seekable source makes the Ogg reader read the last page first, to learn the length.
		seekable: bool,
	}

	impl Read for StallingReader {
		fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
			if self.remaining == 0 {
				return Err(io::Error::new(io::ErrorKind::TimedOut, "download stalled"));
			}
			let allowed = buf.len().min(self.remaining);
			let read = self.inner.read(&mut buf[..allowed])?;
			self.remaining -= read;
			Ok(read)
		}
	}

	impl Seek for StallingReader {
		fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
			self.inner.seek(pos)
		}
	}

	impl MediaSource for StallingReader {
		fn is_seekable(&self) -> bool {
			self.seekable
		}

		fn byte_len(&self) -> Option<u64> {
			self.seekable.then_some(CHIRP.len() as u64)
		}
	}

	#[test]
	fn read_error_surfaces_as_io_error() {
		let source = StallingReader { inner: Cursor::new(CHIRP), remaining: 3000, seekable: true };
		let result =
			PcmDecoder::new(Box::new(source), Some("audio/ogg")).and_then(|mut decoder| {
				let mut buffer = Vec::new();
				while decoder.read(&mut buffer, 4096)? > 0 {}
				Ok(())
			});
		assert!(
			matches!(&result, Err(CoreError::Io(error)) if error.kind() == io::ErrorKind::TimedOut),
			"{result:?}"
		);
	}

	#[test]
	fn frames_decoded_before_a_read_error_are_returned_first() {
		let source = StallingReader {
			inner: Cursor::new(CHIRP),
			remaining: CHIRP.len() * 9 / 10,
			seekable: false,
		};
		let mut decoder = PcmDecoder::new(Box::new(source), Some("audio/ogg")).unwrap();
		let mut buffer = Vec::new();
		// Asks for far more than the reader gives before it stalls.
		let frames = decoder.read(&mut buffer, 10_000_000).unwrap();
		assert!(frames > 0);
		assert_eq!(buffer.len(), frames * CHANNELS);
		assert_eq!(decoder.position(), frames as u64);
		let result = decoder.read(&mut buffer, 4096);
		assert!(
			matches!(&result, Err(CoreError::Io(error)) if error.kind() == io::ErrorKind::TimedOut),
			"{result:?}"
		);
	}

	#[test]
	fn end_of_stream_is_zero_repeatedly() {
		let mut decoder = chirp();
		read_all(&mut decoder);
		let mut buffer = Vec::new();
		assert_eq!(decoder.read(&mut buffer, 4096).unwrap(), 0);
		assert_eq!(decoder.read(&mut buffer, 4096).unwrap(), 0);
		assert!(buffer.is_empty());
	}
}
