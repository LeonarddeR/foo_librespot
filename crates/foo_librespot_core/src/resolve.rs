//! Turning a Spotify link into the tracks and episodes to add to a foobar2000 playlist.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use librespot_core::Session;
use librespot_core::date::Date;
use librespot_core::error::ErrorKind;
use librespot_metadata::show::ShowConsumptionOrder;
use librespot_metadata::{Album, Artist, Episode, Metadata, Playlist, Show, Track};
use librespot_protocol::context_page::ContextPage;
use librespot_protocol::extended_metadata::{
	BatchedEntityRequest, BatchedExtensionResponse, EntityRequest, ExtensionQuery,
};
use librespot_protocol::extension_kind::ExtensionKind;
use librespot_protocol::metadata::{
	Album as AlbumMessage, Episode as EpisodeMessage, Track as TrackMessage,
};
use protobuf::{EnumOrUnknown, Message};

use crate::genre::parse_genres;
use crate::link::{ItemId, Link, LinkKind};
use crate::metadata::{
	AlbumDetails, TrackInfo, availability, episode_availability, episode_info, explicit_allowed,
	track_info,
};
use crate::replaygain::parse_audio_files;
use crate::{CoreError, SpotifyId, SpotifyUri};

/// How many tracks or episodes one metadata request asks for. librespot allows 300 requests per
/// 30 seconds, so asking for each item on its own stalls a show of 2000 episodes; in batches it
/// takes 20 requests.
pub const BATCH_SIZE: usize = 100;

/// How many batches are asked for at once.
pub const BATCH_CONCURRENCY: usize = 2;

/// The first wait before a request librespot refused for its rate limit is tried again. Each
/// wait doubles, so the last try comes about 30 seconds after the first, librespot's window.
const RATE_LIMIT_FIRST_WAIT: Duration = Duration::from_millis(500);
const RATE_LIMIT_TRIES: u32 = 7;

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
	/// The album, playlist, show or artist name, "Liked Songs", or the search words; `None` for a
	/// track or episode link.
	pub name: Option<String>,
	pub tracks: Vec<Arc<TrackInfo>>,
	pub skipped: Vec<Skipped>,
	/// Playable tracks left out because an earlier track had the same ISRC; only a discography
	/// leaves any out.
	pub duplicates: usize,
}

/// What an artist link adds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArtistTracks {
	/// The artist's popular tracks in the account's country, as their Spotify page lists them.
	Popular,
	/// The artist's albums, singles, EPs and compilations, each recording once.
	#[default]
	Discography,
}

/// Where the metadata of tracks and episodes, and of everything that lists them, comes from.
pub trait MetadataSource: Send + Sync {
	/// The info of each of `items`, in the same order, each with its own result.
	fn items(
		&self,
		items: &[ItemId],
	) -> impl Future<Output = Vec<Result<Arc<TrackInfo>, CoreError>>> + Send;

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

	/// The show name and its episode ids in listening order: oldest first for a show meant to be
	/// heard from the start, newest first for the rest.
	fn show(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send;

	/// The artist name and their popular tracks in the account's country, in Spotify's order.
	fn artist(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send;

	/// The track and episode URIs of the logged-in account's Liked Songs, in Spotify's order.
	fn liked_songs(&self) -> impl Future<Output = Result<Vec<SpotifyUri>, CoreError>> + Send;

	/// The track and episode URIs Spotify finds first for `words`: the first page of its search.
	fn search(
		&self,
		words: &str,
	) -> impl Future<Output = Result<Vec<SpotifyUri>, CoreError>> + Send;

	/// The artist name and their releases: [`releases`] of their metadata.
	fn discography(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send;
}

/// The info of one item.
pub async fn item<S: MetadataSource>(
	source: &S,
	item: ItemId,
) -> Result<Arc<TrackInfo>, CoreError> {
	source.items(&[item]).await.pop().unwrap_or_else(|| Err(missing(item)))
}

fn missing(item: ItemId) -> CoreError {
	CoreError::Unavailable(format!("Spotify sent no metadata for this {}", item.uri().item_type()))
}

enum Entry {
	Item(ItemId),
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
pub async fn resolve<S: MetadataSource>(
	source: &S,
	link: &Link,
	artist_tracks: ArtistTracks,
) -> Result<Resolved, CoreError> {
	let mut first_error = None;
	let (name, entries) = match link {
		Link::Id { kind: LinkKind::Track | LinkKind::Episode, .. } => {
			(None, link.item().map(Entry::Item).into_iter().collect::<Vec<_>>())
		}
		Link::Id { kind: LinkKind::Album, id } => {
			let (name, ids) = source.album(*id).await?;
			(Some(name), ids.into_iter().map(|id| Entry::Item(ItemId::Track(id))).collect())
		}
		Link::Id { kind: LinkKind::Playlist, id } => {
			let (name, uris) = source.playlist(*id).await?;
			(Some(name), uris.iter().map(playlist_entry).collect())
		}
		Link::Id { kind: LinkKind::Show, id } => {
			let (name, ids) = source.show(*id).await?;
			(Some(name), ids.into_iter().map(|id| Entry::Item(ItemId::Episode(id))).collect())
		}
		Link::Id { kind: LinkKind::Artist, id } if artist_tracks == ArtistTracks::Discography => {
			let (name, releases) = source.discography(*id).await?;
			let (entries, error) = discography_entries(source, releases).await;
			first_error = error;
			(Some(name), entries)
		}
		Link::Id { kind: LinkKind::Artist, id } => {
			let (name, ids) = source.artist(*id).await?;
			(Some(name), ids.into_iter().map(|id| Entry::Item(ItemId::Track(id))).collect())
		}
		Link::LikedSongs => {
			let uris = source.liked_songs().await?;
			(Some("Liked Songs".to_owned()), uris.iter().map(playlist_entry).collect())
		}
		Link::Search(words) => {
			let uris = source.search(words).await?;
			(Some(words.clone()), uris.iter().map(playlist_entry).collect())
		}
	};

	let items: Vec<ItemId> = entries
		.iter()
		.filter_map(|entry| match entry {
			Entry::Item(item) => Some(*item),
			Entry::Skip(_) => None,
		})
		.collect();
	// `buffered` runs several batches at once but yields them in order.
	let mut fetched = stream::iter(items.chunks(BATCH_SIZE))
		.map(|batch| source.items(batch))
		.buffered(BATCH_CONCURRENCY)
		.flat_map(stream::iter)
		.collect::<Vec<_>>()
		.await
		.into_iter();

	let mut resolved = Resolved { name, tracks: Vec::new(), skipped: Vec::new(), duplicates: 0 };
	let discography = artist_tracks == ArtistTracks::Discography
		&& matches!(link, Link::Id { kind: LinkKind::Artist, .. });
	let mut recordings = HashSet::new();
	for entry in entries {
		let item = match entry {
			Entry::Skip(skipped) => {
				resolved.skipped.push(skipped);
				continue;
			}
			Entry::Item(item) => item,
		};
		match fetched.next().unwrap_or_else(|| Err(missing(item))) {
			Ok(info) if playable(&info) => {
				if discography
					&& let Some(isrc) = &info.isrc
					&& !recordings.insert(isrc.clone())
				{
					resolved.duplicates += 1;
				} else {
					resolved.tracks.push(info);
				}
			}
			Ok(info) => resolved.skipped.push(Skipped {
				uri: item.uri().to_uri(),
				reason: SkipReason::Unavailable(info.unavailable.clone().unwrap_or_else(|| {
					format!("Spotify has no audio for this {}", item.uri().item_type())
				})),
			}),
			Err(error) => {
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

fn playlist_entry(uri: &SpotifyUri) -> Entry {
	let reason = match uri {
		SpotifyUri::Track { id } => return Entry::Item(ItemId::Track(*id)),
		SpotifyUri::Episode { id } => return Entry::Item(ItemId::Episode(*id)),
		SpotifyUri::Local { .. } => SkipReason::LocalFile,
		other => SkipReason::Unsupported(other.item_type().to_owned()),
	};
	Entry::Skip(Skipped { uri: uri.to_uri(), reason })
}

/// The tracks of `releases` in order, each release's in album order, and the first error; a
/// release that fails to load is skipped with its reason.
async fn discography_entries<S: MetadataSource>(
	source: &S,
	releases: Vec<SpotifyId>,
) -> (Vec<Entry>, Option<CoreError>) {
	let albums: Vec<_> = stream::iter(releases)
		.map(|release| async move { (release, source.album(release).await) })
		.buffered(BATCH_CONCURRENCY)
		.collect()
		.await;
	let mut entries = Vec::new();
	let mut first_error = None;
	for (release, album) in albums {
		match album {
			Ok((_, ids)) => {
				entries.extend(ids.into_iter().map(|id| Entry::Item(ItemId::Track(id))));
			}
			Err(error) => {
				entries.push(Entry::Skip(Skipped {
					uri: SpotifyUri::Album { id: release }.to_uri(),
					reason: SkipReason::Unavailable(error.to_string()),
				}));
				first_error.get_or_insert(error);
			}
		}
	}
	(entries, first_error)
}

/// `newest_first`, the order Spotify lists a show's episodes in, put in the order the show is
/// meant to be heard: from the first episode for a sequential show, such as a Bible-in-a-year
/// podcast, and from the newest for the rest.
pub fn listening_order(
	mut newest_first: Vec<SpotifyId>,
	order: ShowConsumptionOrder,
) -> Vec<SpotifyId> {
	if order == ShowConsumptionOrder::SEQUENTIAL {
		newest_first.reverse();
	}
	newest_first
}

/// The releases a discography adds: each album, then each single and EP, then each compilation,
/// in Spotify's order within each group (newest first), taking each release's first edition.
/// Albums the artist only appears on are left out.
pub fn releases(artist: &Artist) -> Vec<SpotifyId> {
	[&artist.albums, &artist.singles, &artist.compilations]
		.into_iter()
		.flat_map(|groups| groups.iter())
		.filter_map(|group| group.first())
		.filter_map(|uri| SpotifyId::try_from(uri).ok())
		.collect()
}

/// How many further pages of one context foo_librespot fetches at most.
const MAX_CONTEXT_PAGES: usize = 1000;

/// One page of a Spotify context: its tracks, or where to fetch them, and where the next page is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Page {
	tracks: Vec<String>,
	/// Set on a page whose tracks still have to be fetched.
	tracks_url: Option<String>,
	next_url: Option<String>,
}

impl Page {
	fn of(page: &ContextPage) -> Self {
		Self {
			tracks: page
				.tracks
				.iter()
				.map(|track| track.uri().to_owned())
				.filter(|uri| !uri.is_empty())
				.collect(),
			tracks_url: Some(page.page_url().to_owned()).filter(|url| !url.is_empty()),
			next_url: Some(page.next_page_url().to_owned()).filter(|url| !url.is_empty()),
		}
	}

	/// A page as `get_next_page` sends it: a `ContextPage` in JSON, with its field names in
	/// either spelling.
	fn from_json(bytes: &[u8]) -> Result<Self, CoreError> {
		let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
			CoreError::Unavailable(format!("Spotify sent a page foo_librespot can't read: {error}"))
		})?;
		let text = |camel: &str, snake: &str| {
			value
				.get(camel)
				.or_else(|| value.get(snake))
				.and_then(serde_json::Value::as_str)
				.filter(|text| !text.is_empty())
				.map(str::to_owned)
		};
		Ok(Self {
			tracks: value["tracks"]
				.as_array()
				.into_iter()
				.flatten()
				.filter_map(|track| track["uri"].as_str())
				.filter(|uri| !uri.is_empty())
				.map(str::to_owned)
				.collect(),
			tracks_url: text("pageUrl", "page_url"),
			next_url: text("nextPageUrl", "next_page_url"),
		})
	}

	/// The page's tracks as librespot names them; ones it can't read are left out.
	fn uris(&self) -> Vec<SpotifyUri> {
		self.tracks.iter().filter_map(|uri| SpotifyUri::from_uri(uri).ok()).collect()
	}
}

/// The tracks of a context, in order.
#[derive(Debug)]
struct ContextTracks {
	uris: Vec<SpotifyUri>,
	/// Pages were left after [`MAX_CONTEXT_PAGES`] fetches.
	cut_off: bool,
}

/// The tracks of `pages` in order. A page without tracks but with a `page_url` is fetched in its
/// place, and a page's `next_page_url` is fetched right after it, through `fetch`, for at most
/// [`MAX_CONTEXT_PAGES`] fetches.
async fn context_tracks<F, Fut>(pages: Vec<Page>, fetch: F) -> Result<ContextTracks, CoreError>
where
	F: Fn(String) -> Fut,
	Fut: Future<Output = Result<Page, CoreError>>,
{
	let mut queue = VecDeque::from(pages);
	let mut uris = Vec::new();
	let mut fetches = 0;
	while let Some(mut page) = queue.pop_front() {
		if page.tracks.is_empty()
			&& let Some(url) = page.tracks_url.take()
		{
			if fetches == MAX_CONTEXT_PAGES {
				return Ok(ContextTracks { uris, cut_off: true });
			}
			fetches += 1;
			let mut loaded = fetch(url).await?;
			loaded.tracks_url = None;
			loaded.next_url = loaded.next_url.or(page.next_url);
			queue.push_front(loaded);
			continue;
		}
		uris.extend(page.uris());
		if let Some(url) = page.next_url {
			if fetches == MAX_CONTEXT_PAGES {
				return Ok(ContextTracks { uris, cut_off: true });
			}
			fetches += 1;
			queue.push_front(fetch(url).await?);
		}
	}
	Ok(ContextTracks { uris, cut_off: false })
}

/// Runs `request` again while librespot refuses it for its own rate limit, waiting longer each
/// time, for about 30 seconds in all.
pub async fn retry_rate_limited<T, F, Fut>(mut request: F) -> Result<T, librespot_core::Error>
where
	F: FnMut() -> Fut,
	Fut: Future<Output = Result<T, librespot_core::Error>>,
{
	let mut wait = RATE_LIMIT_FIRST_WAIT;
	for _ in 1..RATE_LIMIT_TRIES {
		match request().await {
			Err(error) if error.kind == ErrorKind::ResourceExhausted => {
				log::debug!("rate limited, trying again in {wait:?}: {error}");
				tokio::time::sleep(wait).await;
				wait *= 2;
			}
			result => return result,
		}
	}
	request().await
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

/// The album details fetched this run. They don't change, and without this every track read
/// one at a time, such as from an old foo_spotify playlist, would cost a second Spotify
/// request.
#[derive(Clone, Debug, Default)]
pub struct AlbumDetailsCache(Arc<Mutex<HashMap<String, AlbumDetails>>>);

impl AlbumDetailsCache {
	/// The details of `albums`, asking `fetch` only for the albums not seen before. Albums `fetch`
	/// has no details for are asked for again next time.
	pub async fn get_or_fetch<F, Fut>(
		&self,
		albums: Vec<String>,
		fetch: F,
	) -> HashMap<String, AlbumDetails>
	where
		F: FnOnce(Vec<String>) -> Fut,
		Fut: Future<Output = HashMap<String, AlbumDetails>>,
	{
		let (mut found, missing) = {
			let known = self.0.lock().unwrap_or_else(PoisonError::into_inner);
			let mut found = HashMap::new();
			let mut missing = Vec::new();
			for album in albums {
				match known.get(&album) {
					Some(details) => {
						found.insert(album, details.clone());
					}
					None => missing.push(album),
				}
			}
			(found, missing)
		};
		if !missing.is_empty() {
			let fetched = fetch(missing).await;
			self.0.lock().unwrap_or_else(PoisonError::into_inner).extend(fetched.clone());
			found.extend(fetched);
		}
		found
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
	/// Answers what the cache knows and asks the source for the rest in one batch.
	async fn items(&self, items: &[ItemId]) -> Vec<Result<Arc<TrackInfo>, CoreError>> {
		let cached: Vec<Option<Arc<TrackInfo>>> =
			items.iter().map(|item| self.cache.get(item)).collect();
		let missing_items: Vec<ItemId> = items
			.iter()
			.zip(&cached)
			.filter(|(_, info)| info.is_none())
			.map(|(item, _)| *item)
			.collect();
		let mut fetched = if missing_items.is_empty() {
			Vec::new().into_iter()
		} else {
			self.inner.items(&missing_items).await.into_iter()
		};
		items
			.iter()
			.zip(cached)
			.map(|(item, info)| {
				if let Some(info) = info {
					return Ok(info);
				}
				let result = fetched.next().unwrap_or_else(|| Err(missing(*item)));
				// Whether an item plays changes with the clock and the account's country, so an
				// unavailable one is asked for again next time.
				if let Ok(info) = &result
					&& info.unavailable.is_none()
				{
					self.cache.insert(Arc::clone(info));
				}
				result
			})
			.collect()
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

	fn show(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send {
		self.inner.show(id)
	}

	fn artist(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send {
		self.inner.artist(id)
	}

	fn liked_songs(&self) -> impl Future<Output = Result<Vec<SpotifyUri>, CoreError>> + Send {
		self.inner.liked_songs()
	}

	fn search(
		&self,
		words: &str,
	) -> impl Future<Output = Result<Vec<SpotifyUri>, CoreError>> + Send {
		self.inner.search(words)
	}

	fn discography(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<(String, Vec<SpotifyId>), CoreError>> + Send {
		self.inner.discography(id)
	}
}

/// Metadata straight from Spotify, through librespot.
pub struct LibrespotMetadata {
	session: Session,
	albums: AlbumDetailsCache,
}

/// What decides whether this account can play a track: read once per batch, not per track.
struct Viewer {
	country: String,
	catalogue: String,
	/// The account is set not to play explicit content.
	filter_explicit: bool,
	now: Date,
}

impl LibrespotMetadata {
	pub fn new(session: Session, albums: AlbumDetailsCache) -> Self {
		Self { session, albums }
	}

	/// A further page of a context, through its `hm://` URL.
	async fn context_page(&self, url: String) -> Result<Page, CoreError> {
		let bytes = retry_rate_limited(|| self.session.spclient().get_next_page(&url)).await?;
		Page::from_json(&bytes)
	}

	/// The extensions to ask for an item: a track's metadata comes with its loudness and its
	/// genres, in the same request.
	fn queries(item: ItemId) -> Vec<ExtensionQuery> {
		let query = |kind| ExtensionQuery {
			extension_kind: EnumOrUnknown::new(kind),
			..Default::default()
		};
		match item {
			ItemId::Track(_) => {
				vec![
					query(ExtensionKind::TRACK_V4),
					query(ExtensionKind::AUDIO_FILES),
					query(ExtensionKind::TRACK_DESCRIPTOR),
				]
			}
			ItemId::Episode(_) => vec![query(ExtensionKind::EPISODE_V4)],
		}
	}

	/// An item's info, with the URI of a track's album, whose details come in a later request.
	fn info(
		item: ItemId,
		values: Option<&ExtensionValues>,
		viewer: &Viewer,
	) -> Result<(TrackInfo, Option<String>), CoreError> {
		let values = values.ok_or_else(|| missing(item))?;
		let bytes = values.metadata.as_deref().ok_or_else(|| missing(item))?;
		let parse_error = librespot_core::Error::from;
		match item {
			ItemId::Track(_) => {
				let track =
					Track::try_from(&TrackMessage::parse_from_bytes(bytes).map_err(parse_error)?)?;
				let mut info = track_info(&track)?;
				info.unavailable = explicit_allowed(track.is_explicit, viewer.filter_explicit)
					.and_then(|()| {
						availability(&track, &viewer.country, &viewer.catalogue, &viewer.now)
					})
					.err();
				info.replay_gain = values.audio_files.as_deref().and_then(parse_audio_files);
				info.genres = values.descriptors.as_deref().map(parse_genres).unwrap_or_default();
				Ok((info, Some(track.album.id.to_uri())))
			}
			ItemId::Episode(_) => {
				let message = EpisodeMessage::parse_from_bytes(bytes).map_err(parse_error)?;
				let episode = Episode::try_from(&message)?;
				let mut info = episode_info(&episode)?;
				// `external_url` is set only for an episode Spotify doesn't host.
				let hosted = info.external_url.is_none();
				info.unavailable = explicit_allowed(episode.is_explicit, viewer.filter_explicit)
					.and_then(|()| {
						if hosted {
							episode_availability(
								&episode,
								&viewer.country,
								&viewer.catalogue,
								&viewer.now,
							)
						} else {
							Ok(())
						}
					})
					.err();
				Ok((info, None))
			}
		}
	}

	/// The details of each of `albums`, fetched once per album.
	async fn album_details(&self, albums: Vec<String>) -> HashMap<String, AlbumDetails> {
		self.albums.get_or_fetch(albums, |missing| self.fetch_album_details(missing)).await
	}

	/// The details of each of `albums`, in one request. They are a nicety: when the request
	/// fails, tracks are added without them.
	async fn fetch_album_details(&self, albums: Vec<String>) -> HashMap<String, AlbumDetails> {
		if albums.is_empty() {
			return HashMap::new();
		}
		let request = BatchedEntityRequest {
			entity_request: albums
				.into_iter()
				.map(|uri| EntityRequest {
					entity_uri: uri,
					query: vec![ExtensionQuery {
						extension_kind: EnumOrUnknown::new(ExtensionKind::ALBUM_V4),
						..Default::default()
					}],
					..Default::default()
				})
				.collect(),
			..Default::default()
		};
		let response =
			retry_rate_limited(|| self.session.spclient().get_extended_metadata(request.clone()))
				.await;
		let response = match response {
			Ok(response) => response,
			Err(error) => {
				log::debug!("no album details: {error}");
				return HashMap::new();
			}
		};
		extension_values(response)
			.into_iter()
			.filter_map(|(uri, values)| {
				let message = AlbumMessage::parse_from_bytes(values.metadata.as_deref()?).ok()?;
				Some((uri, AlbumDetails::of(&Album::try_from(&message).ok()?)))
			})
			.collect()
	}
}

impl MetadataSource for LibrespotMetadata {
	/// One extended-metadata request for the whole batch.
	async fn items(&self, items: &[ItemId]) -> Vec<Result<Arc<TrackInfo>, CoreError>> {
		if items.is_empty() {
			return Vec::new();
		}
		let request = BatchedEntityRequest {
			entity_request: items
				.iter()
				.map(|item| EntityRequest {
					entity_uri: item.uri().to_uri(),
					query: Self::queries(*item),
					..Default::default()
				})
				.collect(),
			..Default::default()
		};
		let response =
			retry_rate_limited(|| self.session.spclient().get_extended_metadata(request.clone()))
				.await;
		let response = match response {
			Ok(response) => response,
			Err(error) => {
				// Every item fails the same way; the first keeps the error itself.
				let message = error.to_string();
				let mut failed = vec![Err(CoreError::from(error))];
				failed.extend(
					items.iter().skip(1).map(|_| Err(CoreError::Unavailable(message.clone()))),
				);
				return failed;
			}
		};
		let values = extension_values(response);
		let user = self.session.user_data();
		let viewer = Viewer {
			catalogue: user
				.attributes
				.get("catalogue")
				.map_or("premium", String::as_str)
				.to_owned(),
			country: user.country,
			filter_explicit: self.session.filter_explicit_content(),
			now: Date::now_utc(),
		};
		let infos: Vec<_> = items
			.iter()
			.zip(&request.entity_request)
			.map(|(item, asked)| Self::info(*item, values.get(&asked.entity_uri), &viewer))
			.collect();
		let mut albums: Vec<String> =
			infos.iter().filter_map(|info| info.as_ref().ok()?.1.clone()).collect();
		albums.sort_unstable();
		albums.dedup();
		let details = self.album_details(albums).await;
		infos
			.into_iter()
			.map(|info| {
				let (mut info, album) = info?;
				if let Some(found) = album.and_then(|album| details.get(&album)) {
					found.apply(&mut info);
				}
				Ok(Arc::new(info))
			})
			.collect()
	}

	async fn album(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
		let uri = SpotifyUri::Album { id };
		let album = retry_rate_limited(|| Album::get(&self.session, &uri)).await?;
		let ids = album.tracks().map(SpotifyId::try_from).collect::<Result<Vec<_>, _>>()?;
		Ok((album.name, ids))
	}

	async fn playlist(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyUri>), CoreError> {
		let uri = SpotifyUri::Playlist { user: None, id };
		let playlist = retry_rate_limited(|| Playlist::get(&self.session, &uri)).await?;
		Ok((playlist.name().to_owned(), playlist.tracks().cloned().collect()))
	}

	async fn show(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
		let uri = SpotifyUri::Show { id };
		let show = retry_rate_limited(|| Show::get(&self.session, &uri)).await?;
		let ids = show.episodes.0.iter().map(SpotifyId::try_from).collect::<Result<Vec<_>, _>>()?;
		Ok((show.name, listening_order(ids, show.consumption_order)))
	}

	async fn artist(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
		let uri = SpotifyUri::Artist { id };
		let artist = retry_rate_limited(|| Artist::get(&self.session, &uri)).await?;
		// librespot falls back to the worldwide list when the country has none.
		let country = self.session.user_data().country;
		let ids = artist
			.top_tracks
			.for_country(&country)
			.0
			.iter()
			.map(SpotifyId::try_from)
			.collect::<Result<Vec<_>, _>>()?;
		Ok((artist.name, ids))
	}

	async fn liked_songs(&self) -> Result<Vec<SpotifyUri>, CoreError> {
		let uri = format!("spotify:user:{}:collection", self.session.username());
		let context = retry_rate_limited(|| self.session.spclient().get_context(&uri)).await?;
		let pages = context.pages.iter().map(Page::of).collect();
		let tracks = context_tracks(pages, |url| self.context_page(url)).await?;
		if tracks.cut_off {
			log::warn!(
				"stopped reading Liked Songs after {MAX_CONTEXT_PAGES} pages; the tracks after them weren't added"
			);
		}
		Ok(tracks.uris)
	}

	async fn search(&self, words: &str) -> Result<Vec<SpotifyUri>, CoreError> {
		let uri = Link::Search(words.to_owned()).uri();
		let context = retry_rate_limited(|| self.session.spclient().get_context(&uri)).await?;
		Ok(context.pages.first().map(|page| Page::of(page).uris()).unwrap_or_default())
	}

	async fn discography(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
		let uri = SpotifyUri::Artist { id };
		let artist = retry_rate_limited(|| Artist::get(&self.session, &uri)).await?;
		let releases = releases(&artist);
		Ok((artist.name, releases))
	}
}

/// The raw values of the extensions Spotify sent for one item.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExtensionValues {
	/// A `TRACK_V4`, `EPISODE_V4` or `ALBUM_V4` value, as the URI says: a serialized
	/// `metadata.Track`, `metadata.Episode` or `metadata.Album`.
	pub metadata: Option<Vec<u8>>,
	/// An `AUDIO_FILES` value: a serialized `AudioFilesExtensionResponse`.
	pub audio_files: Option<Vec<u8>>,
	/// A `TRACK_DESCRIPTOR` value: a serialized `ExtensionDescriptorData`.
	pub descriptors: Option<Vec<u8>>,
}

/// Sorts the values of a response by the item they belong to, keyed by its URI. Entries Spotify
/// reports as failed (a status code outside 2xx) are left out.
pub fn extension_values(response: BatchedExtensionResponse) -> HashMap<String, ExtensionValues> {
	let mut values: HashMap<String, ExtensionValues> = HashMap::new();
	for array in response.extended_metadata {
		let slot: fn(&mut ExtensionValues) -> &mut Option<Vec<u8>> =
			match array.extension_kind.enum_value() {
				Ok(
					ExtensionKind::TRACK_V4 | ExtensionKind::EPISODE_V4 | ExtensionKind::ALBUM_V4,
				) => |item| &mut item.metadata,
				Ok(ExtensionKind::AUDIO_FILES) => |item| &mut item.audio_files,
				Ok(ExtensionKind::TRACK_DESCRIPTOR) => |item| &mut item.descriptors,
				_ => continue,
			};
		for data in array.extension_data {
			let status = data.header.as_ref().map_or(200, |header| header.status_code);
			let Some(value) =
				data.extension_data.into_option().filter(|_| (200..300).contains(&status))
			else {
				continue;
			};
			*slot(values.entry(data.entity_uri).or_default()) = Some(value.value);
		}
	}
	values
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::sync::{Arc, Mutex};
	use std::time::Duration;

	use librespot_core::error::ErrorKind;

	use librespot_protocol::entity_extension_data::{
		EntityExtensionData, EntityExtensionDataHeader,
	};
	use librespot_protocol::extended_metadata::{
		BatchedExtensionResponse, EntityExtensionDataArray,
	};
	use librespot_protocol::extension_kind::ExtensionKind;
	use protobuf::well_known_types::any::Any;
	use protobuf::{EnumOrUnknown, MessageField};

	use librespot_metadata::show::ShowConsumptionOrder;

	use librespot_core::date::Date;
	use librespot_protocol::metadata::audio_file::Format;
	use librespot_protocol::metadata::{
		Album as AlbumMessage, AlbumGroup as AlbumGroupMessage, Artist as ArtistMessage,
		AudioFile as AudioFileMessage, Episode as EpisodeMessage,
		Restriction as RestrictionMessage, Track as TrackMessage,
	};
	use protobuf::{CodedOutputStream, Message};

	use super::{
		AlbumDetailsCache, ArtistTracks, CachedSource, ExtensionValues, LibrespotMetadata,
		MAX_CONTEXT_PAGES, MetadataSource, Page, SkipReason, TrackCache, Viewer, context_tracks,
		extension_values, listening_order, releases, resolve, retry_rate_limited,
	};
	use crate::link::{ItemId, Link, LinkKind};
	use crate::metadata::{AlbumDetails, EXPLICIT_FILTERED, TrackInfo};
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
			total_tracks: 0,
			total_discs: 0,
			date: None,
			duration_ms: 1000,
			covers: Vec::new(),
			has_audio: true,
			unavailable: None,
			replay_gain: None,
			genres: Vec::new(),
			description: None,
			external_url: None,
			isrc: None,
			explicit: false,
			composers: Vec::new(),
			conductors: Vec::new(),
			orchestras: Vec::new(),
			remixers: Vec::new(),
			label: None,
			copyrights: Vec::new(),
			barcode: None,
			release_type: None,
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
			total_tracks: 0,
			total_discs: 0,
			date: Some("2026-10-01".to_owned()),
			duration_ms: 1000,
			covers: Vec::new(),
			has_audio: true,
			unavailable: None,
			replay_gain: None,
			genres: Vec::new(),
			description: Some("What this episode is about.".to_owned()),
			external_url: None,
			isrc: None,
			explicit: false,
			composers: Vec::new(),
			conductors: Vec::new(),
			orchestras: Vec::new(),
			remixers: Vec::new(),
			label: None,
			copyrights: Vec::new(),
			barcode: None,
			release_type: None,
		}
	}

	#[derive(Default)]
	struct FakeSource {
		tracks: HashMap<SpotifyId, (Result<TrackInfo, &'static str>, Duration)>,
		episodes: HashMap<SpotifyId, TrackInfo>,
		albums: HashMap<SpotifyId, Result<(String, Vec<SpotifyId>), &'static str>>,
		playlists: HashMap<SpotifyId, (String, Vec<SpotifyUri>)>,
		shows: HashMap<SpotifyId, (String, Vec<SpotifyId>)>,
		artists: HashMap<SpotifyId, (String, Vec<SpotifyId>)>,
		liked: Vec<SpotifyUri>,
		searches: HashMap<String, Vec<SpotifyUri>>,
		discographies: HashMap<SpotifyId, (String, Vec<SpotifyId>)>,
		track_calls: AtomicUsize,
		/// The size of each batch asked for, in the order asked.
		batches: Mutex<Vec<usize>>,
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

	impl FakeSource {
		async fn item(&self, item: ItemId) -> Result<Arc<TrackInfo>, CoreError> {
			match item {
				ItemId::Track(id) => {
					self.track_calls.fetch_add(1, Ordering::SeqCst);
					let (result, delay) = self.tracks.get(&id).expect("unknown track");
					tokio::time::sleep(*delay).await;
					result
						.clone()
						.map(Arc::new)
						.map_err(|message| CoreError::Unavailable(message.to_owned()))
				}
				ItemId::Episode(id) => {
					tokio::task::yield_now().await;
					Ok(Arc::new(self.episodes.get(&id).expect("unknown episode").clone()))
				}
			}
		}
	}

	impl MetadataSource for FakeSource {
		async fn items(&self, items: &[ItemId]) -> Vec<Result<Arc<TrackInfo>, CoreError>> {
			self.batches.lock().unwrap().push(items.len());
			futures_util::future::join_all(items.iter().map(|item| self.item(*item))).await
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

		async fn show(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
			tokio::task::yield_now().await;
			Ok(self.shows.get(&id).expect("unknown show").clone())
		}

		async fn artist(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
			tokio::task::yield_now().await;
			Ok(self.artists.get(&id).expect("unknown artist").clone())
		}

		async fn liked_songs(&self) -> Result<Vec<SpotifyUri>, CoreError> {
			tokio::task::yield_now().await;
			Ok(self.liked.clone())
		}

		async fn search(&self, words: &str) -> Result<Vec<SpotifyUri>, CoreError> {
			tokio::task::yield_now().await;
			Ok(self.searches.get(words).cloned().unwrap_or_default())
		}

		async fn discography(&self, id: SpotifyId) -> Result<(String, Vec<SpotifyId>), CoreError> {
			tokio::task::yield_now().await;
			Ok(self.discographies.get(&id).expect("unknown discography").clone())
		}
	}

	/// Track `byte`, recorded as `isrc`.
	fn recording(byte: u8, isrc: Option<&str>) -> (Result<TrackInfo, &'static str>, Duration) {
		let mut info = info(byte);
		info.isrc = isrc.map(str::to_owned);
		(Ok(info), Duration::ZERO)
	}

	/// Keith & Kristyn Getty's releases: an album with tracks 1 and 2, a single with track 3 (the
	/// album's track 1 again), and a compilation with tracks 4 (no ISRC) and 5 (track 2 again).
	fn getty_discography() -> FakeSource {
		let mut source = FakeSource::default();
		source.tracks.insert(id(1), recording(1, Some("USY5K0600040")));
		source.tracks.insert(id(2), recording(2, Some("USY5K0600060")));
		source.tracks.insert(id(3), recording(3, Some("USY5K0600040")));
		source.tracks.insert(id(4), recording(4, None));
		source.tracks.insert(id(5), recording(5, Some("USY5K0600060")));
		source.albums.insert(id(100), Ok(("In Christ Alone".to_owned(), vec![id(1), id(2)])));
		source.albums.insert(id(101), Ok(("In Christ Alone (Single)".to_owned(), vec![id(3)])));
		source.albums.insert(id(102), Ok(("Hymns of Hope".to_owned(), vec![id(4), id(5)])));
		source
			.discographies
			.insert(id(150), ("Keith & Kristyn Getty".to_owned(), vec![id(100), id(101), id(102)]));
		source.artists.insert(id(150), ("Keith & Kristyn Getty".to_owned(), vec![id(2)]));
		source
	}

	fn artist_link() -> Link {
		Link::Id { kind: LinkKind::Artist, id: id(150) }
	}

	#[tokio::test]
	async fn discography_adds_each_release_in_order_and_each_recording_once() {
		let source = getty_discography();
		let resolved = resolve(&source, &artist_link(), ArtistTracks::Discography).await.unwrap();
		assert_eq!(resolved.name.as_deref(), Some("Keith & Kristyn Getty"));
		assert_eq!(titles(&resolved.tracks), ["Track 1", "Track 2", "Track 4"]);
		assert_eq!(resolved.duplicates, 2);
		assert!(resolved.skipped.is_empty());
	}

	#[tokio::test]
	async fn popular_tracks_when_chosen_for_artist_links() {
		let source = getty_discography();
		let resolved = resolve(&source, &artist_link(), ArtistTracks::Popular).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 2"]);
		assert_eq!(resolved.duplicates, 0);
	}

	#[tokio::test]
	async fn an_album_copy_that_cant_play_leaves_the_single_copy() {
		let mut source = getty_discography();
		let mut blocked = info(1);
		blocked.isrc = Some("USY5K0600040".to_owned());
		blocked.unavailable = Some("not available in your country (NL)".to_owned());
		source.tracks.insert(id(1), (Ok(blocked), Duration::ZERO));
		let resolved = resolve(&source, &artist_link(), ArtistTracks::Discography).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 2", "Track 3", "Track 4"]);
		assert_eq!(resolved.skipped.len(), 1);
		assert_eq!(resolved.duplicates, 1);
	}

	#[tokio::test]
	async fn a_release_that_fails_is_skipped_with_its_reason() {
		let mut source = getty_discography();
		source.albums.insert(id(101), Err("Spotify sent no metadata for this album"));
		let resolved = resolve(&source, &artist_link(), ArtistTracks::Discography).await.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1", "Track 2", "Track 4"]);
		assert_eq!(resolved.skipped.len(), 1);
		assert_eq!(resolved.skipped[0].uri, SpotifyUri::Album { id: id(101) }.to_uri());
	}

	#[tokio::test]
	async fn a_discography_whose_releases_all_fail_is_the_first_error() {
		let mut source = getty_discography();
		source.albums.insert(id(100), Err("first"));
		source.albums.insert(id(101), Err("second"));
		source.albums.insert(id(102), Err("third"));
		let result = resolve(&source, &artist_link(), ArtistTracks::Discography).await;
		assert!(matches!(&result, Err(CoreError::Unavailable(message)) if message == "first"));
	}

	fn group(bytes: &[u8]) -> AlbumGroupMessage {
		let mut group = AlbumGroupMessage::new();
		group.album = bytes
			.iter()
			.map(|&byte| {
				let mut album = AlbumMessage::new();
				album.set_gid(vec![byte; 16]);
				album
			})
			.collect();
		group
	}

	#[test]
	fn releases_come_albums_first_then_singles_then_compilations_without_appearances() {
		let mut message = ArtistMessage::new();
		message.set_gid(vec![150; 16]);
		message.album_group = vec![group(&[100]), group(&[103, 104])];
		message.single_group = vec![group(&[101])];
		message.compilation_group = vec![group(&[102])];
		message.appears_on_group = vec![group(&[105])];
		let artist = librespot_metadata::Artist::try_from(&message).unwrap();
		assert_eq!(releases(&artist), [id(100), id(103), id(101), id(102)]);
	}

	#[tokio::test]
	async fn search_link_adds_what_spotify_finds_named_after_the_words() {
		let mut source = FakeSource::with_tracks(&[1, 2]);
		source.searches.insert(
			"in christ alone".to_owned(),
			vec![SpotifyUri::Track { id: id(1) }, SpotifyUri::Track { id: id(2) }],
		);
		let link = Link::Search("in christ alone".to_owned());
		let resolved = resolve(&source, &link, ArtistTracks::Popular).await.unwrap();
		assert_eq!(resolved.name.as_deref(), Some("in christ alone"));
		assert_eq!(titles(&resolved.tracks), ["Track 1", "Track 2"]);
	}

	fn titles(tracks: &[Arc<TrackInfo>]) -> Vec<&str> {
		tracks.iter().map(|track| track.title.as_str()).collect()
	}

	fn track_uri(byte: u8) -> SpotifyUri {
		SpotifyUri::Track { id: id(byte) }
	}

	#[tokio::test]
	async fn liked_songs_link_adds_the_saved_tracks_in_order() {
		let mut source = FakeSource::with_tracks(&[1, 2]);
		source.liked = vec![
			track_uri(2),
			SpotifyUri::from_uri("spotify:local:Keith+Getty::In+Christ+Alone:279").unwrap(),
			track_uri(1),
		];
		let resolved = resolve(&source, &Link::LikedSongs, ArtistTracks::Popular).await.unwrap();
		assert_eq!(resolved.name.as_deref(), Some("Liked Songs"));
		assert_eq!(titles(&resolved.tracks), ["Track 2", "Track 1"]);
		assert_eq!(resolved.skipped.len(), 1);
	}

	fn page(tracks: &[u8], page_url: Option<&str>, next_page_url: Option<&str>) -> Page {
		Page {
			tracks: tracks.iter().map(|&byte| track_uri(byte).to_uri()).collect(),
			tracks_url: page_url.map(str::to_owned),
			next_url: next_page_url.map(str::to_owned),
		}
	}

	#[tokio::test]
	async fn context_pages_are_followed_in_order() {
		let asked = Mutex::new(Vec::new());
		let fetch = |url: String| {
			asked.lock().unwrap().push(url.clone());
			async move {
				Ok::<_, CoreError>(match url.as_str() {
					"hm://page/2" => page(&[2], None, Some("hm://page/3")),
					"hm://page/3" => page(&[3], None, None),
					_ => panic!("unexpected {url}"),
				})
			}
		};
		let pages = vec![
			page(&[1], None, None),
			page(&[], Some("hm://page/2"), None),
			page(&[4], None, None),
		];
		let tracks = context_tracks(pages, fetch).await.unwrap();
		assert_eq!(tracks.uris, [track_uri(1), track_uri(2), track_uri(3), track_uri(4)]);
		assert!(!tracks.cut_off);
		assert_eq!(*asked.lock().unwrap(), ["hm://page/2", "hm://page/3"]);
	}

	#[tokio::test]
	async fn context_paging_stops_after_its_limit_and_says_so() {
		let calls = AtomicUsize::new(0);
		let endless = |_: String| {
			calls.fetch_add(1, Ordering::SeqCst);
			async { Ok::<_, CoreError>(page(&[1], None, Some("hm://again"))) }
		};
		let tracks =
			context_tracks(vec![page(&[1], None, Some("hm://again"))], endless).await.unwrap();
		assert_eq!(calls.load(Ordering::SeqCst), MAX_CONTEXT_PAGES);
		assert_eq!(tracks.uris.len(), MAX_CONTEXT_PAGES + 1);
		assert!(tracks.cut_off);
	}

	#[test]
	fn a_page_reads_from_json_in_either_spelling() {
		let uri = track_uri(1).to_uri();
		let camel = format!(r#"{{"tracks":[{{"uri":"{uri}"}}],"nextPageUrl":"hm://next"}}"#);
		assert_eq!(Page::from_json(camel.as_bytes()).unwrap(), page(&[1], None, Some("hm://next")));
		let snake = r#"{"tracks":[],"page_url":"hm://tracks","next_page_url":""}"#;
		assert_eq!(
			Page::from_json(snake.as_bytes()).unwrap(),
			page(&[], Some("hm://tracks"), None)
		);
		assert!(Page::from_json(b"not json").is_err());
	}

	#[tokio::test]
	async fn artist_link_adds_the_popular_tracks_in_order() {
		let mut source = FakeSource::with_tracks(&[1, 2, 3]);
		source.artists.insert(id(150), ("Keith & Kristyn Getty".to_owned(), vec![id(3), id(1)]));
		let link = Link::Id { kind: LinkKind::Artist, id: id(150) };
		let resolved = resolve(&source, &link, ArtistTracks::Popular).await.unwrap();
		assert_eq!(resolved.name.as_deref(), Some("Keith & Kristyn Getty"));
		assert_eq!(titles(&resolved.tracks), ["Track 3", "Track 1"]);
	}

	#[tokio::test]
	async fn track_link_resolves_to_one_track() {
		let source = FakeSource::with_tracks(&[1]);
		let resolved =
			resolve(&source, &Link::Id { kind: LinkKind::Track, id: id(1) }, ArtistTracks::Popular)
				.await
				.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1"]);
		assert_eq!(resolved.name, None);
		assert!(resolved.skipped.is_empty());
	}

	#[tokio::test]
	async fn episode_link_resolves_to_one_episode() {
		let mut source = FakeSource::default();
		source.episodes.insert(id(50), episode(50));
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Episode, id: id(50) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
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
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Show, id: id(250) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Episode 52", "Episode 50", "Episode 51"]);
		assert_eq!(resolved.name.as_deref(), Some("The Show"));
	}

	#[tokio::test]
	async fn a_long_show_is_fetched_in_batches_of_100_and_kept_in_order() {
		let mut source = FakeSource::default();
		for byte in 1..=250u8 {
			source.episodes.insert(id(byte), episode(byte));
		}
		source.shows.insert(id(255), ("The Show".to_owned(), (1..=250u8).map(id).collect()));
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Show, id: id(255) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
		assert_eq!(resolved.tracks.len(), 250);
		assert_eq!(resolved.tracks[0].title, "Episode 1");
		assert_eq!(resolved.tracks[249].title, "Episode 250");
		let mut batches = source.batches.lock().unwrap().clone();
		batches.sort_unstable();
		assert_eq!(batches, [50, 100, 100]);
	}

	#[test]
	fn a_show_meant_to_be_heard_from_the_start_lists_its_first_episode_first() {
		// Spotify lists every show's episodes newest first.
		let newest_first = vec![id(3), id(2), id(1)];
		assert_eq!(
			listening_order(newest_first.clone(), ShowConsumptionOrder::SEQUENTIAL),
			[id(1), id(2), id(3)]
		);
		assert_eq!(
			listening_order(newest_first.clone(), ShowConsumptionOrder::RECENT),
			newest_first
		);
		assert_eq!(
			listening_order(newest_first.clone(), ShowConsumptionOrder::EPISODIC),
			newest_first
		);
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
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Show, id: id(250) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Episode 50"]);
		assert_eq!(resolved.skipped.len(), 1);
		assert_eq!(resolved.skipped[0].uri, SpotifyUri::Episode { id: id(51) }.to_uri());
	}

	#[tokio::test]
	async fn track_link_failure_is_an_error() {
		let mut source = FakeSource::default();
		source.tracks.insert(id(1), (Err("gone"), Duration::ZERO));
		let result =
			resolve(&source, &Link::Id { kind: LinkKind::Track, id: id(1) }, ArtistTracks::Popular)
				.await;
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
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Album, id: id(100) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
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
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Playlist, id: id(200) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
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
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Playlist, id: id(200) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
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
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Album, id: id(100) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
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
		let resolved = resolve(
			&source,
			&Link::Id { kind: LinkKind::Album, id: id(100) },
			ArtistTracks::Popular,
		)
		.await
		.unwrap();
		assert_eq!(titles(&resolved.tracks), ["Track 1"]);
		assert_eq!(
			resolved.skipped[0].reason,
			SkipReason::Unavailable("not available in your country".to_owned())
		);
	}

	#[tokio::test]
	async fn an_unavailable_track_is_asked_for_again_later() {
		let mut inner = FakeSource::with_tracks(&[1]);
		let mut unreleased = info(2);
		unreleased.unavailable = Some("not released on Spotify yet".to_owned());
		inner.tracks.insert(id(2), (Ok(unreleased), Duration::ZERO));
		inner.albums.insert(id(100), Ok(("The Album".to_owned(), vec![id(1), id(2)])));
		let source = CachedSource::new(inner, TrackCache::default());
		let link = Link::Id { kind: LinkKind::Album, id: id(100) };
		resolve(&source, &link, ArtistTracks::Popular).await.unwrap();
		resolve(&source, &link, ArtistTracks::Popular).await.unwrap();
		// Track 1 once; track 2 each time, since its release or the country may change.
		assert_eq!(source.inner().track_calls.load(Ordering::SeqCst), 3);
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

	const TRACK_URI: &str = "spotify:track:2hQxLiI93xdjqENJuKeXjO";
	const EPISODE_URI: &str = "spotify:episode:4QomYTPgZ1mpVbUmxBsUpz";

	#[test]
	fn extension_values_finds_track_and_audio_files() {
		let response = BatchedExtensionResponse {
			extended_metadata: vec![
				extension(ExtensionKind::AUDIO_FILES, 200, b"loudness"),
				extension(ExtensionKind::TRACK_V4, 200, b"track"),
			],
			..Default::default()
		};
		let values = &extension_values(response)[TRACK_URI];
		assert_eq!(values.metadata.as_deref(), Some(&b"track"[..]));
		assert_eq!(values.audio_files.as_deref(), Some(&b"loudness"[..]));
	}

	#[test]
	fn descriptor_values_are_kept_apart() {
		let response = BatchedExtensionResponse {
			extended_metadata: vec![
				extension(ExtensionKind::TRACK_V4, 200, b"track"),
				extension(ExtensionKind::TRACK_DESCRIPTOR, 200, b"genres"),
			],
			..Default::default()
		};
		let values = &extension_values(response)[TRACK_URI];
		assert_eq!(values.metadata.as_deref(), Some(&b"track"[..]));
		assert_eq!(values.descriptors.as_deref(), Some(&b"genres"[..]));
	}

	#[test]
	fn a_track_is_asked_for_with_its_loudness_and_genres() {
		let kinds: Vec<_> = LibrespotMetadata::queries(ItemId::Track(id(1)))
			.iter()
			.map(|query| query.extension_kind.enum_value())
			.collect();
		assert_eq!(
			kinds,
			[
				Ok(ExtensionKind::TRACK_V4),
				Ok(ExtensionKind::AUDIO_FILES),
				Ok(ExtensionKind::TRACK_DESCRIPTOR)
			]
		);
	}

	/// A `TRACK_DESCRIPTOR` value with one genre.
	fn genre_descriptor(text: &str, weight: f32) -> Vec<u8> {
		let mut descriptor = Vec::new();
		let mut out = CodedOutputStream::vec(&mut descriptor);
		out.write_string(1, text).unwrap();
		out.write_float(2, weight).unwrap();
		out.write_bytes(3, &[1]).unwrap();
		out.flush().unwrap();
		drop(out);
		let mut bytes = Vec::new();
		let mut out = CodedOutputStream::vec(&mut bytes);
		out.write_bytes(1, &descriptor).unwrap();
		out.flush().unwrap();
		drop(out);
		bytes
	}

	#[test]
	fn a_track_takes_its_genres_from_its_descriptors() {
		let mut album = AlbumMessage::new();
		album.set_gid(vec![0x20; 16]);
		let mut track = TrackMessage::new();
		track.set_gid(vec![0x10; 16]);
		track.album = MessageField::some(album);
		let values = ExtensionValues {
			metadata: Some(track.write_to_bytes().unwrap()),
			descriptors: Some(genre_descriptor("gospel", 0.9)),
			..Default::default()
		};
		let item = ItemId::Track(SpotifyId::from_raw(&[0x10; 16]).unwrap());
		let (info, _) = LibrespotMetadata::info(item, Some(&values), &viewer(false)).unwrap();
		assert_eq!(info.genres, ["Gospel"]);
	}

	fn viewer(filter_explicit: bool) -> Viewer {
		Viewer {
			country: "NL".to_owned(),
			catalogue: "premium".to_owned(),
			filter_explicit,
			now: Date::now_utc(),
		}
	}

	fn explicit_track() -> TrackMessage {
		let mut album = AlbumMessage::new();
		album.set_gid(vec![0x20; 16]);
		let mut track = TrackMessage::new();
		track.set_gid(vec![0x10; 16]);
		track.album = MessageField::some(album);
		track.set_explicit(true);
		track
	}

	fn explicit_episode() -> EpisodeMessage {
		let mut episode = EpisodeMessage::new();
		episode.set_gid(vec![0x50; 16]);
		episode.set_explicit(true);
		let mut file = AudioFileMessage::new();
		file.set_file_id(vec![7; 20]);
		file.set_format(Format::OGG_VORBIS_96);
		episode.audio = vec![file];
		episode
	}

	/// Why `info` finds `message` unavailable for an account in the Netherlands.
	fn unavailable(item: ItemId, message: &impl Message, filter_explicit: bool) -> Option<String> {
		let values = ExtensionValues {
			metadata: Some(message.write_to_bytes().unwrap()),
			..Default::default()
		};
		let (info, _) =
			LibrespotMetadata::info(item, Some(&values), &viewer(filter_explicit)).unwrap();
		info.unavailable
	}

	#[test]
	fn an_explicit_track_is_skipped_when_the_account_filters_explicit_content() {
		let track = ItemId::Track(id(0x10));
		assert_eq!(unavailable(track, &explicit_track(), true).as_deref(), Some(EXPLICIT_FILTERED));
		assert_eq!(unavailable(track, &explicit_track(), false), None);
	}

	#[test]
	fn the_explicit_filter_is_named_before_a_country_restriction() {
		let mut restriction = RestrictionMessage::new();
		restriction.catalogue_str = vec!["premium".to_owned()];
		restriction.set_countries_allowed("US".to_owned());
		let mut message = explicit_track();
		message.restriction = vec![restriction];
		let track = ItemId::Track(id(0x10));
		assert_eq!(unavailable(track, &message, true).as_deref(), Some(EXPLICIT_FILTERED));
		let reason = unavailable(track, &message, false);
		assert!(reason.as_deref().is_some_and(|reason| reason.contains("country")), "{reason:?}");
	}

	#[test]
	fn an_explicit_episode_spotify_hosts_is_skipped_with_the_filter() {
		let episode = ItemId::Episode(id(0x50));
		assert_eq!(
			unavailable(episode, &explicit_episode(), true).as_deref(),
			Some(EXPLICIT_FILTERED)
		);
		assert_eq!(unavailable(episode, &explicit_episode(), false), None);
	}

	#[test]
	fn an_explicit_episode_hosted_elsewhere_is_skipped_with_the_filter() {
		let mut message = explicit_episode();
		message.audio.clear();
		message.set_external_url("https://podcasts.example/12.mp3".to_owned());
		let episode = ItemId::Episode(id(0x50));
		assert_eq!(unavailable(episode, &message, true).as_deref(), Some(EXPLICIT_FILTERED));
		assert_eq!(unavailable(episode, &message, false), None);
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
		let values = &extension_values(response)[TRACK_URI];
		assert_eq!(values.metadata.as_deref(), Some(&b"track"[..]));
		assert_eq!(values.audio_files, None);
	}

	#[test]
	fn album_values_are_kept_by_album() {
		let mut albums = extension(ExtensionKind::ALBUM_V4, 200, b"album");
		albums.extension_data[0].entity_uri = "spotify:album:4wPtS24asgBAG4VOmYYB9O".to_owned();
		let response =
			BatchedExtensionResponse { extended_metadata: vec![albums], ..Default::default() };
		let values = extension_values(response);
		assert_eq!(
			values["spotify:album:4wPtS24asgBAG4VOmYYB9O"].metadata.as_deref(),
			Some(&b"album"[..])
		);
	}

	#[test]
	fn extension_values_are_sorted_by_item() {
		let mut tracks = extension(ExtensionKind::TRACK_V4, 200, b"track");
		let mut episode = tracks.extension_data[0].clone();
		episode.entity_uri = EPISODE_URI.to_owned();
		tracks.extension_data.push(episode);
		let mut episodes = extension(ExtensionKind::EPISODE_V4, 200, b"episode");
		episodes.extension_data[0].entity_uri = EPISODE_URI.to_owned();
		let response = BatchedExtensionResponse {
			extended_metadata: vec![tracks, episodes],
			..Default::default()
		};
		let values = extension_values(response);
		assert_eq!(values[TRACK_URI].metadata.as_deref(), Some(&b"track"[..]));
		assert_eq!(values[EPISODE_URI].metadata.as_deref(), Some(&b"episode"[..]));
	}

	fn rate_limited() -> librespot_core::Error {
		librespot_core::Error::resource_exhausted("rate limited for at least another 0 seconds")
	}

	#[tokio::test(start_paused = true)]
	async fn a_request_librespot_rate_limits_is_tried_again() {
		let calls = AtomicUsize::new(0);
		let result = retry_rate_limited(|| async {
			if calls.fetch_add(1, Ordering::SeqCst) < 2 { Err(rate_limited()) } else { Ok(7) }
		})
		.await;
		assert_eq!(result.unwrap(), 7);
		assert_eq!(calls.load(Ordering::SeqCst), 3);
	}

	#[tokio::test(start_paused = true)]
	async fn other_failures_and_lasting_rate_limits_are_given_up_on() {
		let calls = AtomicUsize::new(0);
		let result: Result<(), _> = retry_rate_limited(|| async {
			calls.fetch_add(1, Ordering::SeqCst);
			Err(librespot_core::Error::unavailable("gone"))
		})
		.await;
		assert_eq!(result.unwrap_err().kind, ErrorKind::Unavailable);
		assert_eq!(calls.load(Ordering::SeqCst), 1);

		let started = tokio::time::Instant::now();
		let result: Result<(), _> = retry_rate_limited(|| async { Err(rate_limited()) }).await;
		assert_eq!(result.unwrap_err().kind, ErrorKind::ResourceExhausted);
		assert!(started.elapsed() >= Duration::from_secs(30), "{:?}", started.elapsed());
	}

	fn some_details() -> AlbumDetails {
		let mut album = AlbumMessage::new();
		album.set_gid(vec![0x20; 16]);
		AlbumDetails::of(&librespot_metadata::Album::try_from(&album).unwrap())
	}

	#[tokio::test]
	async fn album_details_are_asked_for_once_per_album() {
		let cache = AlbumDetailsCache::default();
		let asked = Mutex::new(Vec::new());
		let fetch = |albums: Vec<String>| {
			asked.lock().unwrap().push(albums.clone());
			async move { albums.into_iter().map(|album| (album, some_details())).collect() }
		};
		let first = cache.get_or_fetch(vec!["a".to_owned(), "b".to_owned()], fetch).await;
		let second = cache.get_or_fetch(vec!["b".to_owned(), "c".to_owned()], fetch).await;
		let third = cache.get_or_fetch(vec!["a".to_owned()], fetch).await;
		assert_eq!((first.len(), second.len(), third.len()), (2, 2, 1));
		assert_eq!(*asked.lock().unwrap(), [vec!["a", "b"], vec!["c"]]);
	}

	#[tokio::test]
	async fn album_details_that_failed_are_asked_for_again() {
		let cache = AlbumDetailsCache::default();
		let calls = AtomicUsize::new(0);
		let failing = |_: Vec<String>| {
			calls.fetch_add(1, Ordering::SeqCst);
			async { HashMap::new() }
		};
		cache.get_or_fetch(vec!["a".to_owned()], failing).await;
		cache.get_or_fetch(vec!["a".to_owned()], failing).await;
		assert_eq!(calls.load(Ordering::SeqCst), 2);
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
		let result = resolve(
			&source,
			&Link::Id { kind: LinkKind::Album, id: id(100) },
			ArtistTracks::Popular,
		)
		.await;
		assert!(matches!(&result, Err(CoreError::Unavailable(message)) if message == "first"));
	}

	#[tokio::test]
	async fn album_fetch_failure_is_an_error() {
		let mut source = FakeSource::default();
		source.albums.insert(id(100), Err("no such album"));
		let result = resolve(
			&source,
			&Link::Id { kind: LinkKind::Album, id: id(100) },
			ArtistTracks::Popular,
		)
		.await;
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
		let link = Link::Id { kind: LinkKind::Album, id: id(100) };
		resolve(&source, &link, ArtistTracks::Popular).await.unwrap();
		let again = resolve(&source, &link, ArtistTracks::Popular).await.unwrap();
		assert_eq!(again.tracks.len(), 3);
		assert_eq!(source.inner().track_calls.load(Ordering::SeqCst), 3);
		assert_eq!(
			cache.get(&ItemId::Track(id(2))).map(|track| track.title.clone()).as_deref(),
			Some("Track 2")
		);
	}
}
