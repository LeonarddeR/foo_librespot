//! Turning a track, album, playlist, episode or show link into the tracks and episodes to add
//! to a foobar2000 playlist.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use futures_util::stream::{self, StreamExt};
use librespot_core::Session;
use librespot_core::date::Date;
use librespot_metadata::{Album, Episode, Metadata, Playlist, Show, Track};
use librespot_protocol::extended_metadata::{
	BatchedEntityRequest, BatchedExtensionResponse, EntityRequest, ExtensionQuery,
};
use librespot_protocol::extension_kind::ExtensionKind;
use librespot_protocol::metadata::Track as TrackMessage;
use protobuf::{EnumOrUnknown, Message};

use crate::link::{ItemId, Link, LinkKind};
use crate::metadata::{TrackInfo, availability, episode_info, track_info};
use crate::replaygain::parse_audio_files;
use crate::{CoreError, SpotifyId, SpotifyUri};

/// How many track lookups run at once while resolving an album or playlist.
pub const FETCH_CONCURRENCY: usize = 8;

/// Why an entry of an album or playlist was left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
	LocalFile,
	Unsupported(String),
	Unavailable(String),
}

impl fmt::Display for SkipReason {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::LocalFile => f.write_str("local files in Spotify playlists can't be played"),
			Self::Unsupported(kind) => write!(f, "Spotify {kind} items can't be played"),
			Self::Unavailable(reason) => write!(f, "not available: {reason}"),
		}
	}
}

/// An entry that was left out, with its Spotify URI so the user can find it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
	pub uri: String,
	pub reason: SkipReason,
}

/// The tracks a link stands for, in order.
#[derive(Clone, Debug)]
pub struct Resolved {
	/// The album or playlist name; `None` for a track link.
	pub name: Option<String>,
	pub tracks: Vec<Arc<TrackInfo>>,
	pub skipped: Vec<Skipped>,
}

/// Where track, album and playlist metadata comes from.
pub trait MetadataSource: Send + Sync {
	fn track(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<Arc<TrackInfo>, CoreError>> + Send;

	/// The album name and its track ids in album order.
	fn album(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send;

	/// The playlist name and its entries in playlist order.
	fn playlist(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyUri>), CoreError>> + Send;

	fn episode(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<Arc<TrackInfo>, CoreError>> + Send;

	/// The show name and its episode ids in the order Spotify lists them.
	fn show(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send;
}

enum Entry {
	Item(ItemId),
	Skip(Skipped),
}

enum Fetched {
	Item(ItemId, Result<Arc<TrackInfo>, CoreError>),
	Skip(Skipped),
}

/// Whether `info` can go in a playlist: Spotify has audio for it, or, for an episode Spotify
/// does not host, the podcast's own link.
fn playable(info: &TrackInfo) -> bool {
	(info.has_audio || info.external_url.is_some()) && info.unavailable.is_none()
}

/// Resolves `link` into tracks. Entries that can't be played are skipped with a reason; only
/// when nothing at all could be resolved is the first error returned instead, so a network
/// failure never looks like an empty album.
pub async fn resolve<S: MetadataSource>(source: &S, link: &Link) -> Result<Resolved, CoreError> {
	let (name, entries) = match link.kind {
		LinkKind::Track => (None, vec![Entry::Item(ItemId::Track(link.id))]),
		LinkKind::Episode => (None, vec![Entry::Item(ItemId::Episode(link.id))]),
		LinkKind::Album => {
			let (name, ids) = source.album(link.id).await?;
			(Some(name), ids.into_iter().map(|id| Entry::Item(ItemId::Track(id))).collect())
		}
		LinkKind::Playlist => {
			let (name, uris) = source.playlist(link.id).await?;
			(Some(name), uris.iter().map(playlist_entry).collect())
		}
		LinkKind::Show => {
			let (name, ids) = source.show(link.id).await?;
			(Some(name), ids.into_iter().map(|id| Entry::Item(ItemId::Episode(id))).collect())
		}
	};

	// `buffered` runs several lookups at once but yields them in entry order.
	let fetched: Vec<Fetched> = stream::iter(entries)
		.map(|entry| async move {
			match entry {
				Entry::Item(item @ ItemId::Track(id)) => {
					Fetched::Item(item, source.track(id).await)
				}
				Entry::Item(item @ ItemId::Episode(id)) => {
					Fetched::Item(item, source.episode(id).await)
				}
				Entry::Skip(skipped) => Fetched::Skip(skipped),
			}
		})
		.buffered(FETCH_CONCURRENCY)
		.collect()
		.await;

	let mut resolved = Resolved { name, tracks: Vec::new(), skipped: Vec::new() };
	let mut first_error = None;
	for item in fetched {
		match item {
			Fetched::Skip(skipped) => resolved.skipped.push(skipped),
			Fetched::Item(_, Ok(info)) if playable(&info) => resolved.tracks.push(info),
			Fetched::Item(item, Ok(info)) => resolved.skipped.push(Skipped {
				uri: item.uri().to_uri(),
				reason: SkipReason::Unavailable(
					info.unavailable.clone().unwrap_or_else(|| no_audio(item).to_owned()),
				),
			}),
			Fetched::Item(item, Err(error)) => {
				resolved.skipped.push(Skipped {
					uri: item.uri().to_uri(),
					reason: SkipReason::Unavailable(error.to_string()),
				});
				first_error.get_or_insert(error);
			}
		}
	}
	if resolved.tracks.is_empty()
		&& let Some(error) = first_error
	{
		return Err(error);
	}
	Ok(resolved)
}

const fn no_audio(item: ItemId) -> &'static str {
	match item {
		ItemId::Track(_) => "Spotify has no audio for this track",
		ItemId::Episode(_) => "Spotify has no audio for this episode",
	}
}

fn playlist_entry(uri: &SpotifyUri) -> Entry {
	let reason = match uri {
		SpotifyUri::Track { id } => return Entry::Item(ItemId::Track(*id)),
		SpotifyUri::Episode { id } => return Entry::Item(ItemId::Episode(*id)),
		SpotifyUri::Local { .. } => SkipReason::LocalFile,
		other => SkipReason::Unsupported(other.item_type().to_owned()),
	};
	Entry::Skip(Skipped { uri: uri.to_uri(), reason })
}

/// Track and episode metadata already fetched this run, shared by everything that looks them up.
#[derive(Clone, Debug, Default)]
pub struct TrackCache(Arc<Mutex<HashMap<ItemId, Arc<TrackInfo>>>>);

impl TrackCache {
	pub fn get(&self, id: &ItemId) -> Option<Arc<TrackInfo>> {
		self.0.lock().unwrap_or_else(PoisonError::into_inner).get(id).cloned()
	}

	pub fn insert(&self, info: Arc<TrackInfo>) {
		self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(info.id, info);
	}

	/// Forgets every track, for example after logging in to another account, whose country can
	/// make different tracks available.
	pub fn clear(&self) {
		self.0.lock().unwrap_or_else(PoisonError::into_inner).clear();
	}
}

/// A [`MetadataSource`] that looks each track and episode up only once.
pub struct CachedSource<S> {
	inner: S,
	cache: TrackCache,
}

impl<S> CachedSource<S> {
	pub fn new(inner: S, cache: TrackCache) -> Self {
		Self { inner, cache }
	}

	pub fn inner(&self) -> &S {
		&self.inner
	}
}

impl<S: MetadataSource> MetadataSource for CachedSource<S> {
	async fn track(&self, id: SpotifyId) -> Result<Arc<TrackInfo>, CoreError> {
		if let Some(info) = self.cache.get(&ItemId::Track(id)) {
			return Ok(info);
		}
		let info = self.inner.track(id).await?;
		self.cache.insert(Arc::clone(&info));
		Ok(info)
	}

	async fn episode(&self, id: SpotifyId) -> Result<Arc<TrackInfo>, CoreError> {
		if let Some(info) = self.cache.get(&ItemId::Episode(id)) {
			return Ok(info);
		}
		let info = self.inner.episode(id).await?;
		self.cache.insert(Arc::clone(&info));
		Ok(info)
	}

	fn show(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send {
		self.inner.show(id)
	}

	fn album(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send {
		self.inner.album(id)
	}

	fn playlist(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyUri>), CoreError>> + Send {
		self.inner.playlist(id)
	}
}

/// Metadata straight from Spotify, through librespot.
pub struct LibrespotMetadata {
	session: Session,
}

impl LibrespotMetadata {
	pub fn new(session: Session) -> Self {
		Self { session }
	}
}

impl MetadataSource for LibrespotMetadata {
	async fn track(&self, id: SpotifyId) -> Result<Arc<TrackInfo>, CoreError> {
		// One request for both the track and its loudness, the same number librespot's own
		// `Track::get` makes for the track alone.
		let query = |kind| ExtensionQuery {
			extension_kind: EnumOrUnknown::new(kind),
			..Default::default()
		};
		let request = BatchedEntityRequest {
			entity_request: vec![EntityRequest {
				entity_uri: SpotifyUri::Track { id }.to_uri(),
				query: vec![query(ExtensionKind::TRACK_V4), query(ExtensionKind::AUDIO_FILES)],
				..Default::default()
			}],
			..Default::default()
		};
		let response = self.session.spclient().get_extended_metadata(request).await?;
		let values = extension_values(&response);
		let bytes = values.track.ok_or_else(|| {
			CoreError::Unavailable("Spotify sent no metadata for this track".to_owned())
		})?;
		let message =
			TrackMessage::parse_from_bytes(&bytes).map_err(librespot_core::Error::from)?;
		let track = Track::try_from(&message)?;
		let user = self.session.user_data();
		let catalogue = user.attributes.get("catalogue").map_or("premium", String::as_str);
		let mut info = track_info(&track)?;
		info.unavailable = availability(&track, &user.country, catalogue, &Date::now_utc()).err();
		info.replay_gain = values.audio_files.as_deref().and_then(parse_audio_files);
		Ok(Arc::new(info))
	}

	async fn album(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
		let album = Album::get(&self.session, &SpotifyUri::Album { id }).await?;
		let ids = album.tracks().map(SpotifyId::try_from).collect::<Result<Vec<_>, _>>()?;
		Ok((album.name, ids))
	}

	async fn playlist(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyUri>), CoreError> {
		let playlist =
			Playlist::get(&self.session, &SpotifyUri::Playlist { user: None, id }).await?;
		Ok((playlist.name().to_owned(), playlist.tracks().cloned().collect()))
	}

	async fn episode(&self, id: SpotifyId) -> Result<Arc<TrackInfo>, CoreError> {
		let episode = Episode::get(&self.session, &SpotifyUri::Episode { id }).await?;
		Ok(Arc::new(episode_info(&episode)?))
	}

	async fn show(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
		let show = Show::get(&self.session, &SpotifyUri::Show { id }).await?;
		let ids = show.episodes.0.iter().map(SpotifyId::try_from).collect::<Result<Vec<_>, _>>()?;
		Ok((show.name, ids))
	}
}

/// The raw values of the extensions asked for in one extended-metadata request.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExtensionValues {
	/// A `TRACK_V4` value: a serialized `metadata.Track`.
	pub track: Option<Vec<u8>>,
	/// An `AUDIO_FILES` value: a serialized `AudioFilesExtensionResponse`.
	pub audio_files: Option<Vec<u8>>,
}

/// Picks the track and audio-files values out of a response. Entries Spotify reports as failed
/// (a status code outside 2xx) are left out.
pub fn extension_values(response: &BatchedExtensionResponse) -> ExtensionValues {
	let mut values = ExtensionValues::default();
	for array in &response.extended_metadata {
		let Some(value) = array.extension_data.iter().find_map(|data| {
			let status = data.header.as_ref().map_or(200, |header| header.status_code);
			(200..300).contains(&status).then(|| data.extension_data.as_ref()).flatten()
		}) else {
			continue;
		};
		match array.extension_kind.enum_value() {
			Ok(ExtensionKind::TRACK_V4) => values.track = Some(value.value.clone()),
			Ok(ExtensionKind::AUDIO_FILES) => values.audio_files = Some(value.value.clone()),
			_ => {}
		}
	}
	values
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::time::Duration;

	use librespot_protocol::entity_extension_data::{
		EntityExtensionData, EntityExtensionDataHeader,
	};
	use librespot_protocol::extended_metadata::{
		BatchedExtensionResponse, EntityExtensionDataArray,
	};
	use librespot_protocol::extension_kind::ExtensionKind;
	use protobuf::well_known_types::any::Any;
	use protobuf::{EnumOrUnknown, MessageField};

	use super::{CachedSource, MetadataSource, SkipReason, TrackCache, extension_values, resolve};
	use crate::link::{ItemId, Link, LinkKind};
	use crate::metadata::TrackInfo;
	use crate::{CoreError, SpotifyId, SpotifyUri};

	fn id(byte: u8) -> SpotifyId {
		SpotifyId::from_raw(&[byte; 16]).unwrap()
	}

	fn info(byte: u8) -> TrackInfo {
		TrackInfo {
			id: ItemId::Track(id(byte)),
			title: format!("Track {byte}"),
			artists: vec!["Artist".to_owned()],
			album: "Album".to_owned(),
			album_artists: vec!["Artist".to_owned()],
			track_number: u32::from(byte),
			disc_number: 1,
			date: None,
			duration_ms: 1000,
			covers: Vec::new(),
			has_audio: true,
			unavailable: None,
			replay_gain: None,
			description: None,
			external_url: None,
		}
	}

	fn episode(byte: u8) -> TrackInfo {
		TrackInfo {
			id: ItemId::Episode(id(byte)),
			title: format!("Episode {byte}"),
			artists: vec!["The Show".to_owned()],
			album: "The Show".to_owned(),
			album_artists: vec!["The Show".to_owned()],
			track_number: 0,
			disc_number: 0,
			date: Some("2026-10-01".to_owned()),
			duration_ms: 1000,
			covers: Vec::new(),
			has_audio: true,
			unavailable: None,
			replay_gain: None,
			description: Some("What this episode is about.".to_owned()),
			external_url: None,
		}
	}

	#[derive(Default)]
	struct FakeSource {
		tracks: HashMap<SpotifyId, (Result<TrackInfo, &'static str>, Duration)>,
		episodes: HashMap<SpotifyId, TrackInfo>,
		albums: HashMap<SpotifyId, Result<(String, Vec<SpotifyId>), &'static str>>,
		playlists: HashMap<SpotifyId, (String, Vec<SpotifyUri>)>,
		shows: HashMap<SpotifyId, (String, Vec<SpotifyId>)>,
		track_calls: AtomicUsize,
	}

	impl FakeSource {
		fn with_tracks(bytes: &[u8]) -> Self {
			let mut source = Self::default();
			for &byte in bytes {
				source.tracks.insert(id(byte), (Ok(info(byte)), Duration::ZERO));
			}
			source
		}
	}

	impl MetadataSource for FakeSource {
		async fn track(&self, id: SpotifyId) -> Result<Arc<TrackInfo>, CoreError> {
			self.track_calls.fetch_add(1, Ordering::SeqCst);
			let (result, delay) = self.tracks.get(&id).expect("unknown track");
			tokio::time::sleep(*delay).await;
			result
				.clone()
				.map(Arc::new)
				.map_err(|message| CoreError::Unavailable(message.to_owned()))
		}

		async fn album(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
			tokio::task::yield_now().await;
			self.albums
				.get(&id)
				.expect("unknown album")
				.clone()
				.map_err(|message| CoreError::Unavailable(message.to_owned()))
		}

		async fn playlist(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyUri>), CoreError> {
			tokio::task::yield_now().await;
			Ok(self.playlists.get(&id).expect("unknown playlist").clone())
		}

		async fn episode(&self, id: SpotifyId) -> Result<Arc<TrackInfo>, CoreError> {
			tokio::task::yield_now().await;
			Ok(Arc::new(self.episodes.get(&id).expect("unknown episode").clone()))
		}

		async fn show(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
			tokio::task::yield_now().await;
			Ok(self.shows.get(&id).expect("unknown show").clone())
		}
	}

	fn titles(tracks: &[Arc<TrackInfo>]) -> Vec<&str> {
		tracks.iter().map(|track| track.title.as_str()).collect()
	}

	#[tokio::test]
	async fn track_link_resolves_to_one_track() {
		let source = FakeSource::with_tracks(&[1]);
		let resolved = resolve(&source, &Link { kind: LinkKind::Track, id: id(1) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1"]);
		assert_eq!(resolved.name, None);
		assert!(resolved.skipped.is_empty());
	}

	#[tokio::test]
	async fn episode_link_resolves_to_one_episode() {
		let mut source = FakeSource::default();
		source.episodes.insert(id(50), episode(50));
		let resolved =
			resolve(&source, &Link { kind: LinkKind::Episode, id: id(50) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Episode 50"]);
		assert_eq!(resolved.tracks[0].id, ItemId::Episode(id(50)));
	}

	#[tokio::test]
	async fn show_link_resolves_to_its_episodes_in_order_with_the_show_name() {
		let mut source = FakeSource::default();
		for byte in [50, 51, 52] {
			source.episodes.insert(id(byte), episode(byte));
		}
		source.shows.insert(id(250), ("The Show".to_owned(), vec![id(52), id(50), id(51)]));
		let resolved = resolve(&source, &Link { kind: LinkKind::Show, id: id(250) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Episode 52", "Episode 50", "Episode 51"]);
		assert_eq!(resolved.name.as_deref(), Some("The Show"));
	}

	#[tokio::test]
	async fn an_episode_spotify_doesnt_host_is_kept_for_its_own_link() {
		let mut source = FakeSource::default();
		let mut hosted_elsewhere = episode(50);
		hosted_elsewhere.has_audio = false;
		hosted_elsewhere.external_url = Some("https://podcasts.example/episode-50.mp3".to_owned());
		let mut silent = episode(51);
		silent.has_audio = false;
		source.episodes.insert(id(50), hosted_elsewhere);
		source.episodes.insert(id(51), silent);
		source.shows.insert(id(250), ("The Show".to_owned(), vec![id(50), id(51)]));
		let resolved = resolve(&source, &Link { kind: LinkKind::Show, id: id(250) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Episode 50"]);
		assert_eq!(resolved.skipped.len(), 1);
		assert_eq!(resolved.skipped[0].uri, SpotifyUri::Episode { id: id(51) }.to_uri());
	}

	#[tokio::test]
	async fn track_link_failure_is_an_error() {
		let mut source = FakeSource::default();
		source.tracks.insert(id(1), (Err("gone"), Duration::ZERO));
		let result = resolve(&source, &Link { kind: LinkKind::Track, id: id(1) }).await;
		assert!(matches!(&result, Err(CoreError::Unavailable(message)) if message == "gone"));
	}

	#[tokio::test]
	async fn album_keeps_album_order_when_fetches_finish_out_of_order() {
		let mut source = FakeSource::default();
		for byte in 1..=5u8 {
			let delay = Duration::from_millis(u64::from(6 - byte) * 20);
			source.tracks.insert(id(byte), (Ok(info(byte)), delay));
		}
		source.albums.insert(id(100), Ok(("The Album".to_owned(), (1..=5).map(id).collect())));
		let resolved =
			resolve(&source, &Link { kind: LinkKind::Album, id: id(100) }).await.unwrap();
		assert_eq!(
			titles(&resolved.tracks),
			["Track 1", "Track 2", "Track 3", "Track 4", "Track 5"]
		);
		assert_eq!(resolved.name.as_deref(), Some("The Album"));
	}

	#[tokio::test]
	async fn playlist_keeps_episodes_and_skips_local_files_with_a_reason() {
		let mut source = FakeSource::with_tracks(&[1, 2]);
		source.episodes.insert(id(50), episode(50));
		let episode = SpotifyUri::Episode { id: id(50) };
		let local = SpotifyUri::from_uri("spotify:local:Artist:Album:Song:180").unwrap();
		source.playlists.insert(
			id(200),
			(
				"Mix".to_owned(),
				vec![
					SpotifyUri::Track { id: id(1) },
					episode.clone(),
					local.clone(),
					SpotifyUri::Track { id: id(2) },
				],
			),
		);
		let resolved =
			resolve(&source, &Link { kind: LinkKind::Playlist, id: id(200) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1", "Episode 50", "Track 2"]);
		assert_eq!(resolved.name.as_deref(), Some("Mix"));
		let skipped: Vec<_> =
			resolved.skipped.iter().map(|item| (item.uri.clone(), item.reason.clone())).collect();
		assert_eq!(skipped, [(local.to_uri(), SkipReason::LocalFile)]);
	}

	#[tokio::test]
	async fn failed_track_in_playlist_is_skipped_not_fatal() {
		let mut source = FakeSource::with_tracks(&[1]);
		source.tracks.insert(id(2), (Err("region locked"), Duration::ZERO));
		source.playlists.insert(
			id(200),
			(
				"Mix".to_owned(),
				vec![SpotifyUri::Track { id: id(2) }, SpotifyUri::Track { id: id(1) }],
			),
		);
		let resolved =
			resolve(&source, &Link { kind: LinkKind::Playlist, id: id(200) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1"]);
		assert_eq!(resolved.skipped.len(), 1);
		assert_eq!(resolved.skipped[0].uri, SpotifyUri::Track { id: id(2) }.to_uri());
		assert!(
			matches!(&resolved.skipped[0].reason, SkipReason::Unavailable(message) if message.contains("region locked"))
		);
	}

	#[tokio::test]
	async fn track_without_audio_is_skipped() {
		let mut source = FakeSource::with_tracks(&[1]);
		let mut silent = info(2);
		silent.has_audio = false;
		source.tracks.insert(id(2), (Ok(silent), Duration::ZERO));
		source.albums.insert(id(100), Ok(("The Album".to_owned(), vec![id(1), id(2)])));
		let resolved =
			resolve(&source, &Link { kind: LinkKind::Album, id: id(100) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1"]);
		assert!(matches!(resolved.skipped[0].reason, SkipReason::Unavailable(_)));
	}

	#[tokio::test]
	async fn track_unavailable_in_the_users_country_is_skipped() {
		let mut source = FakeSource::with_tracks(&[1]);
		let mut locked = info(2);
		locked.unavailable = Some("not available in your country".to_owned());
		source.tracks.insert(id(2), (Ok(locked), Duration::ZERO));
		source.albums.insert(id(100), Ok(("The Album".to_owned(), vec![id(2), id(1)])));
		let resolved =
			resolve(&source, &Link { kind: LinkKind::Album, id: id(100) }).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1"]);
		assert_eq!(
			resolved.skipped[0].reason,
			SkipReason::Unavailable("not available in your country".to_owned())
		);
	}

	fn extension(kind: ExtensionKind, status: i32, value: &[u8]) -> EntityExtensionDataArray {
		EntityExtensionDataArray {
			extension_kind: EnumOrUnknown::new(kind),
			extension_data: vec![EntityExtensionData {
				header: MessageField::some(EntityExtensionDataHeader {
					status_code: status,
					..Default::default()
				}),
				entity_uri: "spotify:track:2hQxLiI93xdjqENJuKeXjO".to_owned(),
				extension_data: MessageField::some(Any {
					value: value.to_vec(),
					..Default::default()
				}),
				..Default::default()
			}],
			..Default::default()
		}
	}

	#[test]
	fn extension_values_finds_track_and_audio_files() {
		let response = BatchedExtensionResponse {
			extended_metadata: vec![
				extension(ExtensionKind::AUDIO_FILES, 200, b"loudness"),
				extension(ExtensionKind::TRACK_V4, 200, b"track"),
			],
			..Default::default()
		};
		let values = extension_values(&response);
		assert_eq!(values.track.as_deref(), Some(&b"track"[..]));
		assert_eq!(values.audio_files.as_deref(), Some(&b"loudness"[..]));
	}

	#[test]
	fn extension_values_skip_failed_entries() {
		let response = BatchedExtensionResponse {
			extended_metadata: vec![
				extension(ExtensionKind::TRACK_V4, 200, b"track"),
				extension(ExtensionKind::AUDIO_FILES, 404, b"not found"),
			],
			..Default::default()
		};
		let values = extension_values(&response);
		assert_eq!(values.track.as_deref(), Some(&b"track"[..]));
		assert_eq!(values.audio_files, None);
	}

	#[test]
	fn cache_clear_forgets_tracks() {
		let cache = TrackCache::default();
		cache.insert(Arc::new(info(1)));
		cache.clear();
		assert!(cache.get(&ItemId::Track(id(1))).is_none());
	}

	#[tokio::test]
	async fn all_tracks_failing_returns_the_first_error() {
		let mut source = FakeSource::default();
		source.tracks.insert(id(1), (Err("first"), Duration::from_millis(30)));
		source.tracks.insert(id(2), (Err("second"), Duration::ZERO));
		source.albums.insert(id(100), Ok(("The Album".to_owned(), vec![id(1), id(2)])));
		let result = resolve(&source, &Link { kind: LinkKind::Album, id: id(100) }).await;
		assert!(matches!(&result, Err(CoreError::Unavailable(message)) if message == "first"));
	}

	#[tokio::test]
	async fn album_fetch_failure_is_an_error() {
		let mut source = FakeSource::default();
		source.albums.insert(id(100), Err("no such album"));
		let result = resolve(&source, &Link { kind: LinkKind::Album, id: id(100) }).await;
		assert!(
			matches!(&result, Err(CoreError::Unavailable(message)) if message == "no such album")
		);
	}

	#[tokio::test]
	async fn cached_source_fetches_each_track_once() {
		let mut inner = FakeSource::with_tracks(&[1, 2, 3]);
		inner.albums.insert(id(100), Ok(("The Album".to_owned(), vec![id(1), id(2), id(3)])));
		let cache = TrackCache::default();
		let source = CachedSource::new(inner, cache.clone());
		let link = Link { kind: LinkKind::Album, id: id(100) };
		resolve(&source, &link).await.unwrap();
		let again = resolve(&source, &link).await.unwrap();
		assert_eq!(again.tracks.len(), 3);
		assert_eq!(source.inner().track_calls.load(Ordering::SeqCst), 3);
		assert_eq!(
			cache.get(&ItemId::Track(id(2))).map(|track| track.title.clone()).as_deref(),
			Some("Track 2")
		);
	}
}
