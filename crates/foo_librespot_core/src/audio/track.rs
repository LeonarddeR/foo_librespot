//! Opening a Spotify track for decoding: picking the file, fetching it and decrypting it.

use librespot_audio::{AudioDecrypt, AudioFile, StreamLoaderController};
use librespot_core::Session;
use librespot_core::audio_key::AudioKeyError;
use librespot_core::error::ErrorKind;
use librespot_metadata::MetadataError;
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem};
use librespot_metadata::availability::AudioItemAvailability;
use librespot_metadata::track::Tracks;

use super::decoder::PcmDecoder;
use super::format::{Bitrate, bytes_per_second, header_offset, mime_type, pick_file};
use super::header::{NormalisationData, Subfile};
use super::keys::AudioKeys;
use crate::link::ItemId;
use crate::metadata::EXPLICIT_FILTERED;
use crate::replaygain::ReplayGain;
use crate::resolve::retry_rate_limited;
use crate::{CoreError, FileId, SpotifyUri};

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
	file_id: FileId,
	key_origin: KeyOrigin,
}

/// Finds the file to play for `item` and starts downloading it. The key is taken from `keys` when
/// it is there; otherwise Spotify is asked for it and it is added to `keys`. This only awaits; the
/// blocking reads happen in [`FetchedTrack::open`], which must not run on a tokio worker.
pub async fn fetch_track(
	session: &Session,
	item: ItemId,
	bitrate: Bitrate,
	keys: &AudioKeys,
) -> Result<FetchedTrack, CoreError> {
	let id = item.id();
	// Waits out librespot's rate limit, as imports do: right after a big import, pressing Play
	// would otherwise fail at once. An abort (Stop) drops the wait.
	let uri = item.uri();
	let item = retry_rate_limited(|| AudioItem::get_file(session, uri.clone()))
		.await
		.map_err(audio_item_error)?;
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
	let file =
		retry_rate_limited(|| AudioFile::open(session, file_id, bytes_per_second(format))).await?;
	let controller = file.get_stream_loader_controller()?;
	if let Some(key) = keys.get(&file_id) {
		tracing::debug!("using the remembered key for {}", id.to_base62());
		let file = AudioDecrypt::new(Some(key), file);
		return Ok(FetchedTrack {
			file,
			format,
			controller,
			file_id,
			key_origin: KeyOrigin::Remembered,
		});
	}
	// The key is always asked for with the original track id, even for an alternative.
	let (key, key_origin) = match session.audio_key().request(id, file_id).await {
		Ok(key) => {
			keys.insert(file_id, key);
			(Some(key), KeyOrigin::Requested)
		}
		Err(error) => match on_key_failure(format, error.kind) {
			KeyFailure::PlayUnencrypted => {
				tracing::warn!(
					"no audio key for {}, trying without decryption: {error}",
					id.to_base62()
				);
				(None, KeyOrigin::Missing)
			}
			KeyFailure::Fail { reset_session } => {
				controller.close();
				if reset_session {
					// The next call connects again instead of reusing the dead connection.
					session.shutdown();
				}
				return Err(key_error(error));
			}
		},
	};
	Ok(FetchedTrack { file: AudioDecrypt::new(key, file), format, controller, file_id, key_origin })
}

/// Where the key that decrypts a fetched track came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOrigin {
	/// Remembered from an earlier play; Spotify wasn't asked.
	Remembered,
	/// Asked for from Spotify just now.
	Requested,
	/// There is no key; the file is played as it is.
	Missing,
}

/// Whether opening a track failed because its remembered key is wrong. A wrong key turns the
/// audio into noise, so the decoder can't read the file's start; a failed download is an I/O
/// error and says nothing about the key.
pub fn wrong_remembered_key(origin: KeyOrigin, error: &CoreError) -> bool {
	origin == KeyOrigin::Remembered && matches!(error, CoreError::Decode(_))
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
		match retry_rate_limited(|| AudioItem::get_file(session, uri.clone())).await {
			Ok(item) if playability(&item.availability, &item.files, None) == Playability::Play => {
				return Some(item);
			}
			Ok(_) => {}
			Err(error) => tracing::debug!("alternative track unavailable: {error}"),
		}
	}
	None
}

/// librespot's error for an audio key, with Spotify's refusal told apart from other failures.
fn key_error(error: librespot_core::Error) -> CoreError {
	match error.error.downcast_ref::<AudioKeyError>() {
		Some(AudioKeyError::AesKey(code)) => CoreError::KeyRefused { code: *code },
		_ => CoreError::Spotify(error),
	}
}

/// librespot's error for an audio item, with its refusal of an explicit item for an account set
/// not to play explicit content put the way an import puts it.
fn audio_item_error(error: librespot_core::Error) -> CoreError {
	if matches!(
		error.error.downcast_ref::<MetadataError>(),
		Some(MetadataError::ExplicitContentFiltered)
	) {
		CoreError::Unavailable(EXPLICIT_FILTERED.to_owned())
	} else {
		CoreError::Spotify(error)
	}
}

impl FetchedTrack {
	/// The audio file being downloaded.
	pub fn file_id(&self) -> FileId {
		self.file_id
	}

	pub fn key_origin(&self) -> KeyOrigin {
		self.key_origin
	}

	/// Reads Spotify's header and opens the decoder. Blocks until the first data arrives.
	pub fn open(self) -> Result<SpotifyTrack, CoreError> {
		let Self { mut file, format, controller, .. } = self;
		let normalisation = if AudioFiles::is_ogg_vorbis(format) {
			match NormalisationData::parse(&mut file) {
				Ok(data) => Some(data),
				Err(error) => {
					tracing::warn!("no loudness values in the Spotify header: {error}");
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

	use librespot_metadata::MetadataError;
	use librespot_metadata::audio::{AudioFileFormat, AudioFiles};
	use librespot_metadata::availability::UnavailabilityReason;
	use librespot_metadata::track::Tracks;

	use librespot_core::audio_key::AudioKeyError;
	use librespot_core::error::ErrorKind;

	use super::{
		KeyFailure, KeyOrigin, Playability, audio_item_error, key_error, on_key_failure,
		playability, wrong_remembered_key,
	};
	use crate::metadata::EXPLICIT_FILTERED;
	use crate::{CoreError, FileId, SpotifyId, SpotifyUri};

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

	#[test]
	fn the_explicit_filter_refusal_becomes_the_plain_reason() {
		let error = audio_item_error(librespot_core::Error::unavailable(
			MetadataError::ExplicitContentFiltered,
		));
		assert!(
			matches!(&error, CoreError::Unavailable(reason) if reason == EXPLICIT_FILTERED),
			"{error:?}"
		);
	}

	#[test]
	fn other_audio_item_errors_stay_spotify_errors() {
		let error =
			audio_item_error(librespot_core::Error::unavailable(MetadataError::NonPlayable));
		assert!(matches!(&error, CoreError::Spotify(_)), "{error:?}");
	}

	#[test]
	fn a_refused_key_says_so_with_spotifys_code() {
		let error = key_error(AudioKeyError::AesKey(1).into());
		assert!(matches!(error, CoreError::KeyRefused { code: 1 }), "{error:?}");
		assert_eq!(error.to_string(), "Spotify refused the key that decrypts this track (code 1)");
	}

	#[test]
	fn other_key_errors_stay_spotify_errors() {
		let error = key_error(AudioKeyError::Timeout.into());
		assert!(matches!(&error, CoreError::Spotify(_)), "{error:?}");
	}

	fn undecodable() -> CoreError {
		CoreError::Decode("no suitable format reader found".to_owned())
	}

	#[test]
	fn a_remembered_key_that_leaves_the_audio_undecodable_is_wrong() {
		assert!(wrong_remembered_key(KeyOrigin::Remembered, &undecodable()));
	}

	#[test]
	fn a_key_spotify_just_sent_is_never_blamed() {
		assert!(!wrong_remembered_key(KeyOrigin::Requested, &undecodable()));
		assert!(!wrong_remembered_key(KeyOrigin::Missing, &undecodable()));
	}

	#[test]
	fn a_failed_download_is_not_the_remembered_keys_fault() {
		let error = CoreError::Io(std::io::Error::from(std::io::ErrorKind::TimedOut));
		assert!(!wrong_remembered_key(KeyOrigin::Remembered, &error));
	}
}
