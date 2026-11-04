//! Turning a track, album or playlist link into the tracks to add to a foobar2000 playlist.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use futures_util::stream::{self, StreamExt};
use librespot_core::Session;
use librespot_core::date::Date;
use librespot_metadata::{Album, Metadata, Playlist, Track};

use crate::link::{Link, LinkKind};
use crate::metadata::{TrackInfo, availability, track_info};
use crate::{CoreError, SpotifyId, SpotifyUri};

/// How many track lookups run at once while resolving an album or playlist.
pub const FETCH_CONCURRENCY: usize = 8;

/// Why an entry of an album or playlist was left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
	Episode,
	LocalFile,
	Unsupported(String),
	Unavailable(String),
}

impl fmt::Display for SkipReason {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Episode => f.write_str("podcast episodes can't be played"),
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
}

enum Entry {
	Track(SpotifyId),
	Skip(Skipped),
}

enum Fetched {
	Track(SpotifyId, Result<Arc<TrackInfo>, CoreError>),
	Skip(Skipped),
}

/// Resolves `link` into tracks. Entries that can't be played are skipped with a reason; only
/// when nothing at all could be resolved is the first error returned instead, so a network
/// failure never looks like an empty album.
pub async fn resolve<S: MetadataSource>(source: &S, link: &Link) -> Result<Resolved, CoreError> {
	let (name, entries) = match link.kind {
		LinkKind::Track => (None, vec![Entry::Track(link.id)]),
		LinkKind::Album => {
			let (name, ids) = source.album(link.id).await?;
			(Some(name), ids.into_iter().map(Entry::Track).collect())
		}
		LinkKind::Playlist => {
			let (name, uris) = source.playlist(link.id).await?;
			(Some(name), uris.iter().map(playlist_entry).collect())
		}
	};

	// `buffered` runs several lookups at once but yields them in entry order.
	let fetched: Vec<Fetched> = stream::iter(entries)
		.map(|entry| async move {
			match entry {
				Entry::Track(id) => Fetched::Track(id, source.track(id).await),
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
			Fetched::Track(_, Ok(info)) if info.has_audio && info.unavailable.is_none() => {
				resolved.tracks.push(info);
			}
			Fetched::Track(id, Ok(info)) => resolved.skipped.push(Skipped {
				uri: SpotifyUri::Track { id }.to_uri(),
				reason: SkipReason::Unavailable(
					info.unavailable
						.clone()
						.unwrap_or_else(|| "Spotify has no audio for this track".to_owned()),
				),
			}),
			Fetched::Track(id, Err(error)) => {
				resolved.skipped.push(Skipped {
					uri: SpotifyUri::Track { id }.to_uri(),
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

fn playlist_entry(uri: &SpotifyUri) -> Entry {
	let reason = match uri {
		SpotifyUri::Track { id } => return Entry::Track(*id),
		SpotifyUri::Episode { .. } => SkipReason::Episode,
		SpotifyUri::Local { .. } => SkipReason::LocalFile,
		other => SkipReason::Unsupported(other.item_type().to_owned()),
	};
	Entry::Skip(Skipped { uri: uri.to_uri(), reason })
}

/// Track metadata already fetched this run, shared by everything that looks tracks up.
#[derive(Clone, Debug, Default)]
pub struct TrackCache(Arc<Mutex<HashMap<SpotifyId, Arc<TrackInfo>>>>);

impl TrackCache {
	pub fn get(&self, id: &SpotifyId) -> Option<Arc<TrackInfo>> {
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

/// A [`MetadataSource`] that looks each track up only once.
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
		if let Some(info) = self.cache.get(&id) {
			return Ok(info);
		}
		let info = self.inner.track(id).await?;
		self.cache.insert(Arc::clone(&info));
		Ok(info)
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
		let track = Track::get(&self.session, &SpotifyUri::Track { id }).await?;
		let user = self.session.user_data();
		let catalogue = user.attributes.get("catalogue").map_or("premium", String::as_str);
		let mut info = track_info(&track)?;
		info.unavailable = availability(&track, &user.country, catalogue, &Date::now_utc()).err();
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
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::time::Duration;

	use super::{CachedSource, MetadataSource, SkipReason, TrackCache, resolve};
	use crate::link::{Link, LinkKind};
	use crate::metadata::TrackInfo;
	use crate::{CoreError, SpotifyId, SpotifyUri};

	fn id(byte: u8) -> SpotifyId {
		SpotifyId::from_raw(&[byte; 16]).unwrap()
	}

	fn info(byte: u8) -> TrackInfo {
		TrackInfo {
			id: id(byte),
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
		}
	}

	#[derive(Default)]
	struct FakeSource {
		tracks: HashMap<SpotifyId, (Result<TrackInfo, &'static str>, Duration)>,
		albums: HashMap<SpotifyId, Result<(String, Vec<SpotifyId>), &'static str>>,
		playlists: HashMap<SpotifyId, (String, Vec<SpotifyUri>)>,
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
	async fn playlist_skips_episodes_and_local_files_with_reasons() {
		let mut source = FakeSource::with_tracks(&[1, 2]);
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
		assert_eq!(titles(&resolved.tracks), ["Track 1", "Track 2"]);
		assert_eq!(resolved.name.as_deref(), Some("Mix"));
		let skipped: Vec<_> =
			resolved.skipped.iter().map(|item| (item.uri.clone(), item.reason.clone())).collect();
		assert_eq!(
			skipped,
			[(episode.to_uri(), SkipReason::Episode), (local.to_uri(), SkipReason::LocalFile)]
		);
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

	#[test]
	fn cache_clear_forgets_tracks() {
		let cache = TrackCache::default();
		cache.insert(Arc::new(info(1)));
		cache.clear();
		assert!(cache.get(&id(1)).is_none());
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
		assert_eq!(cache.get(&id(2)).map(|track| track.title.clone()).as_deref(), Some("Track 2"));
	}
}
