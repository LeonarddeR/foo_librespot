//! The logged-in account's playlists, the items of the ones it can change, and its Liked Songs.
//! A background fetch fills a shared cache, and readers take what it holds at that moment.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use futures_util::future::{self, Either};
use futures_util::stream::{self, StreamExt};
use http::{HeaderMap, HeaderValue, Method, header::CONTENT_TYPE};
use librespot_core::Session;
use librespot_protocol::playlist4_external::op::Kind as OpKind;
use librespot_protocol::playlist4_external::source_info::Client as SourceClient;
use librespot_protocol::playlist4_external::{
	Add, ChangeInfo, Delta, Item, ItemAttributes, ListChanges, Op, Rem, SelectedListContent,
	SourceInfo,
};
use protobuf::{CodedOutputStream, EnumOrUnknown, Message, MessageField};

use crate::link::{ItemId, Link, LinkKind, decode_search, parse_item};
use crate::resolve::{liked_song_uris, retry_rate_limited};
use crate::{CoreError, SpotifyId, SpotifyUri};

/// The name Liked Songs goes by in menus.
pub const LIKED_SONGS: &str = "Liked Songs";

/// A library fetched longer ago than this is read again when a menu next needs it.
pub(crate) const LIBRARY_REFRESH: Duration = Duration::from_secs(60);

/// How long a fetch may run before the caller gives up on it.
pub(crate) const LIBRARY_FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a fetch that ran out of time failed.
pub(crate) const SLOW_REASON: &str = "Spotify took too long to send your playlists";

/// How long an add or remove may wait for Spotify before it fails.
pub(crate) const LIBRARY_CHANGE_TIMEOUT: Duration = Duration::from_secs(60);

/// Why a change that ran out of time failed.
const CHANGE_SLOW_REASON: &str = "Spotify didn't answer in time";

/// Runs `change` for at most [`LIBRARY_CHANGE_TIMEOUT`]; when the time runs out, `change` is
/// dropped and the answer is [`CoreError::Unavailable`] with [`CHANGE_SLOW_REASON`].
async fn within_change_limit<T>(
	change: impl Future<Output = Result<T, CoreError>>,
) -> Result<T, CoreError> {
	tokio::time::timeout(LIBRARY_CHANGE_TIMEOUT, change)
		.await
		.unwrap_or_else(|_| Err(CoreError::Unavailable(CHANGE_SLOW_REASON.to_owned())))
}

/// How many playlists are read at once.
const PLAYLIST_CONCURRENCY: usize = 4;

/// How many playlists one request for the playlist list asks for.
const ROOTLIST_PAGE: usize = 500;

const PLAYLIST_URI: &str = "spotify:playlist:";
const START_GROUP_URI: &str = "spotify:start-group:";
const END_GROUP_URI: &str = "spotify:end-group:";

/// One playlist in the account's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryPlaylist {
	pub id: SpotifyId,
	pub name: String,
	/// The folders it sits in, outermost first.
	pub folders: Vec<String>,
	/// The account may add and remove its items: its own playlists and collaborative ones.
	pub can_edit: bool,
	pub revision: Vec<u8>,
}

impl LibraryPlaylist {
	/// The name with its folders in front, such as "Church / Sunday set".
	pub fn display_name(&self) -> String {
		self.folders
			.iter()
			.map(String::as_str)
			.chain([self.name.as_str()])
			.collect::<Vec<_>>()
			.join(" / ")
	}

	pub const fn link(&self) -> Link {
		Link::Id { kind: LinkKind::Playlist, id: self.id }
	}
}

/// One row of a playlist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistRow {
	pub uri: String,
	/// What Spotify calls the row, to remove it by.
	pub item_id: Vec<u8>,
}

/// The rows of a playlist, as of one revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistContents {
	pub revision: Vec<u8>,
	pub rows: Vec<PlaylistRow>,
	/// Spotify sent only some of the rows.
	pub truncated: bool,
}

/// Orders the reads and writes of the library: each takes the next one from one counter when
/// it starts.
type Stamp = u64;

/// Whether a store with `stamp` replaces an entry stored with `held`, or `None` when there is
/// none: only a newer stamp at or above `floor` does.
fn lands(stamp: Stamp, held: Option<Stamp>, floor: Stamp) -> bool {
	stamp >= floor && held.is_none_or(|held| stamp > held)
}

/// A playlist's contents with the set of the tracks and episodes it holds.
#[derive(Debug)]
struct Held {
	contents: PlaylistContents,
	/// Rows whose URI names no track or episode are left out.
	items: HashSet<ItemId>,
	/// The stamp of the read that produced `contents`.
	stamp: Stamp,
}

impl Held {
	fn new(contents: PlaylistContents, stamp: Stamp) -> Self {
		let items = contents.rows.iter().filter_map(|row| parse_item(&row.uri)).collect();
		Self { contents, items, stamp }
	}
}

/// What was read from the account. A clone shares the items and Liked Songs it holds.
#[derive(Clone, Debug)]
pub struct Library {
	pub playlists: Vec<LibraryPlaylist>,
	items: HashMap<SpotifyId, Arc<Held>>,
	/// The playlists without items whose read failed, or hadn't finished, when the last fetch
	/// that read the list ended.
	unreadable: HashSet<SpotifyId>,
	liked: Option<Arc<HashSet<ItemId>>>,
	/// The stamp of the read or write that produced `liked`.
	liked_stamp: Stamp,
}

impl Library {
	/// A library of `playlists` that keeps what `previous` knew of them: the items of each
	/// playlist, whatever their revision, which ones couldn't be read, and Liked Songs.
	fn new(playlists: Vec<LibraryPlaylist>, previous: Option<&Self>) -> Self {
		let mut library = Self {
			playlists,
			items: HashMap::new(),
			unreadable: HashSet::new(),
			liked: previous.and_then(|previous| previous.liked.clone()),
			liked_stamp: previous.map_or(0, |previous| previous.liked_stamp),
		};
		if let Some(previous) = previous {
			let editable: Vec<SpotifyId> = library.editable().map(|playlist| playlist.id).collect();
			for id in editable {
				if let Some(held) = previous.items.get(&id) {
					library.items.insert(id, Arc::clone(held));
				} else if previous.unreadable.contains(&id) {
					library.unreadable.insert(id);
				}
			}
		}
		library
	}

	/// A library holding exactly `playlists`, the `contents` of some of them, and `liked`.
	pub fn from_parts(
		playlists: Vec<LibraryPlaylist>,
		contents: Vec<(SpotifyId, PlaylistContents)>,
		liked: Option<HashSet<ItemId>>,
	) -> Self {
		let items = contents
			.into_iter()
			.map(|(id, contents)| (id, Arc::new(Held::new(contents, 0))))
			.collect();
		Self {
			playlists,
			items,
			unreadable: HashSet::new(),
			liked: liked.map(Arc::new),
			liked_stamp: 0,
		}
	}

	/// Liked Songs is known, and every playlist the account can change has its items or couldn't
	/// be read by the last fetch that read the list.
	pub fn is_complete(&self) -> bool {
		self.liked.is_some()
			&& self.editable().all(|playlist| {
				self.items.contains_key(&playlist.id) || self.unreadable.contains(&playlist.id)
			})
	}

	/// Keeps `contents`, read with `stamp`, as the items and revision of playlist `id`, when it is
	/// in the list and the store [`lands`] above `floor`.
	fn store(&mut self, id: SpotifyId, contents: PlaylistContents, stamp: Stamp, floor: Stamp) {
		let held = self.items.get(&id).map(|held| held.stamp);
		let Some(playlist) = self.playlists.iter_mut().find(|playlist| playlist.id == id) else {
			return;
		};
		if !lands(stamp, held, floor) {
			return;
		}
		playlist.revision.clone_from(&contents.revision);
		self.items.insert(id, Arc::new(Held::new(contents, stamp)));
		self.unreadable.remove(&id);
	}

	/// Keeps `liked`, read with `stamp`, as Liked Songs, when the store [`lands`] above `floor`.
	fn store_liked(&mut self, liked: HashSet<ItemId>, stamp: Stamp, floor: Stamp) {
		if lands(stamp, self.liked.as_ref().map(|_| self.liked_stamp), floor) {
			self.liked = Some(Arc::new(liked));
			self.liked_stamp = stamp;
		}
	}

	/// Makes `change`, written with `stamp`, to Liked Songs, when it is known and the write
	/// [`lands`] above `floor`.
	fn change_liked(
		&mut self,
		change: impl FnOnce(&mut HashSet<ItemId>),
		stamp: Stamp,
		floor: Stamp,
	) {
		let Some(liked) = self.liked.as_mut() else { return };
		if lands(stamp, Some(self.liked_stamp), floor) {
			change(Arc::make_mut(liked));
			self.liked_stamp = stamp;
		}
	}

	/// Counts every playlist the account can change that has no items as unreadable.
	fn settle_unread(&mut self) {
		let unread: Vec<SpotifyId> = self
			.editable()
			.filter(|playlist| !self.items.contains_key(&playlist.id))
			.map(|playlist| playlist.id)
			.collect();
		self.unreadable.extend(unread);
	}

	pub fn editable(&self) -> impl Iterator<Item = &LibraryPlaylist> {
		self.playlists.iter().filter(|playlist| playlist.can_edit)
	}

	/// The rows of playlist `id`; `None` while they aren't known.
	pub fn contents(&self, id: SpotifyId) -> Option<&PlaylistContents> {
		self.items.get(&id).map(|held| &held.contents)
	}

	/// Whether `target` holds every one of `items`; `None` while its items aren't known, also
	/// when they couldn't be read. Liked Songs holds songs only: only the tracks among `items`
	/// count for it, and a selection without tracks is never all there.
	pub fn holds(&self, target: Target, items: &[ItemId]) -> Option<bool> {
		match target {
			Target::Playlist(id) => {
				let held = self.items.get(&id)?;
				Some(items.iter().all(|item| held.items.contains(item)))
			}
			Target::LikedSongs => {
				let liked = self.liked.as_ref()?;
				let tracks = tracks_of(items);
				Some(!tracks.is_empty() && tracks.iter().all(|track| liked.contains(track)))
			}
		}
	}

	/// How many songs Liked Songs holds; `None` while it isn't known.
	pub fn liked_count(&self) -> Option<usize> {
		self.liked.as_ref().map(|liked| liked.len())
	}

	/// The playlists the account can change whose items aren't known or are of an older revision.
	fn unread(&self) -> Vec<SpotifyId> {
		self.editable()
			.filter(|playlist| {
				self.items
					.get(&playlist.id)
					.is_none_or(|held| held.contents.revision != playlist.revision)
			})
			.map(|playlist| playlist.id)
			.collect()
	}
}

/// What the library holds right now.
#[derive(Clone, Debug)]
pub enum LibraryState {
	LoggedOut,
	/// Nothing is known yet; a fetch is running.
	Loading,
	/// The playlist list or Liked Songs couldn't be read, and no complete library is held.
	Failed(String),
	/// The playlist list is known; check [`Library::is_complete`] for the items.
	Ready(Arc<Library>),
}

/// The callbacks that hear about changes, each kept under an id until it unsubscribes.
pub struct Subscribers(Mutex<SubscriberList>);

struct SubscriberList {
	next: u64,
	notify: Vec<(u64, Arc<dyn Fn() + Send + Sync>)>,
}

impl Subscribers {
	pub const fn new() -> Self {
		Self(Mutex::new(SubscriberList { next: 0, notify: Vec::new() }))
	}

	fn lock(&self) -> MutexGuard<'_, SubscriberList> {
		self.0.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Keeps `notify` until [`unsubscribe`](Self::unsubscribe) gets the id this returns.
	pub fn subscribe(&self, notify: Box<dyn Fn() + Send + Sync>) -> u64 {
		let mut list = self.lock();
		let id = list.next;
		list.next += 1;
		list.notify.push((id, Arc::from(notify)));
		id
	}

	pub fn unsubscribe(&self, id: u64) {
		self.lock().notify.retain(|(subscriber, _)| *subscriber != id);
	}

	/// Calls every subscriber on this thread, outside the lock, so one may subscribe or
	/// unsubscribe.
	pub fn notify(&self) {
		let notify: Vec<_> =
			self.lock().notify.iter().map(|(_, notify)| Arc::clone(notify)).collect();
		for notify in notify {
			notify();
		}
	}

	/// How many are subscribed.
	pub fn count(&self) -> usize {
		self.lock().notify.len()
	}
}

impl Default for Subscribers {
	fn default() -> Self {
		Self::new()
	}
}

/// Where the library is read from.
pub(crate) trait LibrarySource: Send + Sync {
	/// The account's playlists, folders resolved, in the order Spotify lists them.
	fn rootlist(&self) -> impl Future<Output = Result<Vec<LibraryPlaylist>, CoreError>> + Send;

	/// The rows of one playlist.
	fn contents(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<PlaylistContents, CoreError>> + Send;

	/// The tracks and episodes in Liked Songs.
	fn liked(&self) -> impl Future<Output = Result<HashSet<ItemId>, CoreError>> + Send;

	/// Appends `uris` to playlist `id`, as a change on top of `base_revision`.
	fn add(
		&self,
		id: SpotifyId,
		base_revision: &[u8],
		uris: &[String],
	) -> impl Future<Output = Result<(), CoreError>> + Send;

	/// Removes `rows`, each with its index, from playlist `id`, as a change on top of
	/// `base_revision`.
	fn remove(
		&self,
		id: SpotifyId,
		base_revision: &[u8],
		rows: &[(usize, PlaylistRow)],
	) -> impl Future<Output = Result<(), CoreError>> + Send;

	/// Saves `uris` to Liked Songs, or takes them out when `liked` is false.
	fn set_liked(
		&self,
		uris: &[String],
		liked: bool,
	) -> impl Future<Output = Result<(), CoreError>> + Send;
}

/// What a change does with the items it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
	Add,
	Remove,
}

/// Where a change goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
	LikedSongs,
	Playlist(SpotifyId),
}

/// What a change did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
	/// Items added, or rows removed.
	pub changed: usize,
	/// Items already there when adding.
	pub unchanged: usize,
	/// Episodes left out of a Liked Songs change: Liked Songs holds songs only.
	pub episodes_left_out: usize,
}

/// Gets what a change did, or why it failed.
pub type OnChanged = Box<dyn FnOnce(Result<Change, CoreError>) + Send>;

/// `items` as URIs, each once, in order.
pub(crate) fn distinct_uris(items: &[ItemId]) -> Vec<String> {
	let mut seen = HashSet::new();
	items.iter().map(|item| item.uri().to_uri()).filter(|uri| seen.insert(uri.clone())).collect()
}

/// The distinct tracks among `items`, in order.
pub fn tracks_of(items: &[ItemId]) -> Vec<ItemId> {
	split_items(items).0
}

/// The distinct tracks among `items`, in order, and how many distinct episodes there are.
fn split_items(items: &[ItemId]) -> (Vec<ItemId>, usize) {
	let mut seen = HashSet::new();
	let (mut tracks, mut episodes) = (Vec::new(), 0);
	for &item in items.iter().filter(|item| seen.insert(**item)) {
		match item {
			ItemId::Track(_) => tracks.push(item),
			ItemId::Episode(_) => episodes += 1,
		}
	}
	(tracks, episodes)
}

/// The URIs of `items`, in order.
fn uris_of(items: &[ItemId]) -> Vec<String> {
	items.iter().map(|item| item.uri().to_uri()).collect()
}

/// Which of `uris` `contents` lacks, in order.
pub(crate) fn missing(uris: &[String], contents: &PlaylistContents) -> Vec<String> {
	let held: HashSet<&str> = contents.rows.iter().map(|row| row.uri.as_str()).collect();
	uris.iter().filter(|uri| !held.contains(uri.as_str())).cloned().collect()
}

/// Every row of `contents` holding one of `uris`, with its index.
pub(crate) fn rows_holding(
	uris: &[String],
	contents: &PlaylistContents,
) -> Vec<(usize, PlaylistRow)> {
	let wanted: HashSet<&str> = uris.iter().map(String::as_str).collect();
	contents
		.rows
		.iter()
		.enumerate()
		.filter(|(_, row)| wanted.contains(row.uri.as_str()))
		.map(|(index, row)| (index, row.clone()))
		.collect()
}

/// A change made by this client.
fn delta(op: Op) -> Delta {
	Delta {
		ops: vec![op],
		info: MessageField::some(ChangeInfo {
			source: MessageField::some(SourceInfo {
				client: Some(EnumOrUnknown::new(SourceClient::CLIENT)),
				..Default::default()
			}),
			..Default::default()
		}),
		..Default::default()
	}
}

/// The request that appends `uris` to the end of a playlist at `base_revision`.
pub(crate) fn add_changes(base_revision: &[u8], uris: &[String]) -> ListChanges {
	let items = uris.iter().map(|uri| Item { uri: Some(uri.clone()), ..Default::default() });
	let op = Op {
		kind: Some(EnumOrUnknown::new(OpKind::ADD)),
		add: MessageField::some(Add {
			items: items.collect(),
			add_last: Some(true),
			..Default::default()
		}),
		..Default::default()
	};
	ListChanges {
		base_revision: Some(base_revision.to_vec()),
		deltas: vec![delta(op)],
		want_resulting_revisions: Some(true),
		..Default::default()
	}
}

/// The request that removes `rows`, each named by its URI and item id, from a playlist at
/// `base_revision`.
pub(crate) fn remove_changes(base_revision: &[u8], rows: &[(usize, PlaylistRow)]) -> ListChanges {
	let items = rows.iter().map(|(_, row)| Item {
		uri: Some(row.uri.clone()),
		attributes: MessageField::some(ItemAttributes {
			item_id: Some(row.item_id.clone()),
			..Default::default()
		}),
		..Default::default()
	});
	let op = Op {
		kind: Some(EnumOrUnknown::new(OpKind::REM)),
		rem: MessageField::some(Rem {
			items: items.collect(),
			items_as_key: Some(true),
			..Default::default()
		}),
		..Default::default()
	};
	ListChanges {
		base_revision: Some(base_revision.to_vec()),
		deltas: vec![delta(op)],
		want_resulting_revisions: Some(true),
		..Default::default()
	}
}

/// The body of a write to the "collection" set of `username`: one item per URI, saved at
/// `now_secs`, or removed when `liked` is false. Fields: 1 username, 2 set, 3 items; in an item,
/// 1 URI, 2 time added, 3 removed.
pub(crate) fn liked_write(username: &str, uris: &[String], liked: bool, now_secs: i32) -> Vec<u8> {
	let mut body = Vec::new();
	{
		let mut out = CodedOutputStream::vec(&mut body);
		write_liked(&mut out, username, uris, liked, now_secs)
			.expect("writing to a vector can't fail");
	}
	body
}

/// The content type of a write to the collection.
pub(crate) const LIKED_CONTENT_TYPE: &str = "application/vnd.collection-v2.spotify.proto";

/// The headers of a write to the collection.
pub(crate) fn liked_headers() -> HeaderMap {
	let mut headers = HeaderMap::new();
	headers.insert(CONTENT_TYPE, HeaderValue::from_static(LIKED_CONTENT_TYPE));
	headers
}

fn write_liked(
	out: &mut CodedOutputStream<'_>,
	username: &str,
	uris: &[String],
	liked: bool,
	now_secs: i32,
) -> protobuf::Result<()> {
	out.write_string(1, username)?;
	out.write_string(2, "collection")?;
	for uri in uris {
		let mut item = Vec::new();
		{
			let mut inner = CodedOutputStream::vec(&mut item);
			inner.write_string(1, uri)?;
			inner.write_int32(2, now_secs)?;
			inner.write_bool(3, !liked)?;
			inner.flush()?;
		}
		out.write_bytes(3, &item)?;
	}
	out.flush()
}

#[derive(Default)]
struct Inner {
	/// The next stamp to hand out; `clear` keeps it.
	next_stamp: Stamp,
	/// Nothing with an older stamp lands; `clear` raises it past every stamp handed out.
	floor: Stamp,
	library: Option<Arc<Library>>,
	failed: Option<String>,
	/// The stamp of the running fetch.
	running: Option<Stamp>,
	started: Option<Instant>,
	/// The running fetch has published the playlist list.
	listed: bool,
}

impl Inner {
	fn state(&self) -> LibraryState {
		match (&self.library, &self.failed) {
			(Some(library), _) if library.is_complete() => LibraryState::Ready(Arc::clone(library)),
			(_, Some(reason)) => LibraryState::Failed(reason.clone()),
			(Some(library), None) => LibraryState::Ready(Arc::clone(library)),
			(None, None) => LibraryState::Loading,
		}
	}

	fn is_complete(&self) -> bool {
		self.library.as_ref().is_some_and(|library| library.is_complete())
	}

	/// Marks a new fetch, started at `now`, as running and hands out its stamp.
	fn start(&mut self, now: Instant) -> Stamp {
		let stamp = self.stamp();
		self.running = Some(stamp);
		self.started = Some(now);
		self.listed = false;
		stamp
	}

	fn stamp(&mut self) -> Stamp {
		let stamp = self.next_stamp;
		self.next_stamp += 1;
		stamp
	}

	/// The stamp for a read or write that is part of the fetch or change stamped `begun`: the
	/// next one, or `begun` itself once `clear` has raised the floor past it, so that nothing it
	/// stores lands.
	fn stamp_within(&mut self, begun: Stamp) -> Stamp {
		if self.admits(begun) { self.stamp() } else { begun }
	}

	/// Whether `clear` has left `stamp` at or above the floor.
	const fn admits(&self, stamp: Stamp) -> bool {
		stamp >= self.floor
	}

	/// The held library and the floor, for a store to change.
	fn library_and_floor(&mut self) -> Option<(&mut Library, Stamp)> {
		let floor = self.floor;
		self.library.as_mut().map(|library| (Arc::make_mut(library), floor))
	}

	/// Keeps `contents`, read with `stamp`, as the items of playlist `id`, as
	/// [`Library::store`] decides.
	fn store_rows(&mut self, id: SpotifyId, contents: PlaylistContents, stamp: Stamp) {
		if let Some((library, floor)) = self.library_and_floor() {
			library.store(id, contents, stamp, floor);
		}
	}

	/// Keeps `liked`, read with `stamp`, as Liked Songs, as [`Library::store_liked`] decides.
	fn store_liked(&mut self, liked: HashSet<ItemId>, stamp: Stamp) {
		if let Some((library, floor)) = self.library_and_floor() {
			library.store_liked(liked, stamp, floor);
		}
	}
}

/// The library as last read, shared by everything that shows it.
#[derive(Clone, Default)]
pub(crate) struct LibraryCache {
	inner: Arc<Mutex<Inner>>,
	/// Held by a change from its first read of Spotify to its last.
	changing: Arc<tokio::sync::Mutex<()>>,
	subscribers: Arc<Subscribers>,
}

impl LibraryCache {
	fn lock(&self) -> MutexGuard<'_, Inner> {
		self.inner.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Runs `change` once every change begun before it has ended. When `change` runs longer than
	/// [`LIBRARY_CHANGE_TIMEOUT`], it is dropped and fails with [`CHANGE_SLOW_REASON`]; the
	/// wait for its turn doesn't count.
	pub(crate) async fn in_turn<T>(
		&self,
		change: impl Future<Output = Result<T, CoreError>>,
	) -> Result<T, CoreError> {
		let _turn = self.changing.lock().await;
		within_change_limit(change).await
	}

	/// Makes `edit` with `items` to `target`, as [`add`](Self::add) and [`remove`](Self::remove)
	/// describe. Run it through [`in_turn`](Self::in_turn).
	pub(crate) async fn change<S: LibrarySource>(
		&self,
		source: &S,
		edit: Edit,
		target: Target,
		items: &[ItemId],
	) -> Result<Change, CoreError> {
		match edit {
			Edit::Add => self.add(source, target, items).await,
			Edit::Remove => self.remove(source, target, items).await,
		}
	}

	/// What is held, and the fetch the caller must now [`run`](Fetch::run). A fetch starts when
	/// none runs and any of these holds: the held library is missing or incomplete, the last
	/// fetch failed, or it started more than [`LIBRARY_REFRESH`] before `now`. Never returns
	/// `LoggedOut`.
	pub(crate) fn snapshot(&self, now: Instant) -> (LibraryState, Option<Fetch>) {
		let mut inner = self.lock();
		let state = inner.state();
		let start = inner.running.is_none()
			&& (!inner.is_complete()
				|| inner.failed.is_some()
				|| inner.started.is_none_or(|started| {
					now.saturating_duration_since(started) > LIBRARY_REFRESH
				}));
		let fetch = start.then(|| Fetch { cache: self.clone(), stamp: inner.start(now) });
		(state, fetch)
	}

	/// What is held, without starting anything.
	pub(crate) fn held(&self) -> LibraryState {
		self.lock().state()
	}

	/// Calls `notify` after each publish and failure of a fetch, and after each
	/// [`clear`](Self::clear), on the thread that made it, outside the lock. Returns the id to
	/// [`unsubscribe`](Self::unsubscribe) with.
	pub(crate) fn subscribe(&self, notify: Box<dyn Fn() + Send + Sync>) -> u64 {
		self.subscribers.subscribe(notify)
	}

	pub(crate) fn unsubscribe(&self, id: u64) {
		self.subscribers.unsubscribe(id);
	}

	/// Forgets everything, including what a running fetch would publish.
	pub(crate) fn clear(&self) {
		{
			let mut inner = self.lock();
			let next_stamp = inner.next_stamp;
			*inner = Inner { next_stamp, floor: next_stamp, ..Inner::default() };
		}
		self.subscribers.notify();
	}

	/// Adds `items` to `target`. A playlist is read first, so only what it lacks is sent, and
	/// nothing is sent when Spotify sends only part of it; Liked Songs takes the tracks among
	/// `items` only.
	async fn add<S: LibrarySource>(
		&self,
		source: &S,
		target: Target,
		items: &[ItemId],
	) -> Result<Change, CoreError> {
		let begun = self.lock().stamp();
		match target {
			Target::Playlist(id) => {
				let uris = distinct_uris(items);
				let fresh = whole_contents(source, id).await?;
				let lacking = missing(&uris, &fresh);
				if !lacking.is_empty() {
					source.add(id, &fresh.revision, &lacking).await?;
					self.read_again(source, id, begun, |after| missing(&lacking, after).is_empty())
						.await?;
				}
				Ok(Change {
					changed: lacking.len(),
					unchanged: uris.len() - lacking.len(),
					episodes_left_out: 0,
				})
			}
			Target::LikedSongs => {
				let (tracks, episodes_left_out) = split_items(items);
				let lacking: Vec<ItemId> = {
					let inner = self.lock();
					let held = inner.library.as_ref().and_then(|library| library.liked.as_deref());
					tracks
						.iter()
						.copied()
						.filter(|track| held.is_none_or(|held| !held.contains(track)))
						.collect()
				};
				if !lacking.is_empty() {
					source.set_liked(&uris_of(&lacking), true).await?;
					self.change_liked(begun, |liked| liked.extend(lacking.iter().copied()));
				}
				Ok(Change {
					changed: lacking.len(),
					unchanged: tracks.len() - lacking.len(),
					episodes_left_out,
				})
			}
		}
	}

	/// Removes `items` from `target`: every copy of each in a playlist, the tracks among them
	/// from Liked Songs. A playlist is read first; nothing is sent when Spotify sends only part
	/// of it.
	async fn remove<S: LibrarySource>(
		&self,
		source: &S,
		target: Target,
		items: &[ItemId],
	) -> Result<Change, CoreError> {
		let begun = self.lock().stamp();
		match target {
			Target::Playlist(id) => {
				let fresh = whole_contents(source, id).await?;
				let rows = rows_holding(&distinct_uris(items), &fresh);
				if !rows.is_empty() {
					source.remove(id, &fresh.revision, &rows).await?;
					self.read_again(source, id, begun, |after| {
						let left: HashSet<&[u8]> =
							after.rows.iter().map(|row| row.item_id.as_slice()).collect();
						rows.iter().all(|(_, gone)| !left.contains(gone.item_id.as_slice()))
					})
					.await?;
				}
				Ok(Change { changed: rows.len(), ..Change::default() })
			}
			Target::LikedSongs => {
				let (tracks, episodes_left_out) = split_items(items);
				if !tracks.is_empty() {
					source.set_liked(&uris_of(&tracks), false).await?;
					self.change_liked(begun, |liked| {
						for track in &tracks {
							liked.remove(track);
						}
					});
				}
				Ok(Change { changed: tracks.len(), unchanged: 0, episodes_left_out })
			}
		}
	}

	/// Reads playlist `id` after the change stamped `begun` and keeps its rows and revision, with
	/// the stamp the read took as it started, unless `clear` has run since `begun`. Fails with
	/// [`stale_change`] when `applied` says the rows it read lack the change. When the read
	/// fails, the held copy stays, the next snapshot starts a fetch, and the change counts as made.
	async fn read_again<S: LibrarySource>(
		&self,
		source: &S,
		id: SpotifyId,
		begun: Stamp,
		applied: impl FnOnce(&PlaylistContents) -> bool,
	) -> Result<(), CoreError> {
		let stamp = self.lock().stamp_within(begun);
		let result = source.contents(id).await;
		let mut inner = self.lock();
		if !inner.admits(stamp) {
			return Ok(());
		}
		let contents = match result {
			Ok(contents) => contents,
			Err(error) => {
				log::warn!("couldn't read a Spotify playlist after changing it: {error}");
				inner.started = None;
				return Ok(());
			}
		};
		let applied = applied(&contents);
		inner.store_rows(id, contents, stamp);
		if applied { Ok(()) } else { Err(stale_change()) }
	}

	/// Makes `change` to the held Liked Songs, as a write that is part of the change stamped
	/// `begun`, as [`Library::change_liked`] decides.
	fn change_liked(&self, begun: Stamp, change: impl FnOnce(&mut HashSet<ItemId>)) {
		let mut inner = self.lock();
		let stamp = inner.stamp_within(begun);
		if let Some((library, floor)) = inner.library_and_floor() {
			library.change_liked(change, stamp, floor);
		}
	}
}

/// A fetch of the library, which [`LibraryCache::snapshot`] hands out. It holds the cache's
/// running-fetch slot until it is dropped, whether it finished, was dropped part-way or
/// panicked; it leaves alone a fetch started after it.
pub(crate) struct Fetch {
	cache: LibraryCache,
	stamp: Stamp,
}

impl Drop for Fetch {
	fn drop(&mut self) {
		let mut inner = self.cache.lock();
		if inner.running == Some(self.stamp) {
			inner.running = None;
		}
	}
}

impl Fetch {
	/// Waits for `source`, then [`read`](Self::read)s the library from it, all within
	/// [`LIBRARY_FETCH_TIMEOUT`]. When `source` fails, the fetch [`fail`](Self::fail)s with its
	/// error; when the time runs out, with [`SLOW_REASON`].
	pub(crate) async fn run<S: LibrarySource>(
		self,
		source: impl Future<Output = Result<S, CoreError>>,
	) {
		let read = async {
			match source.await {
				Ok(source) => self.read(&source).await,
				Err(error) => self.fail(&error),
			}
		};
		tokio::select! {
			() = read => {}
			() = tokio::time::sleep(LIBRARY_FETCH_TIMEOUT) => {
				self.fail(&CoreError::Unavailable(SLOW_REASON.to_owned()));
			}
		}
	}

	/// Reads Liked Songs and the playlist list together, and publishes the list as soon as it
	/// arrives. Then reads the items of each playlist the account can change whose revision is
	/// new, and keeps each, and Liked Songs, as its read finishes. Items whose revision didn't
	/// change are kept, and a playlist whose revision changed keeps its old rows until the new
	/// ones arrive; if they can't be read, the old ones stay. A playlist without old rows whose
	/// read fails counts as unreadable once the fetch ends, until a later fetch reads it.
	/// Subscribers hear when the list is published and when the fetch ends. Each read takes its
	/// stamp as it starts, so what a change stored since stays. Nothing it read lands once
	/// `clear` has run.
	async fn read<S: LibrarySource>(&self, source: &S) {
		let liked_stamp = {
			let mut inner = self.cache.lock();
			if !inner.admits(self.stamp) {
				return;
			}
			inner.stamp_within(self.stamp)
		};
		let mut liked_read = Box::pin(future::maybe_done(source.liked()));
		let listed = match future::select(pin!(source.rootlist()), liked_read.as_mut()).await {
			Either::Left((listed, _)) => listed,
			Either::Right(((), rootlist)) => rootlist.await,
		};
		let playlists = match listed {
			Ok(playlists) => playlists,
			Err(error) => {
				self.fail(&error);
				return;
			}
		};
		let unread = {
			let mut inner = self.cache.lock();
			if !inner.admits(self.stamp) {
				return;
			}
			let library = Library::new(playlists, inner.library.as_deref());
			let unread = library.unread();
			inner.library = Some(Arc::new(library));
			inner.failed = None;
			inner.listed = true;
			unread
		};
		self.cache.subscribers.notify();

		let rows = async {
			let mut reads = stream::iter(unread)
				.map(|id| async move {
					let stamp = self.cache.lock().stamp_within(self.stamp);
					(id, stamp, source.contents(id).await)
				})
				.buffer_unordered(PLAYLIST_CONCURRENCY);
			while let Some((id, stamp, result)) = reads.next().await {
				match result {
					Ok(contents) => self.cache.lock().store_rows(id, contents, stamp),
					Err(error) => log::warn!("couldn't read a Spotify playlist: {error}"),
				}
			}
		};
		let liked = async {
			liked_read.as_mut().await;
			match liked_read.as_mut().take_output().expect("the read has finished") {
				Ok(liked) => {
					self.cache.lock().store_liked(liked, liked_stamp);
					None
				}
				Err(error) => {
					log::warn!("couldn't read Liked Songs: {error}");
					Some(error.to_string())
				}
			}
		};
		let ((), liked_error) = future::join(rows, liked).await;

		{
			let mut inner = self.cache.lock();
			if !inner.admits(self.stamp) {
				return;
			}
			let Some((library, _)) = inner.library_and_floor() else {
				return;
			};
			library.settle_unread();
			let complete = library.is_complete();
			inner.failed = liked_error.filter(|_| !complete);
			if inner.running == Some(self.stamp) {
				inner.running = None;
			}
		}
		self.cache.subscribers.notify();
	}

	/// Ends the fetch with `error`, for example because there is no session; it does nothing
	/// once another fetch holds the slot. A complete library stays as it is, and the next refresh
	/// waits [`LIBRARY_REFRESH`] from the fetch's start. When this fetch published the list and
	/// Liked Songs is known, the playlists still without items count as unreadable, which
	/// completes the library; any other library answers `Failed` with `error`, and the next
	/// snapshot starts a fetch.
	fn fail(&self, error: &CoreError) {
		{
			let mut inner = self.cache.lock();
			if inner.running != Some(self.stamp) {
				return;
			}
			inner.running = None;
			let liked_known = inner.library.as_ref().is_some_and(|library| library.liked.is_some());
			if inner.is_complete() {
				log::warn!("couldn't refresh the Spotify library: {error}");
			} else if inner.listed && liked_known {
				log::warn!("couldn't read every Spotify playlist: {error}");
				if let Some((library, _)) = inner.library_and_floor() {
					library.settle_unread();
				}
			} else {
				inner.failed = Some(error.to_string());
			}
		}
		self.cache.subscribers.notify();
	}
}

/// Why a change refuses a playlist Spotify sent only part of.
const TRUNCATED_REASON: &str = "Spotify sent only part of this playlist, so foo_librespot can't tell which tracks it already holds";

/// The error of a change Spotify answered but did not apply, because its base revision was old.
fn stale_change() -> CoreError {
	CoreError::Unavailable(
		"Spotify didn't take the change because the playlist changed meanwhile; try again"
			.to_owned(),
	)
}

/// The rows of playlist `id`, read from `source`; fails when Spotify sent only some of them.
async fn whole_contents<S: LibrarySource>(
	source: &S,
	id: SpotifyId,
) -> Result<PlaylistContents, CoreError> {
	let contents = source.contents(id).await?;
	if contents.truncated {
		return Err(CoreError::Unavailable(TRUNCATED_REASON.to_owned()));
	}
	Ok(contents)
}

/// The playlists of a rootlist answer, with the folders each sits in.
pub(crate) fn rootlist_playlists(message: &SelectedListContent) -> Vec<LibraryPlaylist> {
	let mut playlists = Vec::new();
	let Some(contents) = message.contents.as_ref() else {
		return playlists;
	};
	let mut folders: Vec<String> = Vec::new();
	for (index, item) in contents.items.iter().enumerate() {
		let uri = item.uri();
		if let Some(rest) = uri.strip_prefix(START_GROUP_URI) {
			let name = rest.split_once(':').map_or("", |(_, name)| name);
			folders.push(decode_search(name, true));
		} else if uri.starts_with(END_GROUP_URI) {
			folders.pop();
		} else if let Some(id) = uri.strip_prefix(PLAYLIST_URI)
			&& let Ok(id) = SpotifyId::from_base62(id)
			&& let Some(meta) = contents.meta_items.get(index)
		{
			playlists.push(LibraryPlaylist {
				id,
				name: meta.attributes.name().to_owned(),
				folders: folders.clone(),
				can_edit: meta.capabilities.can_edit_items(),
				revision: meta.revision().to_vec(),
			});
		}
	}
	playlists
}

/// The revision and rows of a playlist answer.
pub(crate) fn playlist_contents(message: &SelectedListContent) -> PlaylistContents {
	let rows = message
		.contents
		.as_ref()
		.map(|contents| {
			contents
				.items
				.iter()
				.map(|item| PlaylistRow {
					uri: item.uri().to_owned(),
					item_id: item.attributes.item_id().to_vec(),
				})
				.collect()
		})
		.unwrap_or_default();
	PlaylistContents {
		revision: message.revision().to_vec(),
		rows,
		truncated: message.contents.truncated(),
	}
}

/// The track or episode `uri` names; `None` for anything else.
fn item_of(uri: &SpotifyUri) -> Option<ItemId> {
	match uri {
		SpotifyUri::Track { id } => Some(ItemId::Track(*id)),
		SpotifyUri::Episode { id } => Some(ItemId::Episode(*id)),
		_ => None,
	}
}

/// The library as Spotify has it, through librespot.
pub(crate) struct LibrespotLibrary {
	session: Session,
}

impl LibrespotLibrary {
	pub(crate) const fn new(session: Session) -> Self {
		Self { session }
	}
}

impl LibrarySource for LibrespotLibrary {
	/// Reads the list from the start in pages of [`ROOTLIST_PAGE`], while Spotify says more
	/// follow.
	async fn rootlist(&self) -> Result<Vec<LibraryPlaylist>, CoreError> {
		let mut whole = SelectedListContent::new();
		let mut from = 0;
		loop {
			let bytes = retry_rate_limited(|| {
				self.session.spclient().get_rootlist(from, Some(ROOTLIST_PAGE))
			})
			.await?;
			let mut page = SelectedListContent::parse_from_bytes(&bytes)
				.map_err(librespot_core::Error::from)?;
			let Some(contents) = page.contents.as_mut() else {
				break;
			};
			let next = usize::try_from(contents.pos()).unwrap_or(from) + contents.items.len();
			let more = contents.truncated() && next > from;
			let all = whole.contents.mut_or_insert_default();
			all.items.append(&mut contents.items);
			all.meta_items.append(&mut contents.meta_items);
			if !more {
				break;
			}
			from = next;
		}
		Ok(rootlist_playlists(&whole))
	}

	async fn contents(&self, id: SpotifyId) -> Result<PlaylistContents, CoreError> {
		let bytes = retry_rate_limited(|| self.session.spclient().get_playlist(&id)).await?;
		let message =
			SelectedListContent::parse_from_bytes(&bytes).map_err(librespot_core::Error::from)?;
		if message.contents.truncated() {
			log::warn!("Spotify sent only part of playlist {id}");
		}
		Ok(playlist_contents(&message))
	}

	async fn liked(&self) -> Result<HashSet<ItemId>, CoreError> {
		let uris = liked_song_uris(&self.session).await?;
		Ok(uris.iter().filter_map(item_of).collect())
	}

	async fn add(
		&self,
		id: SpotifyId,
		base_revision: &[u8],
		uris: &[String],
	) -> Result<(), CoreError> {
		self.send_changes(id, &add_changes(base_revision, uris)).await
	}

	async fn remove(
		&self,
		id: SpotifyId,
		base_revision: &[u8],
		rows: &[(usize, PlaylistRow)],
	) -> Result<(), CoreError> {
		self.send_changes(id, &remove_changes(base_revision, rows)).await
	}

	async fn set_liked(&self, uris: &[String], liked: bool) -> Result<(), CoreError> {
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(0, |since| i32::try_from(since.as_secs()).unwrap_or(i32::MAX));
		let body = liked_write(&self.session.username(), uris, liked, now);
		let headers = liked_headers();
		retry_rate_limited(|| {
			self.session.spclient().request(
				&Method::POST,
				"/collection/v2/write",
				Some(headers.clone()),
				Some(&body),
			)
		})
		.await?;
		Ok(())
	}
}

impl LibrespotLibrary {
	async fn send_changes(&self, id: SpotifyId, changes: &ListChanges) -> Result<(), CoreError> {
		let endpoint = format!("/playlist/v2/playlist/{}/changes", id.to_base62());
		retry_rate_limited(|| {
			self.session.spclient().request_with_protobuf(&Method::POST, &endpoint, None, changes)
		})
		.await?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use std::collections::{HashMap, HashSet};
	use std::sync::Arc;
	use std::sync::Mutex;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
	use std::time::{Duration, Instant};

	use librespot_protocol::playlist_permission::Capabilities;
	use librespot_protocol::playlist4_external::{
		Item, ItemAttributes, ListAttributes, ListItems, MetaItem, SelectedListContent,
	};
	use protobuf::MessageField;
	use tokio::sync::Notify;

	use super::{
		Change, Edit, Fetch, LIBRARY_CHANGE_TIMEOUT, LIBRARY_FETCH_TIMEOUT, LIBRARY_REFRESH,
		LIKED_CONTENT_TYPE, LibraryCache, LibraryPlaylist, LibrarySource, LibraryState,
		PlaylistContents, PlaylistRow, SLOW_REASON, SourceClient, Target, add_changes,
		distinct_uris, liked_headers, liked_write, missing, playlist_contents, remove_changes,
		rootlist_playlists, rows_holding, tracks_of,
	};
	use crate::link::{ItemId, Link, LinkKind, parse_item};
	use crate::{CoreError, SpotifyId};

	impl LibraryCache {
		/// A fetch started at `now`, whether one runs or not.
		fn begin(&self, now: Instant) -> Fetch {
			let stamp = self.lock().start(now);
			Fetch { cache: self.clone(), stamp }
		}

		/// Reads a fetch started at `now` to its end.
		async fn fetch<S: LibrarySource>(&self, source: &S, now: Instant) {
			self.begin(now).read(source).await;
		}

		/// Ends a fetch started now with `error`.
		fn fail(&self, error: &CoreError) {
			self.begin(Instant::now()).fail(error);
		}
	}

	fn id(byte: u8) -> SpotifyId {
		SpotifyId::from_raw(&[byte; 16]).unwrap()
	}

	fn playlist(byte: u8, name: &str, can_edit: bool, revision: &str) -> LibraryPlaylist {
		LibraryPlaylist {
			id: id(byte),
			name: name.to_owned(),
			folders: Vec::new(),
			can_edit,
			revision: revision.as_bytes().to_vec(),
		}
	}

	#[test]
	fn a_library_from_parts_holds_what_it_was_given() {
		let playlists = vec![playlist(1, "Sunday set", true, "r1")];
		let contents =
			PlaylistContents { revision: b"r1".to_vec(), rows: rows(&[t1()]), truncated: false };
		let incomplete = super::Library::from_parts(playlists.clone(), Vec::new(), None);
		assert!(!incomplete.is_complete());
		let library = super::Library::from_parts(
			playlists,
			vec![(id(1), contents.clone())],
			Some(HashSet::from([t2()])),
		);
		assert!(library.is_complete());
		assert_eq!(library.contents(id(1)), Some(&contents));
		assert_eq!(library.liked_count(), Some(1));
	}

	fn rows(items: &[ItemId]) -> Vec<PlaylistRow> {
		items
			.iter()
			.enumerate()
			.map(|(index, item)| PlaylistRow {
				uri: uri_of(*item),
				item_id: vec![u8::try_from(index).unwrap()],
			})
			.collect()
	}

	fn contents(revision: &str, items: &[ItemId]) -> PlaylistContents {
		PlaylistContents {
			revision: revision.as_bytes().to_vec(),
			rows: rows(items),
			truncated: false,
		}
	}

	#[test]
	fn a_playlist_holds_what_its_rows_name_and_ignores_other_rows() {
		let mut held = contents("r1", &[t1(), episode(2)]);
		held.rows.push(PlaylistRow { uri: "spotify:local:a:b:c:180".to_owned(), item_id: vec![9] });
		let library = super::Library::from_parts(
			vec![playlist(1, "A", true, "r1")],
			vec![(id(1), held)],
			None,
		);
		let a = Target::Playlist(id(1));
		assert_eq!(library.holds(a, &[t1(), episode(2)]), Some(true));
		assert_eq!(library.holds(a, &[t1(), t2()]), Some(false));
		assert_eq!(library.holds(Target::Playlist(id(2)), &[t1()]), None);
		assert_eq!(library.holds(Target::LikedSongs, &[t1()]), None);
	}

	#[test]
	fn liked_songs_counts_the_tracks_only_and_never_holds_episodes_alone() {
		let library = super::Library::from_parts(
			Vec::new(),
			Vec::new(),
			Some(HashSet::from([t1(), episode(2)])),
		);
		assert_eq!(library.holds(Target::LikedSongs, &[t1(), episode(3)]), Some(true));
		assert_eq!(library.holds(Target::LikedSongs, &[episode(2)]), Some(false));
		assert_eq!(library.holds(Target::LikedSongs, &[t1(), t2()]), Some(false));
	}

	#[test]
	fn tracks_of_keeps_the_first_of_each_track() {
		let items = [episode(2), t2(), t1(), episode(2), t2()];
		assert_eq!(tracks_of(&items), vec![t2(), t1()]);
	}

	fn item(uri: &str) -> Item {
		Item { uri: Some(uri.to_owned()), ..Default::default() }
	}

	fn meta(name: &str, can_edit: bool, revision: &str) -> MetaItem {
		MetaItem {
			revision: Some(revision.as_bytes().to_vec()),
			attributes: MessageField::some(ListAttributes {
				name: Some(name.to_owned()),
				..Default::default()
			}),
			capabilities: MessageField::some(Capabilities {
				can_edit_items: Some(can_edit),
				..Default::default()
			}),
			..Default::default()
		}
	}

	fn rootlist(entries: Vec<(String, MetaItem)>) -> SelectedListContent {
		let (items, meta_items) = entries.into_iter().map(|(uri, meta)| (item(&uri), meta)).unzip();
		SelectedListContent {
			contents: MessageField::some(ListItems {
				pos: Some(0),
				truncated: Some(false),
				items,
				meta_items,
				..Default::default()
			}),
			..Default::default()
		}
	}

	fn playlist_uri(byte: u8) -> String {
		format!("spotify:playlist:{}", id(byte).to_base62())
	}

	fn group(start: bool, name: &str) -> (String, MetaItem) {
		let uri = if start {
			format!("spotify:start-group:aa:{name}")
		} else {
			"spotify:end-group:aa".to_owned()
		};
		(uri, MetaItem::default())
	}

	#[test]
	fn the_playlist_list_follows_folders_and_takes_names_from_the_meta_items() {
		let message = rootlist(vec![
			group(true, "Church"),
			(playlist_uri(1), meta("Sunday set", true, "r1")),
			group(false, ""),
			(playlist_uri(2), meta("Followed", false, "r2")),
			(format!("spotify:track:{}", id(3).to_base62()), MetaItem::default()),
		]);
		let playlists = rootlist_playlists(&message);
		assert_eq!(
			playlists,
			vec![
				LibraryPlaylist {
					folders: vec!["Church".to_owned()],
					..playlist(1, "Sunday set", true, "r1")
				},
				playlist(2, "Followed", false, "r2"),
			]
		);
		assert_eq!(playlists[0].link(), Link::Id { kind: LinkKind::Playlist, id: id(1) });
	}

	#[test]
	fn a_folder_name_is_decoded() {
		let message =
			rootlist(vec![group(true, "My+Folder%21"), (playlist_uri(1), meta("A", true, "r"))]);
		assert_eq!(rootlist_playlists(&message)[0].folders, vec!["My Folder!".to_owned()]);
	}

	#[test]
	fn folders_nest_outermost_first() {
		let message = rootlist(vec![
			group(true, "Church"),
			group(true, "Youth"),
			(playlist_uri(1), meta("Sunday set", true, "r")),
			group(false, ""),
			(playlist_uri(2), meta("Hymns", true, "r")),
			group(false, ""),
			(playlist_uri(3), meta("Loose", true, "r")),
		]);
		let playlists = rootlist_playlists(&message);
		assert_eq!(playlists[0].folders, vec!["Church".to_owned(), "Youth".to_owned()]);
		assert_eq!(playlists[1].folders, vec!["Church".to_owned()]);
		assert!(playlists[2].folders.is_empty());
	}

	#[test]
	fn display_name_puts_the_folders_first() {
		let playlist = LibraryPlaylist {
			folders: vec!["Church".to_owned(), "Youth".to_owned()],
			..playlist(1, "Sunday set", true, "r")
		};
		assert_eq!(playlist.display_name(), "Church / Youth / Sunday set");
		assert_eq!(self::playlist(2, "Loose", true, "r").display_name(), "Loose");
	}

	#[test]
	fn playlist_contents_keep_each_row_with_its_item_id() {
		let row = |uri: &str, item_id: u8| Item {
			uri: Some(uri.to_owned()),
			attributes: MessageField::some(ItemAttributes {
				item_id: Some(vec![item_id; 8]),
				..Default::default()
			}),
			..Default::default()
		};
		let message = SelectedListContent {
			revision: Some(b"rev".to_vec()),
			contents: MessageField::some(ListItems {
				pos: Some(0),
				truncated: Some(false),
				items: vec![row("spotify:track:a", 1), row("spotify:track:b", 2)],
				..Default::default()
			}),
			..Default::default()
		};
		assert_eq!(
			playlist_contents(&message),
			PlaylistContents {
				revision: b"rev".to_vec(),
				rows: vec![
					PlaylistRow { uri: "spotify:track:a".to_owned(), item_id: vec![1; 8] },
					PlaylistRow { uri: "spotify:track:b".to_owned(), item_id: vec![2; 8] },
				],
				truncated: false,
			}
		);
	}

	#[test]
	fn playlist_contents_say_when_spotify_sent_only_part() {
		let message = SelectedListContent {
			revision: Some(b"rev".to_vec()),
			contents: MessageField::some(ListItems {
				pos: Some(0),
				truncated: Some(true),
				items: vec![item("spotify:track:a")],
				..Default::default()
			}),
			..Default::default()
		};
		assert!(playlist_contents(&message).truncated);
	}

	type IndexedRow = (usize, PlaylistRow);

	#[derive(Default)]
	struct FakeLibrary {
		playlists: Mutex<Vec<LibraryPlaylist>>,
		contents: Mutex<HashMap<SpotifyId, PlaylistContents>>,
		liked: Mutex<HashSet<ItemId>>,
		rootlist_calls: AtomicUsize,
		contents_calls: AtomicUsize,
		liked_calls: AtomicUsize,
		fail_rootlist: AtomicBool,
		fail_contents: AtomicBool,
		hang_rootlist: AtomicBool,
		/// When set, `contents` waits for it before answering.
		gate: Option<Arc<Notify>>,
		/// The call number of `contents` whose answer, what the playlist was when the call began,
		/// is held back until `contents_gate` opens; 0 for none.
		hold_contents_call: AtomicUsize,
		contents_gate: Arc<Notify>,
		/// The call number of `contents` that fails; 0 for none.
		fail_contents_call: AtomicUsize,
		/// When set, `liked` answers what Liked Songs was when the call began, once `liked_gate`
		/// opens.
		hold_liked: AtomicBool,
		liked_gate: Arc<Notify>,
		/// When set, `add` and `remove` change nothing, as Spotify does for a stale revision.
		drop_changes: AtomicBool,
		/// When set, `set_liked` waits for `set_liked_gate` after recording its call.
		hold_set_liked: AtomicBool,
		set_liked_gate: Arc<Notify>,
		/// The playlists whose `contents` waits for `held_gate` before answering.
		held: Mutex<HashSet<SpotifyId>>,
		held_gate: Arc<Notify>,
		/// The playlists whose `contents` fails.
		failing: Mutex<HashSet<SpotifyId>>,
		/// The playlist of each `contents` call, in order.
		contents_ids: Mutex<Vec<SpotifyId>>,
		fail_liked: AtomicBool,
		/// The calls to `add`: the base revision and the URIs.
		adds: Mutex<Vec<(Vec<u8>, Vec<String>)>>,
		/// The calls to `remove`: the base revision and the rows.
		removes: Mutex<Vec<(Vec<u8>, Vec<IndexedRow>)>>,
		/// The calls to `set_liked`.
		likes: Mutex<Vec<(Vec<String>, bool)>>,
	}

	impl LibrarySource for FakeLibrary {
		async fn rootlist(&self) -> Result<Vec<LibraryPlaylist>, CoreError> {
			self.rootlist_calls.fetch_add(1, Ordering::SeqCst);
			tokio::task::yield_now().await;
			if self.hang_rootlist.load(Ordering::SeqCst) {
				std::future::pending::<()>().await;
			}
			if self.fail_rootlist.load(Ordering::SeqCst) {
				return Err(CoreError::Unavailable("no network".to_owned()));
			}
			Ok(self.playlists.lock().unwrap().clone())
		}

		async fn contents(&self, id: SpotifyId) -> Result<PlaylistContents, CoreError> {
			let call = self.contents_calls.fetch_add(1, Ordering::SeqCst) + 1;
			self.contents_ids.lock().unwrap().push(id);
			if let Some(gate) = &self.gate {
				gate.notified().await;
			}
			let held = self.held.lock().unwrap().contains(&id);
			if held {
				self.held_gate.notified().await;
			}
			if self.fail_contents.load(Ordering::SeqCst)
				|| self.fail_contents_call.load(Ordering::SeqCst) == call
				|| self.failing.lock().unwrap().contains(&id)
			{
				return Err(CoreError::Unavailable("playlist failed".to_owned()));
			}
			let answer = self.contents.lock().unwrap().get(&id).expect("unknown playlist").clone();
			if self.hold_contents_call.load(Ordering::SeqCst) == call {
				self.contents_gate.notified().await;
			}
			Ok(answer)
		}

		async fn liked(&self) -> Result<HashSet<ItemId>, CoreError> {
			self.liked_calls.fetch_add(1, Ordering::SeqCst);
			tokio::task::yield_now().await;
			if self.fail_liked.load(Ordering::SeqCst) {
				return Err(CoreError::Unavailable("Liked Songs failed".to_owned()));
			}
			let answer = self.liked.lock().unwrap().clone();
			if self.hold_liked.load(Ordering::SeqCst) {
				self.liked_gate.notified().await;
			}
			Ok(answer)
		}

		async fn add(
			&self,
			id: SpotifyId,
			base_revision: &[u8],
			uris: &[String],
		) -> Result<(), CoreError> {
			tokio::task::yield_now().await;
			self.adds.lock().unwrap().push((base_revision.to_vec(), uris.to_vec()));
			if self.drop_changes.load(Ordering::SeqCst) {
				return Ok(());
			}
			let mut all = self.contents.lock().unwrap();
			let held = all.get_mut(&id).expect("unknown playlist");
			held.revision = b"after".to_vec();
			for uri in uris {
				held.rows.push(PlaylistRow { uri: uri.clone(), item_id: vec![9] });
			}
			Ok(())
		}

		async fn remove(
			&self,
			id: SpotifyId,
			base_revision: &[u8],
			rows: &[(usize, PlaylistRow)],
		) -> Result<(), CoreError> {
			tokio::task::yield_now().await;
			self.removes.lock().unwrap().push((base_revision.to_vec(), rows.to_vec()));
			if self.drop_changes.load(Ordering::SeqCst) {
				return Ok(());
			}
			let mut all = self.contents.lock().unwrap();
			let held = all.get_mut(&id).expect("unknown playlist");
			held.revision = b"after".to_vec();
			held.rows.retain(|row| !rows.iter().any(|(_, gone)| gone == row));
			Ok(())
		}

		async fn set_liked(&self, uris: &[String], liked: bool) -> Result<(), CoreError> {
			tokio::task::yield_now().await;
			self.likes.lock().unwrap().push((uris.to_vec(), liked));
			if self.hold_set_liked.load(Ordering::SeqCst) {
				self.set_liked_gate.notified().await;
			}
			let mut set = self.liked.lock().unwrap();
			for uri in uris {
				if liked {
					set.insert(parse_item(uri).expect("a track"));
				} else {
					set.remove(&parse_item(uri).expect("a track"));
				}
			}
			Ok(())
		}
	}

	impl FakeLibrary {
		/// Holds back the answer of the `n`th `contents` call from now until `contents_gate`
		/// opens, and returns that call's number.
		fn hold_contents(&self, n: usize) -> usize {
			let call = self.contents_calls.load(Ordering::SeqCst) + n;
			self.hold_contents_call.store(call, Ordering::SeqCst);
			call
		}

		/// Whether `contents` call `call` has begun.
		fn has_begun(&self, call: usize) -> bool {
			self.contents_calls.load(Ordering::SeqCst) >= call
		}
	}

	/// A library of an editable playlist A holding `t1`, a followed playlist B, and one liked
	/// song `t2`.
	fn fake() -> FakeLibrary {
		let fake = FakeLibrary::default();
		*fake.playlists.lock().unwrap() =
			vec![playlist(1, "A", true, "r1"), playlist(2, "B", false, "r2")];
		fake.contents.lock().unwrap().insert(id(1), contents("r1", &[t1()]));
		fake.liked.lock().unwrap().insert(t2());
		fake
	}

	fn track(byte: u8) -> ItemId {
		ItemId::Track(id(byte))
	}

	fn episode(byte: u8) -> ItemId {
		ItemId::Episode(id(byte))
	}

	/// The track the fake's playlist A holds.
	fn t1() -> ItemId {
		track(0x71)
	}

	/// The track the fake's Liked Songs holds.
	fn t2() -> ItemId {
		track(0x72)
	}

	fn t3() -> ItemId {
		track(0x73)
	}

	fn t9() -> ItemId {
		track(0x79)
	}

	fn uri_of(item: ItemId) -> String {
		item.uri().to_uri()
	}

	fn strings(uris: &[&str]) -> Vec<String> {
		uris.iter().map(|uri| (*uri).to_owned()).collect()
	}

	#[test]
	fn distinct_uris_keeps_the_first_of_each() {
		let items = [track(1), episode(2), track(1), track(3), episode(2)];
		assert_eq!(
			distinct_uris(&items),
			vec![uri_of(track(1)), uri_of(episode(2)), uri_of(track(3))]
		);
	}

	#[test]
	fn missing_lists_what_the_playlist_lacks() {
		let held = contents("r", &[t1(), t3()]);
		let lacking = missing(&distinct_uris(&[t1(), t2(), t3(), t9()]), &held);
		assert_eq!(lacking, vec![uri_of(t2()), uri_of(t9())]);
	}

	#[test]
	fn rows_holding_finds_every_copy() {
		let held = PlaylistContents {
			revision: b"r".to_vec(),
			rows: vec![
				PlaylistRow { uri: "a".to_owned(), item_id: vec![0] },
				PlaylistRow { uri: "b".to_owned(), item_id: vec![1] },
				PlaylistRow { uri: "a".to_owned(), item_id: vec![2] },
			],
			truncated: false,
		};
		let found = rows_holding(&strings(&["a"]), &held);
		assert_eq!(found, vec![(0, held.rows[0].clone()), (2, held.rows[2].clone())]);
	}

	#[test]
	fn the_add_message_appends_the_tracks_after_the_base_revision() {
		use librespot_protocol::playlist4_external::op::Kind;
		let message = add_changes(b"rev", &strings(&["spotify:track:a", "spotify:track:b"]));
		assert_eq!(message.base_revision(), b"rev");
		assert!(message.want_resulting_revisions());
		assert_eq!(message.deltas.len(), 1);
		let delta = &message.deltas[0];
		assert_eq!(delta.info.source.client(), SourceClient::CLIENT);
		assert_eq!(delta.ops.len(), 1);
		let op = &delta.ops[0];
		assert_eq!(op.kind(), Kind::ADD);
		assert!(op.add.add_last());
		let uris: Vec<&str> = op.add.items.iter().map(Item::uri).collect();
		assert_eq!(uris, ["spotify:track:a", "spotify:track:b"]);
	}

	#[test]
	fn the_remove_message_names_each_row() {
		use librespot_protocol::playlist4_external::op::Kind;
		let rows = vec![
			(0, PlaylistRow { uri: "spotify:track:a".to_owned(), item_id: vec![1; 8] }),
			(4, PlaylistRow { uri: "spotify:track:a".to_owned(), item_id: vec![2; 8] }),
		];
		let message = remove_changes(b"rev", &rows);
		assert_eq!(message.base_revision(), b"rev");
		assert!(message.want_resulting_revisions());
		let delta = &message.deltas[0];
		assert_eq!(delta.info.source.client(), SourceClient::CLIENT);
		assert_eq!(delta.ops.len(), 1);
		let op = &delta.ops[0];
		assert_eq!(op.kind(), Kind::REM);
		assert!(op.rem.items_as_key());
		assert_eq!(op.rem.items.len(), 2);
		assert_eq!(op.rem.items[1].uri(), "spotify:track:a");
		assert_eq!(op.rem.items[0].attributes.item_id(), [1; 8]);
		assert_eq!(op.rem.items[1].attributes.item_id(), [2; 8]);
	}

	/// The fields of a Liked Songs write: the username, the set, and each item's fields.
	fn read_write(bytes: &[u8]) -> (String, String, Vec<(String, i32, bool)>) {
		let mut input = protobuf::CodedInputStream::from_bytes(bytes);
		let (mut username, mut set, mut items) = (String::new(), String::new(), Vec::new());
		while let Some(tag) = input.read_raw_tag_or_eof().unwrap() {
			match tag >> 3 {
				1 => username = input.read_string().unwrap(),
				2 => set = input.read_string().unwrap(),
				3 => {
					let item = input.read_bytes().unwrap();
					let mut item = protobuf::CodedInputStream::from_bytes(&item);
					let (mut uri, mut added_at, mut removed) = (String::new(), 0, false);
					while let Some(tag) = item.read_raw_tag_or_eof().unwrap() {
						match tag >> 3 {
							1 => uri = item.read_string().unwrap(),
							2 => added_at = item.read_int32().unwrap(),
							3 => removed = item.read_bool().unwrap(),
							other => panic!("unexpected field {other}"),
						}
					}
					items.push((uri, added_at, removed));
				}
				other => panic!("unexpected field {other}"),
			}
		}
		(username, set, items)
	}

	#[test]
	fn the_liked_songs_write_names_each_track() {
		let uris = strings(&["spotify:track:a", "spotify:track:b"]);
		let (username, set, items) = read_write(&liked_write("leonard", &uris, true, 1234));
		assert_eq!(username, "leonard");
		assert_eq!(set, "collection");
		assert_eq!(
			items,
			vec![
				("spotify:track:a".to_owned(), 1234, false),
				("spotify:track:b".to_owned(), 1234, false),
			]
		);
		let (_, _, items) = read_write(&liked_write("leonard", &uris, false, 1234));
		assert!(items.iter().all(|(_, _, removed)| *removed));
	}

	async fn loaded(source: &FakeLibrary) -> LibraryCache {
		let cache = LibraryCache::default();
		cache.fetch(source, Instant::now()).await;
		cache
	}

	#[tokio::test]
	async fn adding_reads_the_playlist_first_and_adds_only_what_it_lacks() {
		let source = fake();
		source.contents.lock().unwrap().insert(id(1), contents("r1", &[track(1)]));
		let cache = loaded(&source).await;
		// The playlist changed on Spotify after the cache read it.
		source.contents.lock().unwrap().get_mut(&id(1)).unwrap().revision = b"r9".to_vec();
		let change =
			cache.add(&source, Target::Playlist(id(1)), &[track(1), track(2)]).await.unwrap();
		assert_eq!(change, Change { changed: 1, unchanged: 1, episodes_left_out: 0 });
		assert_eq!(*source.adds.lock().unwrap(), vec![(b"r9".to_vec(), vec![uri_of(track(2))])]);
	}

	#[tokio::test]
	async fn a_track_selected_twice_is_added_once() {
		let source = fake();
		let cache = loaded(&source).await;
		let change =
			cache.add(&source, Target::Playlist(id(1)), &[track(5), track(5)]).await.unwrap();
		assert_eq!(change.changed, 1);
		assert_eq!(source.adds.lock().unwrap()[0].1, vec![uri_of(track(5))]);
	}

	#[tokio::test]
	async fn adding_what_is_all_there_sends_nothing() {
		let source = fake();
		source.contents.lock().unwrap().insert(id(1), contents("r1", &[track(1)]));
		let cache = loaded(&source).await;
		let change = cache.add(&source, Target::Playlist(id(1)), &[track(1)]).await.unwrap();
		assert_eq!(change, Change { changed: 0, unchanged: 1, episodes_left_out: 0 });
		assert!(source.adds.lock().unwrap().is_empty());
	}

	#[tokio::test]
	async fn removing_takes_every_copy_out() {
		let source = fake();
		source
			.contents
			.lock()
			.unwrap()
			.insert(id(1), contents("r1", &[track(1), track(2), track(1)]));
		let cache = loaded(&source).await;
		let change = cache.remove(&source, Target::Playlist(id(1)), &[track(1)]).await.unwrap();
		assert_eq!(change.changed, 2);
		let removes = source.removes.lock().unwrap();
		assert_eq!(removes[0].0, b"r1".to_vec());
		assert_eq!(removes[0].1.iter().map(|(index, _)| *index).collect::<Vec<_>>(), vec![0, 2]);
		drop(removes);
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[track(1)]), Some(false));
		assert_eq!(library.holds(Target::Playlist(id(1)), &[track(2)]), Some(true));
	}

	#[tokio::test]
	async fn a_change_refuses_a_playlist_spotify_sent_only_part_of() {
		let source = fake();
		let cache = loaded(&source).await;
		let cut = PlaylistContents { truncated: true, ..contents("r1", &[track(1)]) };
		source.contents.lock().unwrap().insert(id(1), cut);
		let added = cache.add(&source, Target::Playlist(id(1)), &[track(4)]).await;
		let removed = cache.remove(&source, Target::Playlist(id(1)), &[track(1)]).await;
		for result in [added, removed] {
			assert!(
				matches!(&result, Err(CoreError::Unavailable(reason)) if reason == "Spotify sent only part of this playlist, so foo_librespot can't tell which tracks it already holds"),
				"{result:?}"
			);
		}
		assert!(source.adds.lock().unwrap().is_empty());
		assert!(source.removes.lock().unwrap().is_empty());
	}

	#[tokio::test]
	async fn removing_what_is_not_there_sends_nothing() {
		let source = fake();
		let cache = loaded(&source).await;
		let change = cache.remove(&source, Target::Playlist(id(1)), &[track(7)]).await.unwrap();
		assert_eq!(change.changed, 0);
		assert!(source.removes.lock().unwrap().is_empty());
	}

	#[tokio::test]
	async fn after_a_change_the_held_copy_is_read_again() {
		let source = fake();
		let cache = loaded(&source).await;
		assert_eq!(ready(cache.held()).holds(Target::Playlist(id(1)), &[track(4)]), Some(false));
		cache.add(&source, Target::Playlist(id(1)), &[track(4)]).await.unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[track(4)]), Some(true));
		assert_eq!(library.contents(id(1)).unwrap().revision, b"after".to_vec());
		assert_eq!(library.playlists[0].revision, b"after".to_vec());
	}

	#[tokio::test]
	async fn saving_to_liked_songs_leaves_episodes_out() {
		let source = fake();
		let cache = loaded(&source).await;
		let change = cache
			.add(&source, Target::LikedSongs, &[track(1), episode(2), episode(2)])
			.await
			.unwrap();
		assert_eq!(change, Change { changed: 1, unchanged: 0, episodes_left_out: 1 });
		assert_eq!(*source.likes.lock().unwrap(), vec![(vec![uri_of(track(1))], true)]);
		assert_eq!(ready(cache.held()).holds(Target::LikedSongs, &[track(1)]), Some(true));
	}

	#[tokio::test]
	async fn saving_counts_what_liked_songs_already_holds() {
		let source = fake();
		source.liked.lock().unwrap().insert(track(1));
		let cache = loaded(&source).await;
		let change = cache.add(&source, Target::LikedSongs, &[track(1), track(2)]).await.unwrap();
		assert_eq!(change, Change { changed: 1, unchanged: 1, episodes_left_out: 0 });
		assert_eq!(*source.likes.lock().unwrap(), vec![(vec![uri_of(track(2))], true)]);
		let change = cache.add(&source, Target::LikedSongs, &[track(1), track(2)]).await.unwrap();
		assert_eq!(change, Change { changed: 0, unchanged: 2, episodes_left_out: 0 });
		assert_eq!(source.likes.lock().unwrap().len(), 1);
	}

	#[tokio::test]
	async fn saving_without_a_held_liked_set_sends_every_track() {
		let source = fake();
		let cache = LibraryCache::default();
		let change = cache.add(&source, Target::LikedSongs, &[track(1)]).await.unwrap();
		assert_eq!(change, Change { changed: 1, unchanged: 0, episodes_left_out: 0 });
	}

	#[tokio::test]
	async fn removing_from_liked_songs_unsaves_the_tracks() {
		let source = fake();
		source.liked.lock().unwrap().extend([track(1), track(2)]);
		let cache = loaded(&source).await;
		let change = cache
			.remove(&source, Target::LikedSongs, &[track(1), track(1), track(2)])
			.await
			.unwrap();
		assert_eq!(change, Change { changed: 2, unchanged: 0, episodes_left_out: 0 });
		assert_eq!(
			*source.likes.lock().unwrap(),
			vec![(vec![uri_of(track(1)), uri_of(track(2))], false)]
		);
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::LikedSongs, &[track(1)]), Some(false));
		assert_eq!(library.holds(Target::LikedSongs, &[track(2)]), Some(false));
	}

	#[tokio::test]
	async fn a_write_in_flight_over_a_logout_leaves_the_next_account_alone() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		source.hold_set_liked.store(true, Ordering::SeqCst);
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.add(&*source, Target::LikedSongs, &[track(1)]).await }
		});
		until(|| !source.likes.lock().unwrap().is_empty()).await;
		cache.clear();
		let other = fake();
		cache.fetch(&other, Instant::now()).await;
		source.set_liked_gate.notify_one();
		task.await.unwrap().unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::LikedSongs, &[track(1)]), Some(false));
	}

	#[tokio::test]
	async fn a_playlist_change_in_flight_over_a_logout_leaves_the_next_account_alone() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		let read_before = source.hold_contents(1);
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.add(&*source, Target::Playlist(id(1)), &[track(4)]).await }
		});
		until(|| source.has_begun(read_before)).await;
		cache.clear();
		let other = fake();
		cache.fetch(&other, Instant::now()).await;
		source.contents_gate.notify_one();
		task.await.unwrap().unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[track(4)]), Some(false));
		assert_eq!(library.contents(id(1)).unwrap().revision, b"r1".to_vec());
	}

	#[tokio::test]
	async fn a_fetch_that_read_liked_songs_before_a_save_keeps_the_saved_track() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		source.hold_liked.store(true, Ordering::SeqCst);
		let fetch = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, Instant::now()).await }
		});
		until(|| source.liked_calls.load(Ordering::SeqCst) == 2).await;
		until(|| source.liked.lock().unwrap().len() == 1).await;
		tokio::task::yield_now().await;
		cache.add(&*source, Target::LikedSongs, &[track(1)]).await.unwrap();
		source.liked_gate.notify_one();
		fetch.await.unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::LikedSongs, &[track(1)]), Some(true));
	}

	#[tokio::test]
	async fn a_fetch_that_read_a_playlist_before_a_change_keeps_the_change() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		// Playlist A has a new revision on Spotify.
		source.playlists.lock().unwrap()[0].revision = b"r2".to_vec();
		source.contents.lock().unwrap().get_mut(&id(1)).unwrap().revision = b"r2".to_vec();
		let fetch_read = source.hold_contents(1);
		let fetch = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, Instant::now()).await }
		});
		until(|| source.has_begun(fetch_read)).await;
		cache.add(&*source, Target::Playlist(id(1)), &[track(4)]).await.unwrap();
		source.contents_gate.notify_one();
		fetch.await.unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[track(4)]), Some(true));
		assert_eq!(library.contents(id(1)).unwrap().revision, b"after".to_vec());
	}

	#[tokio::test]
	async fn of_two_re_reads_of_a_playlist_the_later_one_stays_when_it_lands_first() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		let a = Target::Playlist(id(1));
		// The first change reads the playlist before and after its add.
		let first_re_read = source.hold_contents(2);
		let first = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.add(&*source, a, &[track(4)]).await }
		});
		until(|| source.has_begun(first_re_read)).await;
		cache.add(&*source, a, &[track(5)]).await.unwrap();
		source.contents_gate.notify_one();
		first.await.unwrap().unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds(a, &[t1(), track(4), track(5)]), Some(true));
	}

	#[tokio::test]
	async fn a_failed_re_read_after_a_change_makes_the_next_snapshot_fetch() {
		let source = fake();
		let cache = loaded(&source).await;
		let (_, start) = cache.snapshot(Instant::now());
		assert!(start.is_none());
		// The read before the change is the next call; the read after it is the one after.
		let next = source.contents_calls.load(Ordering::SeqCst) + 2;
		source.fail_contents_call.store(next, Ordering::SeqCst);
		let change = cache.add(&source, Target::Playlist(id(1)), &[track(4)]).await.unwrap();
		assert_eq!(change.changed, 1);
		let (state, start) = cache.snapshot(Instant::now());
		assert!(start.is_some());
		assert!(is_ready(&state));
	}

	#[test]
	fn the_liked_songs_write_is_sent_as_the_collection_type() {
		let headers = liked_headers();
		assert_eq!(
			headers.get(http::header::CONTENT_TYPE).unwrap(),
			"application/vnd.collection-v2.spotify.proto"
		);
		assert_eq!(LIKED_CONTENT_TYPE, "application/vnd.collection-v2.spotify.proto");
	}

	const STALE: &str =
		"Spotify didn't take the change because the playlist changed meanwhile; try again";

	#[tokio::test]
	async fn an_add_spotify_dropped_is_an_error() {
		let source = fake();
		let cache = loaded(&source).await;
		source.drop_changes.store(true, Ordering::SeqCst);
		let result = cache.add(&source, Target::Playlist(id(1)), &[track(4)]).await;
		assert!(
			matches!(&result, Err(CoreError::Unavailable(reason)) if reason == STALE),
			"{result:?}"
		);
	}

	#[tokio::test]
	async fn a_remove_spotify_dropped_is_an_error() {
		let source = fake();
		source.contents.lock().unwrap().insert(id(1), contents("r1", &[track(1)]));
		let cache = loaded(&source).await;
		source.drop_changes.store(true, Ordering::SeqCst);
		let result = cache.remove(&source, Target::Playlist(id(1)), &[track(1)]).await;
		assert!(
			matches!(&result, Err(CoreError::Unavailable(reason)) if reason == STALE),
			"{result:?}"
		);
	}

	#[tokio::test]
	async fn a_change_whose_re_read_fails_still_reports_success() {
		let source = fake();
		let cache = loaded(&source).await;
		let next = source.contents_calls.load(Ordering::SeqCst) + 2;
		source.fail_contents_call.store(next, Ordering::SeqCst);
		source.drop_changes.store(true, Ordering::SeqCst);
		assert!(cache.add(&source, Target::Playlist(id(1)), &[track(4)]).await.is_ok());
	}

	fn is_ready(state: &LibraryState) -> bool {
		matches!(state, LibraryState::Ready(_))
	}

	fn ready(state: LibraryState) -> Arc<super::Library> {
		match state {
			LibraryState::Ready(library) => library,
			other => panic!("not ready: {other:?}"),
		}
	}

	/// Lets other tasks run until `condition` holds.
	async fn until(condition: impl Fn() -> bool) {
		for _ in 0..1000 {
			if condition() {
				return;
			}
			tokio::task::yield_now().await;
		}
		panic!("never happened");
	}

	#[tokio::test]
	async fn the_first_snapshot_starts_a_fetch_and_says_loading() {
		let cache = LibraryCache::default();
		let now = Instant::now();
		let (state, start) = cache.snapshot(now);
		assert!(matches!(state, LibraryState::Loading));
		assert!(start.is_some());
		let (state, start) = cache.snapshot(now);
		assert!(matches!(state, LibraryState::Loading));
		assert!(start.is_none());
	}

	#[tokio::test]
	async fn the_list_is_published_before_the_items() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let now = Instant::now();
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, now).await }
		});
		until(|| is_ready(&cache.held())).await;
		let library = ready(cache.held());
		assert!(!library.is_complete());
		assert_eq!(library.playlists.len(), 2);
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t1()]), None);
		gate.notify_one();
		task.await.unwrap();
		let library = ready(cache.held());
		assert!(library.is_complete());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t1()]), Some(true));
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t1(), t9()]), Some(false));
		assert_eq!(library.holds(Target::Playlist(id(2)), &[t1()]), None);
		assert_eq!(library.holds(Target::LikedSongs, &[t2()]), Some(true));
		assert_eq!(library.holds(Target::LikedSongs, &[t1()]), Some(false));
		assert_eq!(library.contents(id(1)).unwrap().rows.len(), 1);
	}

	#[tokio::test]
	async fn only_playlists_the_account_can_change_are_read() {
		let source = fake();
		let cache = LibraryCache::default();
		cache.fetch(&source, Instant::now()).await;
		assert_eq!(source.contents_calls.load(Ordering::SeqCst), 1);
		assert_eq!(source.liked_calls.load(Ordering::SeqCst), 1);
		let library = ready(cache.held());
		assert_eq!(library.editable().count(), 1);
	}

	#[tokio::test]
	async fn liked_songs_is_read_while_the_playlist_list_is_on_its_way() {
		let source = Arc::new(fake());
		source.hang_rootlist.store(true, Ordering::SeqCst);
		let cache = LibraryCache::default();
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, Instant::now()).await }
		});
		until(|| source.liked_calls.load(Ordering::SeqCst) == 1).await;
		assert_eq!(source.rootlist_calls.load(Ordering::SeqCst), 1);
		assert!(matches!(cache.held(), LibraryState::Loading));
		task.abort();
	}

	#[tokio::test]
	async fn unchanged_playlists_are_not_read_again() {
		let source = fake();
		let cache = LibraryCache::default();
		let now = Instant::now();
		cache.fetch(&source, now).await;
		cache.fetch(&source, now).await;
		assert_eq!(source.rootlist_calls.load(Ordering::SeqCst), 2);
		assert_eq!(source.contents_calls.load(Ordering::SeqCst), 1);

		source.playlists.lock().unwrap()[0].revision = b"r3".to_vec();
		source.contents.lock().unwrap().insert(id(1), contents("r3", &[t1(), t3()]));
		cache.fetch(&source, now).await;
		assert_eq!(source.contents_calls.load(Ordering::SeqCst), 2);
		let library = ready(cache.held());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t3()]), Some(true));
	}

	#[tokio::test]
	async fn a_fresh_library_starts_no_fetch_and_a_stale_one_does() {
		let source = fake();
		let cache = LibraryCache::default();
		let start = Instant::now();
		assert!(cache.snapshot(start).1.is_some());
		cache.fetch(&source, start).await;
		let (state, fetch) = cache.snapshot(start + Duration::from_secs(30));
		assert!(is_ready(&state));
		assert!(fetch.is_none());
		let (state, fetch) = cache.snapshot(start + LIBRARY_REFRESH + Duration::from_secs(1));
		assert!(is_ready(&state));
		assert!(fetch.is_some());
	}

	#[tokio::test]
	async fn a_failed_first_fetch_says_why_and_tries_again() {
		let source = fake();
		source.fail_rootlist.store(true, Ordering::SeqCst);
		let cache = LibraryCache::default();
		let now = Instant::now();
		assert!(cache.snapshot(now).1.is_some());
		cache.fetch(&source, now).await;
		let (state, fetch) = cache.snapshot(now);
		assert!(
			matches!(&state, LibraryState::Failed(reason) if reason == "no network"),
			"{state:?}"
		);
		assert!(fetch.is_some());
	}

	#[tokio::test]
	async fn a_failed_refresh_keeps_the_complete_library() {
		let source = fake();
		let cache = LibraryCache::default();
		let start = Instant::now();
		cache.fetch(&source, start).await;
		source.fail_rootlist.store(true, Ordering::SeqCst);
		let later = start + LIBRARY_REFRESH + Duration::from_secs(1);
		let fetch = cache.snapshot(later).1.expect("no refresh started");
		fetch.read(&source).await;
		assert!(ready(cache.held()).is_complete());
		cache.fail(&CoreError::NotLoggedIn);
		assert!(ready(cache.held()).is_complete());
	}

	/// Subscribes to `cache` a counter that reads what it holds each time; returns its id and
	/// the count.
	fn counting(cache: &LibraryCache) -> (u64, Arc<AtomicUsize>) {
		let calls = Arc::new(AtomicUsize::new(0));
		let id = cache.subscribe(Box::new({
			let (cache, calls) = (cache.clone(), Arc::clone(&calls));
			move || {
				let _ = cache.held();
				calls.fetch_add(1, Ordering::SeqCst);
			}
		}));
		(id, calls)
	}

	#[tokio::test]
	async fn a_subscriber_hears_the_list_and_the_end_of_each_fetch() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let (_, calls) = counting(&cache);
		let now = Instant::now();
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, now).await }
		});
		until(|| calls.load(Ordering::SeqCst) == 1).await;
		assert!(is_ready(&cache.held()));
		gate.notify_one();
		task.await.unwrap();
		assert_eq!(calls.load(Ordering::SeqCst), 2);
		cache.fetch(&*source, now).await;
		assert_eq!(calls.load(Ordering::SeqCst), 4);
	}

	#[tokio::test]
	async fn an_unsubscribed_subscriber_hears_nothing() {
		let source = fake();
		let cache = LibraryCache::default();
		let (id, calls) = counting(&cache);
		let (_, other) = counting(&cache);
		cache.unsubscribe(id);
		cache.fetch(&source, Instant::now()).await;
		cache.clear();
		assert_eq!(calls.load(Ordering::SeqCst), 0);
		assert_eq!(other.load(Ordering::SeqCst), 3);
	}

	#[tokio::test]
	async fn logging_out_forgets_the_library() {
		let source = fake();
		let cache = LibraryCache::default();
		let now = Instant::now();
		cache.fetch(&source, now).await;
		cache.clear();
		assert!(matches!(cache.held(), LibraryState::Loading));
		let (state, fetch) = cache.snapshot(now);
		assert!(matches!(state, LibraryState::Loading));
		assert!(fetch.is_some());
	}

	#[tokio::test]
	async fn a_fetch_that_ends_after_a_logout_publishes_nothing() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let now = Instant::now();
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, now).await }
		});
		until(|| is_ready(&cache.held())).await;
		cache.clear();
		gate.notify_one();
		task.await.unwrap();
		assert!(matches!(cache.held(), LibraryState::Loading));
	}

	/// Changes playlist A to revision r3 with a new row, as Spotify would after an edit.
	fn change_a(source: &FakeLibrary) {
		source.playlists.lock().unwrap()[0].revision = b"r3".to_vec();
		source.contents.lock().unwrap().insert(id(1), contents("r3", &[t1(), t3()]));
	}

	#[tokio::test]
	async fn a_changed_playlist_whose_reread_fails_keeps_its_old_rows() {
		let source = fake();
		let cache = LibraryCache::default();
		let start = Instant::now();
		cache.fetch(&source, start).await;
		change_a(&source);
		source.fail_contents.store(true, Ordering::SeqCst);
		cache.fetch(&source, start).await;
		let library = ready(cache.held());
		assert!(library.is_complete());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t1()]), Some(true));
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t3()]), Some(false));
		// The next refresh reads it again.
		source.fail_contents.store(false, Ordering::SeqCst);
		cache.fetch(&source, start).await;
		assert_eq!(ready(cache.held()).holds(Target::Playlist(id(1)), &[t3()]), Some(true));
	}

	#[tokio::test]
	async fn a_library_stays_complete_while_a_changed_playlist_is_read_again() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let start = Instant::now();
		gate.notify_one();
		cache.fetch(&*source, start).await;
		change_a(&source);
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, start).await }
		});
		until(|| ready_revision(&cache) == Some(b"r3".to_vec())).await;
		assert!(ready(cache.held()).is_complete());
		gate.notify_one();
		task.await.unwrap();
		assert_eq!(ready(cache.held()).holds(Target::Playlist(id(1)), &[t3()]), Some(true));
	}

	fn ready_revision(cache: &LibraryCache) -> Option<Vec<u8>> {
		match cache.held() {
			LibraryState::Ready(library) => Some(library.playlists[0].revision.clone()),
			_ => None,
		}
	}

	#[tokio::test]
	async fn a_fetch_dropped_part_way_lets_the_next_snapshot_start_one() {
		let source = fake();
		source.hang_rootlist.store(true, Ordering::SeqCst);
		let cache = LibraryCache::default();
		let now = Instant::now();
		let fetch = cache.snapshot(now).1.expect("no fetch started");
		let read = async move { fetch.read(&source).await };
		let result = tokio::time::timeout(Duration::from_millis(20), read).await;
		assert!(result.is_err());
		assert!(cache.snapshot(now).1.is_some());
	}

	#[tokio::test]
	async fn a_clear_during_a_fetch_lets_the_next_snapshot_start_one() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let now = Instant::now();
		let old = cache.snapshot(now).1.expect("no fetch started");
		let task = tokio::spawn({
			let source = Arc::clone(&source);
			async move { old.read(&*source).await }
		});
		until(|| is_ready(&cache.held())).await;
		cache.clear();
		let new = cache.snapshot(now).1;
		assert!(new.is_some());
		gate.notify_one();
		task.await.unwrap();
		assert!(matches!(cache.held(), LibraryState::Loading));
		// The new fetch still runs: the old one's end must not mark it finished.
		assert!(cache.snapshot(now).1.is_none());
	}

	#[tokio::test]
	async fn a_subscriber_hears_each_clear() {
		let cache = LibraryCache::default();
		let (_, calls) = counting(&cache);
		cache.clear();
		cache.clear();
		assert_eq!(calls.load(Ordering::SeqCst), 2);
	}

	#[tokio::test]
	async fn an_old_fetch_that_fails_and_ends_leaves_a_newer_one_alone() {
		let cache = LibraryCache::default();
		let now = Instant::now();
		let old = cache.snapshot(now).1.expect("no fetch started");
		cache.clear();
		let new = cache.snapshot(now).1;
		assert!(new.is_some());
		old.fail(&CoreError::Unavailable("too slow".to_owned()));
		drop(old);
		assert!(matches!(cache.held(), LibraryState::Loading));
		assert!(cache.snapshot(now).1.is_none());
	}

	#[tokio::test]
	async fn a_finished_fetch_leaves_the_mark_of_a_fetch_its_subscriber_started() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let now = Instant::now();
		let fetch = cache.snapshot(now).1.expect("no fetch started");
		let task = tokio::spawn({
			let source = Arc::clone(&source);
			async move { fetch.read(&*source).await }
		});
		until(|| is_ready(&cache.held())).await;
		let later = now + LIBRARY_REFRESH + Duration::from_secs(1);
		let started = Arc::new(Mutex::new(Vec::new()));
		cache.subscribe(Box::new({
			let (cache, started) = (cache.clone(), Arc::clone(&started));
			move || {
				if let Some(fetch) = cache.snapshot(later).1 {
					started.lock().unwrap().push(fetch);
				}
			}
		}));
		assert!(cache.snapshot(later).1.is_none());
		gate.notify_one();
		task.await.unwrap();
		assert_eq!(started.lock().unwrap().len(), 1);
		let much_later = later + LIBRARY_REFRESH + Duration::from_secs(1);
		assert!(cache.snapshot(much_later).1.is_none());
	}

	#[tokio::test(start_paused = true)]
	async fn a_fetch_that_runs_out_of_time_fails_and_says_so() {
		let source = fake();
		source.hang_rootlist.store(true, Ordering::SeqCst);
		let cache = LibraryCache::default();
		let now = Instant::now();
		let (_, calls) = counting(&cache);
		let fetch = cache.snapshot(now).1.expect("no fetch started");
		fetch.run(async { Ok::<_, CoreError>(source) }).await;
		assert!(
			matches!(cache.held(), LibraryState::Failed(reason) if reason == SLOW_REASON),
			"{:?}",
			cache.held()
		);
		assert_eq!(calls.load(Ordering::SeqCst), 1);
		assert!(cache.snapshot(now).1.is_some());
	}

	#[tokio::test(start_paused = true)]
	async fn a_change_spotify_never_answers_fails_after_the_limit() {
		let source = fake();
		let cache = loaded(&source).await;
		source.held.lock().unwrap().insert(id(1));
		let started = tokio::time::Instant::now();
		let items = [track(4)];
		let change =
			cache.in_turn(cache.change(&source, Edit::Add, Target::Playlist(id(1)), &items));
		let result = tokio::time::timeout(LIBRARY_CHANGE_TIMEOUT + Duration::from_secs(1), change)
			.await
			.expect("the change outlived its limit");
		assert!(is_too_slow(&result), "{result:?}");
		assert_eq!(started.elapsed(), LIBRARY_CHANGE_TIMEOUT);
		assert!(source.adds.lock().unwrap().is_empty());
	}

	fn is_too_slow(result: &Result<Change, CoreError>) -> bool {
		matches!(result, Err(CoreError::Unavailable(reason)) if reason == "Spotify didn't answer in time")
	}

	/// Starts `edit` with `items` to `target` on another task, as `Core::change` makes it.
	fn spawn_change(
		cache: &LibraryCache,
		source: &Arc<FakeLibrary>,
		edit: Edit,
		target: Target,
		items: Vec<ItemId>,
	) -> tokio::task::JoinHandle<Result<Change, CoreError>> {
		let (cache, source) = (cache.clone(), Arc::clone(source));
		tokio::spawn(
			async move { cache.in_turn(cache.change(&*source, edit, target, &items)).await },
		)
	}

	#[tokio::test]
	async fn a_second_change_waits_for_the_first_and_sees_its_rows() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		let first_read = source.hold_contents(1);
		let a = Target::Playlist(id(1));
		let first = spawn_change(&cache, &source, Edit::Add, a, vec![track(4)]);
		until(|| source.has_begun(first_read)).await;
		let reads = source.contents_calls.load(Ordering::SeqCst);
		let second = spawn_change(&cache, &source, Edit::Add, a, vec![track(4)]);
		for _ in 0..20 {
			tokio::task::yield_now().await;
		}
		assert_eq!(source.contents_calls.load(Ordering::SeqCst), reads);
		source.contents_gate.notify_one();
		assert_eq!(first.await.unwrap().unwrap().changed, 1);
		assert_eq!(
			second.await.unwrap().unwrap(),
			Change { changed: 0, unchanged: 1, episodes_left_out: 0 }
		);
		assert_eq!(source.adds.lock().unwrap().len(), 1);
	}

	#[tokio::test(start_paused = true)]
	async fn the_wait_for_a_turn_doesnt_count_against_the_limit() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		source.held.lock().unwrap().insert(id(1));
		source.hold_set_liked.store(true, Ordering::SeqCst);
		let first =
			spawn_change(&cache, &source, Edit::Add, Target::Playlist(id(1)), vec![track(4)]);
		let second = spawn_change(&cache, &source, Edit::Add, Target::LikedSongs, vec![track(5)]);
		tokio::time::sleep(LIBRARY_CHANGE_TIMEOUT + Duration::from_secs(30)).await;
		source.set_liked_gate.notify_one();
		let first = first.await.unwrap();
		assert!(is_too_slow(&first), "{first:?}");
		assert_eq!(second.await.unwrap().unwrap().changed, 1);
	}

	/// Adds playlist C, which the account can change, holding `t3`.
	fn add_c(source: &FakeLibrary) {
		source.playlists.lock().unwrap().push(playlist(3, "C", true, "r3"));
		source.contents.lock().unwrap().insert(id(3), contents("r3", &[t3()]));
	}

	#[tokio::test]
	async fn a_playlist_that_cant_be_read_leaves_the_library_complete_without_its_rows() {
		let source = fake();
		source.failing.lock().unwrap().insert(id(1));
		let cache = LibraryCache::default();
		let start = Instant::now();
		cache.fetch(&source, start).await;
		let (state, fetch) = cache.snapshot(start);
		let library = ready(state);
		assert!(library.is_complete());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t1()]), None);
		assert!(fetch.is_none());
		source.failing.lock().unwrap().clear();
		let later = start + LIBRARY_REFRESH + Duration::from_secs(1);
		assert!(cache.snapshot(later).1.is_some());
		cache.fetch(&source, later).await;
		assert_eq!(ready(cache.held()).holds(Target::Playlist(id(1)), &[t1()]), Some(true));
	}

	#[tokio::test]
	async fn a_playlist_that_cant_be_read_counts_only_once_the_fetch_ends() {
		let source = Arc::new(fake());
		add_c(&source);
		source.failing.lock().unwrap().insert(id(1));
		source.held.lock().unwrap().insert(id(3));
		let cache = LibraryCache::default();
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, Instant::now()).await }
		});
		until(|| source.contents_calls.load(Ordering::SeqCst) == 2).await;
		tokio::task::yield_now().await;
		assert!(!ready(cache.held()).is_complete());
		source.held_gate.notify_one();
		task.await.unwrap();
		let library = ready(cache.held());
		assert!(library.is_complete());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t1()]), None);
		assert_eq!(library.holds(Target::Playlist(id(3)), &[t3()]), Some(true));
	}

	#[tokio::test]
	async fn a_playlist_that_couldnt_be_read_keeps_the_library_complete_while_it_is_read_again() {
		let source = Arc::new(fake());
		source.failing.lock().unwrap().insert(id(1));
		let cache = LibraryCache::default();
		cache.fetch(&*source, Instant::now()).await;
		source.failing.lock().unwrap().clear();
		source.held.lock().unwrap().insert(id(1));
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, Instant::now()).await }
		});
		until(|| source.contents_calls.load(Ordering::SeqCst) == 2).await;
		assert!(ready(cache.held()).is_complete());
		source.held_gate.notify_one();
		task.await.unwrap();
		assert_eq!(ready(cache.held()).holds(Target::Playlist(id(1)), &[t1()]), Some(true));
	}

	#[tokio::test(start_paused = true)]
	async fn a_fetch_that_runs_out_of_time_keeps_the_rows_it_read_and_settles_the_rest() {
		let slow = fake();
		add_c(&slow);
		slow.held.lock().unwrap().insert(id(3));
		let cache = LibraryCache::default();
		let now = Instant::now();
		let fetch = cache.snapshot(now).1.expect("no fetch started");
		fetch.run(async { Ok::<_, CoreError>(slow) }).await;
		let library = ready(cache.held());
		assert!(library.is_complete());
		assert_eq!(library.holds(Target::Playlist(id(1)), &[t1()]), Some(true));
		assert_eq!(library.holds(Target::Playlist(id(3)), &[t3()]), None);
		assert_eq!(library.holds(Target::LikedSongs, &[t2()]), Some(true));
		let source = fake();
		add_c(&source);
		let next_menu = now + LIBRARY_FETCH_TIMEOUT;
		let fetch = cache.snapshot(next_menu).1.expect("no fetch started");
		fetch.read(&source).await;
		assert_eq!(*source.contents_ids.lock().unwrap(), [id(3)]);
		assert_eq!(ready(cache.held()).holds(Target::Playlist(id(3)), &[t3()]), Some(true));
	}

	#[tokio::test]
	async fn a_failing_liked_songs_read_still_fails_the_library() {
		let source = fake();
		source.failing.lock().unwrap().insert(id(1));
		source.fail_liked.store(true, Ordering::SeqCst);
		let cache = LibraryCache::default();
		let now = Instant::now();
		cache.fetch(&source, now).await;
		let (state, fetch) = cache.snapshot(now);
		assert!(
			matches!(&state, LibraryState::Failed(reason) if reason == "Liked Songs failed"),
			"{state:?}"
		);
		assert!(fetch.is_some());
	}
}
