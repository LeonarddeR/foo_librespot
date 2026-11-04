//! Opening a Spotify track for decoding: picking the file, fetching it and decrypting it.

use librespot_audio::{AudioDecrypt, AudioFile, StreamLoaderController};
use librespot_core::Session;
use librespot_core::error::ErrorKind;
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem};
use librespot_metadata::availability::AudioItemAvailability;
use librespot_metadata::track::Tracks;

use super::decoder::PcmDecoder;
use super::format::{Bitrate, bytes_per_second, header_offset, mime_type, pick_file};
use super::header::{NormalisationData, ReplayGain, Subfile};
use crate::{CoreError, SpotifyId, SpotifyUri};

/// Whether a track can be played as it is, only through one of its alternatives, or not at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Playability {
	Play,
	/// Spotify lists other releases of the same recording to play instead, in this order.
	TryAlternatives(Vec<SpotifyUri>),
	Unavailable(String),
}

/// librespot's decision in `playback/src/player.rs` (`find_available_alternative`).
pub fn playability(
	availability: &AudioItemAvailability,
	files: &AudioFiles,
	alternatives: Option<&Tracks>,
) -> Playability {
	match (availability, alternatives) {
		(Err(reason), _) => Playability::Unavailable(reason.to_string()),
		(Ok(()), _) if !files.0.is_empty() => Playability::Play,
		(Ok(()), Some(alternatives)) if !alternatives.0.is_empty() => {
			Playability::TryAlternatives(alternatives.0.clone())
		}
		(Ok(()), _) => Playability::Unavailable("Spotify has no audio for this track".to_owned()),
	}
}

/// A track whose file is being downloaded, not yet opened for decoding.
pub struct FetchedTrack {
	file: AudioDecrypt<AudioFile>,
	format: AudioFileFormat,
	controller: StreamLoaderController,
}

/// Finds the file to play for `id` and starts downloading it. This only awaits; the blocking
/// reads happen in [`FetchedTrack::open`], which must not run on a tokio worker.
pub async fn fetch_track(
	session: &Session,
	id: SpotifyId,
	bitrate: Bitrate,
) -> Result<FetchedTrack, CoreError> {
	let item = AudioItem::get_file(session, SpotifyUri::Track { id }).await?;
	let item = match playability(&item.availability, &item.files, item.alternatives.as_ref()) {
		Playability::Play => item,
		Playability::TryAlternatives(alternatives) => {
			find_alternative(session, alternatives).await.ok_or_else(|| {
				CoreError::Unavailable("no version of this track can be played".to_owned())
			})?
		}
		Playability::Unavailable(reason) => return Err(CoreError::Unavailable(reason)),
	};
	let (format, file_id) = pick_file(&item.files, bitrate).ok_or_else(|| {
		CoreError::Unavailable("Spotify has no audio file foo_librespot can play".to_owned())
	})?;
	let file = AudioFile::open(session, file_id, bytes_per_second(format)).await?;
	let controller = file.get_stream_loader_controller()?;
	// The key is always asked for with the original track id, even for an alternative.
	let key = match session.audio_key().request(id, file_id).await {
		Ok(key) => Some(key),
		Err(error) => match on_key_failure(format, error.kind) {
			KeyFailure::PlayUnencrypted => {
				log::warn!(
					"no audio key for {}, trying without decryption: {error}",
					id.to_base62()
				);
				None
			}
			KeyFailure::Fail { reset_session } => {
				controller.close();
				if reset_session {
					// The next call connects again instead of reusing the dead connection.
					session.shutdown();
				}
				return Err(CoreError::Spotify(error));
			}
		},
	};
	Ok(FetchedTrack { file: AudioDecrypt::new(key, file), format, controller })
}

/// What to do when Spotify doesn't hand over a file's audio key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFailure {
	/// Some files aren't encrypted; if this one is, decoding fails on its own.
	PlayUnencrypted,
	/// Give up. `reset_session` when the key request timed out: the access point connection is
	/// most likely dead, though librespot only notices when its keep-alive fails minutes later.
	Fail { reset_session: bool },
}

/// Spotify's Ogg Vorbis files are always encrypted, so without the key there is nothing to play,
/// and decoding the encrypted bytes would only report a misleading decode error. librespot's
/// player tries without a key anyway; that only makes sense for its other formats.
pub fn on_key_failure(format: AudioFileFormat, kind: ErrorKind) -> KeyFailure {
	if !AudioFiles::is_ogg_vorbis(format) {
		return KeyFailure::PlayUnencrypted;
	}
	KeyFailure::Fail {
		reset_session: matches!(kind, ErrorKind::Aborted | ErrorKind::DeadlineExceeded),
	}
}

async fn find_alternative(session: &Session, alternatives: Vec<SpotifyUri>) -> Option<AudioItem> {
	for uri in alternatives {
		match AudioItem::get_file(session, uri).await {
			Ok(item) if playability(&item.availability, &item.files, None) == Playability::Play => {
				return Some(item);
			}
			Ok(_) => {}
			Err(error) => log::debug!("alternative track unavailable: {error}"),
		}
	}
	None
}

impl FetchedTrack {
	/// Reads Spotify's header and opens the decoder. Blocks until the first data arrives.
	pub fn open(self) -> Result<SpotifyTrack, CoreError> {
		let Self { mut file, format, controller } = self;
		let normalisation = if AudioFiles::is_ogg_vorbis(format) {
			match NormalisationData::parse(&mut file) {
				Ok(data) => Some(data),
				Err(error) => {
					log::warn!("no loudness values in the Spotify header: {error}");
					None
				}
			}
		} else {
			None
		};
		let length = controller.len() as u64;
		let source = Subfile::new(file, header_offset(format), length)?;
		let decoder = PcmDecoder::new(Box::new(source), mime_type(format))?;
		// Read ahead steadily now that playback can start.
		controller.set_stream_mode();
		Ok(SpotifyTrack { decoder, controller, normalisation, format })
	}
}

/// A Spotify track opened for decoding.
pub struct SpotifyTrack {
	decoder: PcmDecoder,
	controller: StreamLoaderController,
	normalisation: Option<NormalisationData>,
	format: AudioFileFormat,
}

impl SpotifyTrack {
	/// See [`PcmDecoder::read`].
	pub fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError> {
		self.decoder.read(out, max_frames)
	}

	/// See [`PcmDecoder::seek`]. librespot fetches the needed range at once while seeking.
	pub fn seek(&mut self, frame: u64) -> Result<(), CoreError> {
		self.controller.set_random_access_mode();
		let result = self.decoder.seek(frame);
		self.controller.set_stream_mode();
		result
	}

	pub fn total_frames(&self) -> Option<u64> {
		self.decoder.total_frames()
	}

	pub fn replay_gain(&self) -> Option<ReplayGain> {
		self.normalisation.map(ReplayGain::from)
	}

	pub fn format(&self) -> AudioFileFormat {
		self.format
	}
}

impl Drop for SpotifyTrack {
	fn drop(&mut self) {
		// Stops the download.
		self.controller.close();
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use librespot_metadata::audio::{AudioFileFormat, AudioFiles};
	use librespot_metadata::availability::UnavailabilityReason;
	use librespot_metadata::track::Tracks;

	use librespot_core::error::ErrorKind;

	use super::{KeyFailure, Playability, on_key_failure, playability};
	use crate::{FileId, SpotifyId, SpotifyUri};

	fn one_file() -> AudioFiles {
		AudioFiles(HashMap::from([(AudioFileFormat::OGG_VORBIS_320, FileId::from_raw(&[1; 20]))]))
	}

	fn alternatives() -> Tracks {
		Tracks(vec![
			SpotifyUri::Track { id: SpotifyId::from_raw(&[2; 16]).unwrap() },
			SpotifyUri::Track { id: SpotifyId::from_raw(&[3; 16]).unwrap() },
		])
	}

	#[test]
	fn vorbis_without_a_key_fails() {
		assert_eq!(
			on_key_failure(AudioFileFormat::OGG_VORBIS_320, ErrorKind::PermissionDenied),
			KeyFailure::Fail { reset_session: false }
		);
	}

	#[test]
	fn key_timeout_resets_the_session() {
		for kind in [ErrorKind::Aborted, ErrorKind::DeadlineExceeded] {
			assert_eq!(
				on_key_failure(AudioFileFormat::OGG_VORBIS_160, kind),
				KeyFailure::Fail { reset_session: true }
			);
		}
	}

	#[test]
	fn mp3_without_a_key_continues_unencrypted() {
		assert_eq!(
			on_key_failure(AudioFileFormat::MP3_320, ErrorKind::Aborted),
			KeyFailure::PlayUnencrypted
		);
	}

	#[test]
	fn available_item_with_files_plays() {
		assert_eq!(playability(&Ok(()), &one_file(), Some(&alternatives())), Playability::Play);
	}

	#[test]
	fn available_item_without_files_tries_alternatives() {
		assert_eq!(
			playability(&Ok(()), &AudioFiles::default(), Some(&alternatives())),
			Playability::TryAlternatives(alternatives().0)
		);
	}

	#[test]
	fn unavailable_item_reports_the_reason() {
		let result = playability(&Err(UnavailabilityReason::Embargo), &one_file(), None);
		assert!(
			matches!(&result, Playability::Unavailable(message) if message.contains("available date is in the future")),
			"{result:?}"
		);
	}

	#[test]
	fn item_without_files_or_alternatives_is_unavailable() {
		assert!(matches!(
			playability(&Ok(()), &AudioFiles::default(), None),
			Playability::Unavailable(_)
		));
		assert!(matches!(
			playability(&Ok(()), &AudioFiles::default(), Some(&Tracks::default())),
			Playability::Unavailable(_)
		));
	}
}
