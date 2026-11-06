//! Turning a track, album, playlist, episode or show link into the tracks and episodes to add
//! to a foobar2000 playlist.

use std::collections::HashMap;
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
	AlbumTotals, TrackInfo, availability, episode_availability, episode_info, explicit_allowed,
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
	/// The album, playlist or show name; `None` for a track or episode link.
	pub name: Option<String>,
	pub tracks: Vec<Arc<TrackInfo>>,
	pub skipped: Vec<Skipped>,
}

/// Where track, episode, album, playlist and show metadata comes from.
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
pub async fn resolve<S: MetadataSource>(source: &S, link: &Link) -> Result<Resolved, CoreError> {
	let (name, entries) = match link.kind {
		LinkKind::Track | LinkKind::Episode => {
			(None, link.item().map(Entry::Item).into_iter().collect::<Vec<_>>())
		}
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
		LinkKind::Artist => {
			let (name, ids) = source.artist(link.id).await?;
			(Some(name), ids.into_iter().map(|id| Entry::Item(ItemId::Track(id))).collect())
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

	let mut resolved = Resolved { name, tracks: Vec::new(), skipped: Vec::new() };
	let mut first_error = None;
	for entry in entries {
		let item = match entry {
			Entry::Skip(skipped) => {
				resolved.skipped.push(skipped);
				continue;
			}
			Entry::Item(item) => item,
		};
		match fetched.next().unwrap_or_else(|| Err(missing(item))) {
			Ok(info) if playable(&info) => resolved.tracks.push(info),
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

/// How albums are split into discs, fetched this run. That doesn't change, and without this
/// every track read one at a time, such as from an old foo_spotify playlist, would cost a second
/// Spotify request.
#[derive(Clone, Debug, Default)]
pub struct AlbumTotalsCache(Arc<Mutex<HashMap<String, AlbumTotals>>>);

impl AlbumTotalsCache {
	/// The totals of `albums`, asking `fetch` only for the albums not seen before. Albums `fetch`
	/// has no totals for are asked for again next time.
	pub async fn get_or_fetch<F, Fut>(
		&self,
		albums: Vec<String>,
		fetch: F,
	) -> HashMap<String, AlbumTotals>
	where
		F: FnOnce(Vec<String>) -> Fut,
		Fut: Future<Output = HashMap<String, AlbumTotals>>,
	{
		let (mut found, missing) = {
			let known = self.0.lock().unwrap_or_else(PoisonError::into_inner);
			let mut found = HashMap::new();
			let mut missing = Vec::new();
			for album in albums {
				match known.get(&album) {
					Some(totals) => {
						found.insert(album, totals.clone());
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
}

/// Metadata straight from Spotify, through librespot.
pub struct LibrespotMetadata {
	session: Session,
	totals: AlbumTotalsCache,
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
	pub fn new(session: Session, totals: AlbumTotalsCache) -> Self {
		Self { session, totals }
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

	/// An item's info, with the URI of a track's album, whose totals come in a later request.
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

	/// How each of `albums` is split into discs, fetched once per album.
	async fn album_totals(&self, albums: Vec<String>) -> HashMap<String, AlbumTotals> {
		self.totals.get_or_fetch(albums, |missing| self.fetch_album_totals(missing)).await
	}

	/// How each of `albums` is split into discs, in one request. The totals are a nicety: when
	/// the request fails, tracks are added without them.
	async fn fetch_album_totals(&self, albums: Vec<String>) -> HashMap<String, AlbumTotals> {
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
				log::debug!("no album totals: {error}");
				return HashMap::new();
			}
		};
		extension_values(response)
			.into_iter()
			.filter_map(|(uri, values)| {
				let message = AlbumMessage::parse_from_bytes(values.metadata.as_deref()?).ok()?;
				Some((uri, AlbumTotals::of(&Album::try_from(&message).ok()?)))
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
		let totals = self.album_totals(albums).await;
		infos
			.into_iter()
			.map(|info| {
				let (mut info, album) = info?;
				if let Some(totals) = album.and_then(|album| totals.get(&album)) {
					totals.apply(&mut info);
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
		Album as AlbumMessage, AudioFile as AudioFileMessage, Episode as EpisodeMessage,
		Restriction as RestrictionMessage, Track as TrackMessage,
	};
	use protobuf::{CodedOutputStream, Message};

	use super::{
		AlbumTotalsCache, CachedSource, ExtensionValues, LibrespotMetadata, MetadataSource,
		SkipReason, TrackCache, Viewer, extension_values, listening_order, resolve,
		retry_rate_limited,
	};
	use crate::link::{ItemId, Link, LinkKind};
	use crate::metadata::{AlbumTotals, EXPLICIT_FILTERED, TrackInfo};
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
	}

	fn titles(tracks: &[Arc<TrackInfo>]) -> Vec<&str> {
		tracks.iter().map(|track| track.title.as_str()).collect()
	}

	#[tokio::test]
	async fn artist_link_adds_the_popular_tracks_in_order() {
		let mut source = FakeSource::with_tracks(&[1, 2, 3]);
		source.artists.insert(id(150), ("Keith & Kristyn Getty".to_owned(), vec![id(3), id(1)]));
		let link = Link { kind: LinkKind::Artist, id: id(150) };
		let resolved = resolve(&source, &link).await.unwrap();
		assert_eq!(resolved.name.as_deref(), Some("Keith & Kristyn Getty"));
		assert_eq!(titles(&resolved.tracks), ["Track 3", "Track 1"]);
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
	async fn a_long_show_is_fetched_in_batches_of_100_and_kept_in_order() {
		let mut source = FakeSource::default();
		for byte in 1..=250u8 {
			source.episodes.insert(id(byte), episode(byte));
		}
		source.shows.insert(id(255), ("The Show".to_owned(), (1..=250u8).map(id).collect()));
		let resolved = resolve(&source, &Link { kind: LinkKind::Show, id: id(255) }).await.unwrap();
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

	#[tokio::test]
	async fn an_unavailable_track_is_asked_for_again_later() {
		let mut inner = FakeSource::with_tracks(&[1]);
		let mut unreleased = info(2);
		unreleased.unavailable = Some("not released on Spotify yet".to_owned());
		inner.tracks.insert(id(2), (Ok(unreleased), Duration::ZERO));
		inner.albums.insert(id(100), Ok(("The Album".to_owned(), vec![id(1), id(2)])));
		let source = CachedSource::new(inner, TrackCache::default());
		let link = Link { kind: LinkKind::Album, id: id(100) };
		resolve(&source, &link).await.unwrap();
		resolve(&source, &link).await.unwrap();
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

	fn some_totals() -> AlbumTotals {
		let mut album = AlbumMessage::new();
		album.set_gid(vec![0x20; 16]);
		AlbumTotals::of(&librespot_metadata::Album::try_from(&album).unwrap())
	}

	#[tokio::test]
	async fn album_totals_are_asked_for_once_per_album() {
		let cache = AlbumTotalsCache::default();
		let asked = Mutex::new(Vec::new());
		let fetch = |albums: Vec<String>| {
			asked.lock().unwrap().push(albums.clone());
			async move { albums.into_iter().map(|album| (album, some_totals())).collect() }
		};
		let first = cache.get_or_fetch(vec!["a".to_owned(), "b".to_owned()], fetch).await;
		let second = cache.get_or_fetch(vec!["b".to_owned(), "c".to_owned()], fetch).await;
		let third = cache.get_or_fetch(vec!["a".to_owned()], fetch).await;
		assert_eq!((first.len(), second.len(), third.len()), (2, 2, 1));
		assert_eq!(*asked.lock().unwrap(), [vec!["a", "b"], vec!["c"]]);
	}

	#[tokio::test]
	async fn album_totals_that_failed_are_asked_for_again() {
		let cache = AlbumTotalsCache::default();
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
