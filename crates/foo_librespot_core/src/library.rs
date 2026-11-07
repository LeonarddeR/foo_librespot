//! The logged-in account's playlists, the items of the ones it can change, and its Liked Songs.
//! A background fetch fills a shared cache, and readers take what it holds at that moment.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

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

use crate::link::{ItemId, Link, LinkKind, decode_search};
use crate::resolve::{liked_song_uris, retry_rate_limited};
use crate::{CoreError, SpotifyId};

/// The name Liked Songs goes by in menus.
pub const LIKED_SONGS: &str = "Liked Songs";

/// A library fetched longer ago than this is read again when a menu next needs it.
pub const LIBRARY_REFRESH: Duration = Duration::from_secs(60);

/// How long a fetch may run before the caller gives up on it.
pub const LIBRARY_FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a fetch that ran out of time failed.
pub(crate) const SLOW_REASON: &str = "Spotify took too long to send your playlists";

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
}

/// A playlist's contents with the set of its URIs.
#[derive(Clone, Debug)]
struct Held {
	contents: PlaylistContents,
	uris: HashSet<String>,
}

impl Held {
	fn new(contents: PlaylistContents) -> Self {
		let uris = contents.rows.iter().map(|row| row.uri.clone()).collect();
		Self { contents, uris }
	}
}

/// What was read from the account.
#[derive(Clone, Debug)]
pub struct Library {
	pub playlists: Vec<LibraryPlaylist>,
	items: HashMap<SpotifyId, Held>,
	liked: Option<HashSet<String>>,
}

impl Library {
	/// A library of `playlists` that keeps what `previous` knew of them: the items of each
	/// playlist, whatever their revision, and Liked Songs.
	fn new(playlists: Vec<LibraryPlaylist>, previous: Option<&Self>) -> Self {
		let items = previous
			.map(|previous| {
				playlists
					.iter()
					.filter(|playlist| playlist.can_edit)
					.filter_map(|playlist| {
						let held = previous.items.get(&playlist.id)?;
						Some((playlist.id, held.clone()))
					})
					.collect()
			})
			.unwrap_or_default();
		Self { playlists, items, liked: previous.and_then(|previous| previous.liked.clone()) }
	}

	/// A library holding exactly `playlists`, the `contents` of some of them, and `liked`.
	pub fn from_parts(
		playlists: Vec<LibraryPlaylist>,
		contents: Vec<(SpotifyId, PlaylistContents)>,
		liked: Option<HashSet<String>>,
	) -> Self {
		let items = contents.into_iter().map(|(id, contents)| (id, Held::new(contents))).collect();
		Self { playlists, items, liked }
	}

	/// Every playlist the account can change has its items, and Liked Songs is known.
	pub fn is_complete(&self) -> bool {
		self.liked.is_some()
			&& self.editable().all(|playlist| self.items.contains_key(&playlist.id))
	}

	pub fn editable(&self) -> impl Iterator<Item = &LibraryPlaylist> {
		self.playlists.iter().filter(|playlist| playlist.can_edit)
	}

	/// The rows of playlist `id`; `None` while they aren't known.
	pub fn contents(&self, id: SpotifyId) -> Option<&PlaylistContents> {
		self.items.get(&id).map(|held| &held.contents)
	}

	/// Whether playlist `id` holds every one of `uris`; `None` while its items aren't known.
	pub fn holds_all(&self, id: SpotifyId, uris: &[String]) -> Option<bool> {
		let held = self.items.get(&id)?;
		Some(uris.iter().all(|uri| held.uris.contains(uri)))
	}

	/// Whether Liked Songs holds every one of `uris`; `None` while it isn't known.
	pub fn liked_holds_all(&self, uris: &[String]) -> Option<bool> {
		let liked = self.liked.as_ref()?;
		Some(uris.iter().all(|uri| liked.contains(uri)))
	}

	/// How many songs Liked Songs holds; `None` while it isn't known.
	pub fn liked_count(&self) -> Option<usize> {
		self.liked.as_ref().map(HashSet::len)
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
	Failed(String),
	/// The playlist list is known; check [`Library::is_complete`] for the items.
	Ready(Arc<Library>),
}

/// Runs once when a fetch next publishes.
pub type OnLibraryChange = Box<dyn FnOnce() + Send>;

/// Where the library is read from.
pub trait LibrarySource: Send + Sync {
	/// The account's playlists, folders resolved, in the order Spotify lists them.
	fn rootlist(&self) -> impl Future<Output = Result<Vec<LibraryPlaylist>, CoreError>> + Send;

	/// The rows of one playlist.
	fn contents(
		&self,
		id: SpotifyId,
	) -> impl Future<Output = Result<PlaylistContents, CoreError>> + Send;

	/// The URIs in Liked Songs.
	fn liked(&self) -> impl Future<Output = Result<HashSet<String>, CoreError>> + Send;

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

/// `items` as URIs, each once, in order.
pub fn distinct_uris(items: &[ItemId]) -> Vec<String> {
	let mut seen = HashSet::new();
	items.iter().map(|item| item.uri().to_uri()).filter(|uri| seen.insert(uri.clone())).collect()
}

/// Which of `uris` `contents` lacks, in order, and how many of them it already holds.
pub fn missing(uris: &[String], contents: &PlaylistContents) -> (Vec<String>, usize) {
	let held: HashSet<&str> = contents.rows.iter().map(|row| row.uri.as_str()).collect();
	let lacking: Vec<String> =
		uris.iter().filter(|uri| !held.contains(uri.as_str())).cloned().collect();
	let already = uris.len() - lacking.len();
	(lacking, already)
}

/// Every row of `contents` holding one of `uris`, with its index.
pub fn rows_holding(uris: &[String], contents: &PlaylistContents) -> Vec<(usize, PlaylistRow)> {
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
	/// The token of the next fetch to start; never reset, so a token names one fetch ever.
	next_token: u64,
	library: Option<Arc<Library>>,
	failed: Option<String>,
	/// The token of the running fetch.
	running: Option<u64>,
	started: Option<Instant>,
	listeners: Vec<OnLibraryChange>,
	/// Counts the calls of `clear`; a change publishes only into the epoch it began in.
	epoch: u64,
	/// The playlists a change published since the running fetch began.
	changed_playlists: HashSet<SpotifyId>,
	/// A change published Liked Songs since the running fetch began.
	changed_liked: bool,
}

impl Inner {
	fn state(&self) -> LibraryState {
		match &self.library {
			Some(library) if library.is_complete() => LibraryState::Ready(Arc::clone(library)),
			library => match (&self.failed, library) {
				(Some(reason), _) => LibraryState::Failed(reason.clone()),
				(None, Some(library)) => LibraryState::Ready(Arc::clone(library)),
				(None, None) => LibraryState::Loading,
			},
		}
	}

	fn is_complete(&self) -> bool {
		self.library.as_ref().is_some_and(|library| library.is_complete())
	}
}

/// Names one running fetch: only the fetch holding the current ticket may publish, fail or end it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FetchTicket(u64);

/// Marks the fetch of its ticket as ended when dropped, whether it finished, was dropped
/// part-way or panicked. It leaves alone a fetch that has another ticket.
struct Running<'a> {
	cache: &'a LibraryCache,
	ticket: FetchTicket,
}

impl Drop for Running<'_> {
	fn drop(&mut self) {
		let mut inner = self.cache.lock();
		if inner.running == Some(self.ticket.0) {
			inner.running = None;
		}
	}
}

/// The library as last read, shared by everything that shows it.
#[derive(Clone, Default)]
pub struct LibraryCache(Arc<Mutex<Inner>>);

impl LibraryCache {
	fn lock(&self) -> MutexGuard<'_, Inner> {
		self.0.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// What is held, and whether the caller must now run `fetch`. True when no fetch runs and
	/// any of these holds: nothing is held, the held library is incomplete, the last fetch
	/// failed, or it started more than [`LIBRARY_REFRESH`] before `now`. A true answer marks a
	/// fetch as running. `on_change` runs once, at the next publish of a running fetch; when none
	/// runs, it is dropped unrun. Never returns `LoggedOut`.
	pub fn snapshot(
		&self,
		now: Instant,
		on_change: Option<OnLibraryChange>,
	) -> (LibraryState, bool) {
		let (state, ticket) = self.snapshot_ticket(now, on_change);
		(state, ticket.is_some())
	}

	/// [`snapshot`](Self::snapshot) that hands out the ticket of the fetch it starts.
	pub(crate) fn snapshot_ticket(
		&self,
		now: Instant,
		on_change: Option<OnLibraryChange>,
	) -> (LibraryState, Option<FetchTicket>) {
		let mut inner = self.lock();
		let state = inner.state();
		let start = inner.running.is_none()
			&& (inner.library.is_none()
				|| !inner.is_complete()
				|| inner.failed.is_some()
				|| inner.started.is_none_or(|started| {
					now.saturating_duration_since(started) > LIBRARY_REFRESH
				}));
		let ticket = start.then(|| {
			let ticket = FetchTicket(inner.next_token);
			inner.next_token += 1;
			inner.running = Some(ticket.0);
			inner.started = Some(now);
			inner.changed_playlists.clear();
			inner.changed_liked = false;
			ticket
		});
		if inner.running.is_some()
			&& let Some(on_change) = on_change
		{
			inner.listeners.push(on_change);
		}
		(state, ticket)
	}

	/// What is held, without starting anything.
	pub fn held(&self) -> LibraryState {
		self.lock().state()
	}

	/// Reads the playlist list and publishes it. Then it reads the items of each playlist the
	/// account can change whose revision is new, and Liked Songs, and publishes again. Items whose
	/// revision didn't change are kept, and a playlist whose revision changed keeps its old rows
	/// until the new ones arrive; if they can't be read, the old ones stay.
	pub async fn fetch<S: LibrarySource>(&self, source: &S, now: Instant) {
		let ticket = self.current_ticket(now);
		self.fetch_of(ticket, source).await;
	}

	/// The ticket of the running fetch, or of a new one when none runs.
	fn current_ticket(&self, now: Instant) -> FetchTicket {
		let mut inner = self.lock();
		if let Some(token) = inner.running {
			return FetchTicket(token);
		}
		let ticket = FetchTicket(inner.next_token);
		inner.next_token += 1;
		inner.running = Some(ticket.0);
		inner.started = Some(now);
		inner.changed_playlists.clear();
		inner.changed_liked = false;
		ticket
	}

	/// [`fetch`](Self::fetch) for the fetch `ticket` names; it does nothing, and changes nothing,
	/// once another ticket is current.
	pub(crate) async fn fetch_of<S: LibrarySource>(&self, ticket: FetchTicket, source: &S) {
		if self.lock().running != Some(ticket.0) {
			return;
		}
		let _running = Running { cache: self, ticket };
		let playlists = match source.rootlist().await {
			Ok(playlists) => playlists,
			Err(error) => {
				self.fetch_failed(ticket, &error);
				return;
			}
		};
		let unread = {
			let mut inner = self.lock();
			if inner.running != Some(ticket.0) {
				return;
			}
			let library = Library::new(playlists, inner.library.as_deref());
			let unread = library.unread();
			inner.library = Some(Arc::new(library));
			inner.failed = None;
			unread
		};
		self.notify(ticket);

		let read = async {
			stream::iter(unread)
				.map(|id| async move { (id, source.contents(id).await) })
				.buffer_unordered(PLAYLIST_CONCURRENCY)
				.collect::<Vec<_>>()
				.await
		};
		let (contents, liked) = futures_util::future::join(read, source.liked()).await;

		let listeners = {
			let mut inner = self.lock();
			if inner.running != Some(ticket.0) {
				return;
			}
			let mut library =
				inner.library.as_deref().cloned().unwrap_or_else(|| Library::new(Vec::new(), None));
			let mut first_error = None;
			for (id, result) in contents {
				match result {
					Ok(contents) => {
						// A change published a newer copy while this one was being read.
						if !inner.changed_playlists.contains(&id) {
							library.items.insert(id, Held::new(contents));
						}
					}
					Err(error) => {
						// A playlist that was read before keeps its old rows.
						log::warn!("couldn't read a Spotify playlist: {error}");
						first_error.get_or_insert_with(|| error.to_string());
					}
				}
			}
			match liked {
				Ok(liked) => {
					if !(inner.changed_liked && library.liked.is_some()) {
						library.liked = Some(liked);
					}
				}
				Err(error) => {
					log::warn!("couldn't read Liked Songs: {error}");
					first_error.get_or_insert_with(|| error.to_string());
				}
			}
			inner.failed = first_error.filter(|_| !library.is_complete());
			inner.library = Some(Arc::new(library));
			inner.running = None;
			std::mem::take(&mut inner.listeners)
		};
		run(listeners);
	}

	/// A fetch that couldn't start, for example because there is no session. It ends the fetch
	/// that is running; when none runs, it counts as one that started at the call, so the next
	/// refresh waits [`LIBRARY_REFRESH`].
	pub fn fail(&self, error: &CoreError) {
		let ticket = self.current_ticket(Instant::now());
		self.fetch_failed(ticket, error);
	}

	/// Runs `read`, the work of the fetch `ticket` names, for at most `limit`. When the time runs
	/// out, the fetch fails with [`SLOW_REASON`] while `read` is still alive, and `read` is
	/// dropped afterwards.
	pub(crate) async fn run_within(
		&self,
		ticket: FetchTicket,
		limit: Duration,
		read: impl Future<Output = ()> + Send,
	) {
		tokio::pin!(read);
		tokio::select! {
			() = &mut read => {}
			() = tokio::time::sleep(limit) => {
				self.fail_of(ticket, &CoreError::Unavailable(SLOW_REASON.to_owned()));
			}
		}
	}

	/// [`fail`](Self::fail) for the fetch `ticket` names; it does nothing once another ticket is
	/// current.
	pub(crate) fn fail_of(&self, ticket: FetchTicket, error: &CoreError) {
		self.fetch_failed(ticket, error);
	}

	/// Forgets everything, including what a running fetch would publish. Listeners waiting for
	/// that fetch run once, on the thread that calls this.
	pub fn clear(&self) {
		let listeners = {
			let mut inner = self.lock();
			let listeners = std::mem::take(&mut inner.listeners);
			let (next_token, epoch) = (inner.next_token, inner.epoch + 1);
			*inner = Inner { next_token, epoch, ..Inner::default() };
			listeners
		};
		run(listeners);
	}

	/// Adds `items` to `target`. A playlist is read first, so only what it lacks is sent; Liked
	/// Songs takes the tracks among `items` only.
	pub async fn add<S: LibrarySource>(
		&self,
		source: &S,
		target: Target,
		items: &[ItemId],
	) -> Result<Change, CoreError> {
		let epoch = self.lock().epoch;
		match target {
			Target::Playlist(id) => {
				let uris = distinct_uris(items);
				let fresh = source.contents(id).await?;
				let (lacking, already) = missing(&uris, &fresh);
				if !lacking.is_empty() {
					source.add(id, &fresh.revision, &lacking).await?;
					self.read_again(source, id, epoch).await;
				}
				Ok(Change { changed: lacking.len(), unchanged: already, episodes_left_out: 0 })
			}
			Target::LikedSongs => {
				let (tracks, episodes_left_out) = split_items(items);
				let held = self.lock().library.as_ref().and_then(|library| library.liked.clone());
				let lacking: Vec<String> = tracks
					.iter()
					.filter(|uri| held.as_ref().is_none_or(|held| !held.contains(*uri)))
					.cloned()
					.collect();
				if !lacking.is_empty() {
					source.set_liked(&lacking, true).await?;
					self.update_liked(epoch, |liked| liked.extend(lacking.iter().cloned()));
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
	/// from Liked Songs.
	pub async fn remove<S: LibrarySource>(
		&self,
		source: &S,
		target: Target,
		items: &[ItemId],
	) -> Result<Change, CoreError> {
		let epoch = self.lock().epoch;
		match target {
			Target::Playlist(id) => {
				let fresh = source.contents(id).await?;
				let rows = rows_holding(&distinct_uris(items), &fresh);
				if !rows.is_empty() {
					source.remove(id, &fresh.revision, &rows).await?;
					self.read_again(source, id, epoch).await;
				}
				Ok(Change { changed: rows.len(), ..Change::default() })
			}
			Target::LikedSongs => {
				let (tracks, episodes_left_out) = split_items(items);
				if !tracks.is_empty() {
					source.set_liked(&tracks, false).await?;
					self.update_liked(epoch, |liked| {
						for uri in &tracks {
							liked.remove(uri);
						}
					});
				}
				Ok(Change { changed: tracks.len(), unchanged: 0, episodes_left_out })
			}
		}
	}

	/// Reads playlist `id` after a change and publishes its rows and revision, unless `clear`
	/// has run since `epoch`. When the read fails, the held copy stays and the next snapshot
	/// starts a fetch.
	async fn read_again<S: LibrarySource>(&self, source: &S, id: SpotifyId, epoch: u64) {
		let result = source.contents(id).await;
		let mut inner = self.lock();
		if inner.epoch != epoch {
			return;
		}
		let contents = match result {
			Ok(contents) => contents,
			Err(error) => {
				log::warn!("couldn't read a Spotify playlist after changing it: {error}");
				inner.started = None;
				return;
			}
		};
		let Some(mut library) = inner.library.as_deref().cloned() else {
			return;
		};
		if let Some(playlist) = library.playlists.iter_mut().find(|playlist| playlist.id == id) {
			playlist.revision.clone_from(&contents.revision);
			library.items.insert(id, Held::new(contents));
			inner.library = Some(Arc::new(library));
			if inner.running.is_some() {
				inner.changed_playlists.insert(id);
			}
		}
	}

	/// Changes the held Liked Songs, when it is known and `clear` has not run since `epoch`.
	fn update_liked(&self, epoch: u64, change: impl FnOnce(&mut HashSet<String>)) {
		let mut inner = self.lock();
		if inner.epoch != epoch {
			return;
		}
		let Some(mut library) = inner.library.as_deref().cloned() else {
			return;
		};
		if let Some(liked) = library.liked.as_mut() {
			change(liked);
			inner.library = Some(Arc::new(library));
			if inner.running.is_some() {
				inner.changed_liked = true;
			}
		}
	}

	/// Ends the fetch of `ticket` with `error`: a complete library stays as it is.
	fn fetch_failed(&self, ticket: FetchTicket, error: &CoreError) {
		let listeners = {
			let mut inner = self.lock();
			if inner.running != Some(ticket.0) {
				return;
			}
			inner.running = None;
			if inner.is_complete() {
				log::warn!("couldn't refresh the Spotify library: {error}");
			} else {
				inner.failed = Some(error.to_string());
			}
			std::mem::take(&mut inner.listeners)
		};
		run(listeners);
	}

	/// Runs the listeners waiting for a publish of `ticket`'s fetch.
	fn notify(&self, ticket: FetchTicket) {
		let listeners = {
			let mut inner = self.lock();
			if inner.running != Some(ticket.0) {
				return;
			}
			std::mem::take(&mut inner.listeners)
		};
		run(listeners);
	}
}

/// The distinct tracks among `items` as URIs, and how many distinct episodes there are.
fn split_items(items: &[ItemId]) -> (Vec<String>, usize) {
	let tracks: Vec<ItemId> =
		items.iter().copied().filter(|item| matches!(item, ItemId::Track(_))).collect();
	let episodes: Vec<ItemId> =
		items.iter().copied().filter(|item| matches!(item, ItemId::Episode(_))).collect();
	(distinct_uris(&tracks), distinct_uris(&episodes).len())
}

fn run(listeners: Vec<OnLibraryChange>) {
	for listener in listeners {
		listener();
	}
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
	PlaylistContents { revision: message.revision().to_vec(), rows }
}

/// The library as Spotify has it, through librespot.
pub struct LibrespotLibrary {
	session: Session,
}

impl LibrespotLibrary {
	pub const fn new(session: Session) -> Self {
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
			let page = SelectedListContent::parse_from_bytes(&bytes)
				.map_err(librespot_core::Error::from)?;
			let Some(contents) = page.contents.as_ref() else {
				break;
			};
			let next = usize::try_from(contents.pos()).unwrap_or(from) + contents.items.len();
			let more = contents.truncated() && next > from;
			let all = whole.contents.mut_or_insert_default();
			all.items.extend(contents.items.iter().cloned());
			all.meta_items.extend(contents.meta_items.iter().cloned());
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

	async fn liked(&self) -> Result<HashSet<String>, CoreError> {
		let uris = liked_song_uris(&self.session).await?;
		Ok(uris.iter().map(librespot_core::SpotifyUri::to_uri).collect())
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
		let mut headers = HeaderMap::new();
		headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/x-protobuf"));
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
		Change, LIBRARY_FETCH_TIMEOUT, LIBRARY_REFRESH, LibraryCache, LibraryPlaylist,
		LibrarySource, LibraryState, PlaylistContents, PlaylistRow, SLOW_REASON, SourceClient,
		Target, add_changes, distinct_uris, liked_write, missing, playlist_contents,
		remove_changes, rootlist_playlists, rows_holding,
	};
	use crate::link::{ItemId, Link, LinkKind};
	use crate::{CoreError, SpotifyId};

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
			PlaylistContents { revision: b"r1".to_vec(), rows: rows(&["spotify:track:a"]) };
		let incomplete = super::Library::from_parts(playlists.clone(), Vec::new(), None);
		assert!(!incomplete.is_complete());
		let library = super::Library::from_parts(
			playlists,
			vec![(id(1), contents.clone())],
			Some(HashSet::from(["spotify:track:b".to_owned()])),
		);
		assert!(library.is_complete());
		assert_eq!(library.contents(id(1)), Some(&contents));
		assert_eq!(library.liked_count(), Some(1));
	}

	fn rows(uris: &[&str]) -> Vec<PlaylistRow> {
		uris.iter()
			.enumerate()
			.map(|(index, uri)| PlaylistRow {
				uri: (*uri).to_owned(),
				item_id: vec![u8::try_from(index).unwrap()],
			})
			.collect()
	}

	fn contents(revision: &str, uris: &[&str]) -> PlaylistContents {
		PlaylistContents { revision: revision.as_bytes().to_vec(), rows: rows(uris) }
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
			}
		);
	}

	type IndexedRow = (usize, PlaylistRow);

	#[derive(Default)]
	struct FakeLibrary {
		playlists: Mutex<Vec<LibraryPlaylist>>,
		contents: Mutex<HashMap<SpotifyId, PlaylistContents>>,
		liked: Mutex<HashSet<String>>,
		rootlist_calls: AtomicUsize,
		contents_calls: AtomicUsize,
		liked_calls: AtomicUsize,
		fail_rootlist: AtomicBool,
		fail_contents: AtomicBool,
		hang_rootlist: AtomicBool,
		/// When set, `contents` waits for it before answering.
		gate: Option<Arc<Notify>>,
		/// When set, the next `contents` answer is held back until `contents_gate` opens; it
		/// holds what the playlist was when the call began.
		hold_contents_once: AtomicBool,
		contents_gate: Arc<Notify>,
		/// The call number of `contents` that fails; 0 for none.
		fail_contents_call: AtomicUsize,
		/// When set, `liked` answers what Liked Songs was when the call began, once `liked_gate`
		/// opens.
		hold_liked: AtomicBool,
		liked_gate: Arc<Notify>,
		/// When set, `set_liked` waits for `set_liked_gate` after recording its call.
		hold_set_liked: AtomicBool,
		set_liked_gate: Arc<Notify>,
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
			if let Some(gate) = &self.gate {
				gate.notified().await;
			}
			if self.fail_contents.load(Ordering::SeqCst)
				|| self.fail_contents_call.load(Ordering::SeqCst) == call
			{
				return Err(CoreError::Unavailable("playlist failed".to_owned()));
			}
			let answer = self.contents.lock().unwrap().get(&id).expect("unknown playlist").clone();
			if self.hold_contents_once.swap(false, Ordering::SeqCst) {
				self.contents_gate.notified().await;
			}
			Ok(answer)
		}

		async fn liked(&self) -> Result<HashSet<String>, CoreError> {
			self.liked_calls.fetch_add(1, Ordering::SeqCst);
			tokio::task::yield_now().await;
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
					set.insert(uri.clone());
				} else {
					set.remove(uri);
				}
			}
			Ok(())
		}
	}

	/// A library of an editable playlist A holding `t1`, a followed playlist B, and one liked
	/// song `t2`.
	fn fake() -> FakeLibrary {
		let fake = FakeLibrary::default();
		*fake.playlists.lock().unwrap() =
			vec![playlist(1, "A", true, "r1"), playlist(2, "B", false, "r2")];
		fake.contents.lock().unwrap().insert(id(1), contents("r1", &["t1"]));
		fake.liked.lock().unwrap().insert("t2".to_owned());
		fake
	}

	fn track(byte: u8) -> ItemId {
		ItemId::Track(id(byte))
	}

	fn episode(byte: u8) -> ItemId {
		ItemId::Episode(id(byte))
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
	fn missing_lists_what_the_playlist_lacks_and_counts_the_rest() {
		let held = contents("r", &["a", "c"]);
		let (lacking, already) = missing(&strings(&["a", "b", "c", "d"]), &held);
		assert_eq!(lacking, strings(&["b", "d"]));
		assert_eq!(already, 2);
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
		source.contents.lock().unwrap().insert(id(1), contents("r1", &[&uri_of(track(1))]));
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
		source.contents.lock().unwrap().insert(id(1), contents("r1", &[&uri_of(track(1))]));
		let cache = loaded(&source).await;
		let change = cache.add(&source, Target::Playlist(id(1)), &[track(1)]).await.unwrap();
		assert_eq!(change, Change { changed: 0, unchanged: 1, episodes_left_out: 0 });
		assert!(source.adds.lock().unwrap().is_empty());
	}

	#[tokio::test]
	async fn removing_takes_every_copy_out() {
		let source = fake();
		source.contents.lock().unwrap().insert(
			id(1),
			contents("r1", &[&uri_of(track(1)), &uri_of(track(2)), &uri_of(track(1))]),
		);
		let cache = loaded(&source).await;
		let change = cache.remove(&source, Target::Playlist(id(1)), &[track(1)]).await.unwrap();
		assert_eq!(change.changed, 2);
		let removes = source.removes.lock().unwrap();
		assert_eq!(removes[0].0, b"r1".to_vec());
		assert_eq!(removes[0].1.iter().map(|(index, _)| *index).collect::<Vec<_>>(), vec![0, 2]);
		drop(removes);
		let library = ready(cache.held());
		assert_eq!(library.holds_all(id(1), &[uri_of(track(1))]), Some(false));
		assert_eq!(library.holds_all(id(1), &[uri_of(track(2))]), Some(true));
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
		let uris = [uri_of(track(4))];
		assert_eq!(ready(cache.held()).holds_all(id(1), &uris), Some(false));
		cache.add(&source, Target::Playlist(id(1)), &[track(4)]).await.unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds_all(id(1), &uris), Some(true));
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
		assert_eq!(ready(cache.held()).liked_holds_all(&[uri_of(track(1))]), Some(true));
	}

	#[tokio::test]
	async fn saving_counts_what_liked_songs_already_holds() {
		let source = fake();
		source.liked.lock().unwrap().insert(uri_of(track(1)));
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
		source.liked.lock().unwrap().extend([uri_of(track(1)), uri_of(track(2))]);
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
		assert_eq!(library.liked_holds_all(&[uri_of(track(1))]), Some(false));
		assert_eq!(library.liked_holds_all(&[uri_of(track(2))]), Some(false));
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
		assert_eq!(library.liked_holds_all(&[uri_of(track(1))]), Some(false));
	}

	#[tokio::test]
	async fn a_playlist_change_in_flight_over_a_logout_leaves_the_next_account_alone() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		source.hold_contents_once.store(true, Ordering::SeqCst);
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.add(&*source, Target::Playlist(id(1)), &[track(4)]).await }
		});
		until(|| !source.hold_contents_once.load(Ordering::SeqCst)).await;
		cache.clear();
		let other = fake();
		cache.fetch(&other, Instant::now()).await;
		source.contents_gate.notify_one();
		task.await.unwrap().unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds_all(id(1), &[uri_of(track(4))]), Some(false));
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
		assert_eq!(library.liked_holds_all(&[uri_of(track(1))]), Some(true));
	}

	#[tokio::test]
	async fn a_fetch_that_read_a_playlist_before_a_change_keeps_the_change() {
		let source = Arc::new(fake());
		let cache = loaded(&source).await;
		// The playlist changed elsewhere, so the next fetch reads it again.
		source.playlists.lock().unwrap()[0].revision = b"r2".to_vec();
		source.contents.lock().unwrap().get_mut(&id(1)).unwrap().revision = b"r2".to_vec();
		source.hold_contents_once.store(true, Ordering::SeqCst);
		let fetch = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, Instant::now()).await }
		});
		until(|| !source.hold_contents_once.load(Ordering::SeqCst)).await;
		cache.add(&*source, Target::Playlist(id(1)), &[track(4)]).await.unwrap();
		source.contents_gate.notify_one();
		fetch.await.unwrap();
		let library = ready(cache.held());
		assert_eq!(library.holds_all(id(1), &[uri_of(track(4))]), Some(true));
		assert_eq!(library.contents(id(1)).unwrap().revision, b"after".to_vec());
	}

	#[tokio::test]
	async fn a_failed_re_read_after_a_change_makes_the_next_snapshot_fetch() {
		let source = fake();
		let cache = loaded(&source).await;
		let (_, start) = cache.snapshot(Instant::now(), None);
		assert!(!start);
		// The read before the change is the next call; the read after it is the one after.
		let next = source.contents_calls.load(Ordering::SeqCst) + 2;
		source.fail_contents_call.store(next, Ordering::SeqCst);
		let change = cache.add(&source, Target::Playlist(id(1)), &[track(4)]).await.unwrap();
		assert_eq!(change.changed, 1);
		let (state, start) = cache.snapshot(Instant::now(), None);
		assert!(start);
		assert!(is_ready(&state));
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
		let (state, start) = cache.snapshot(now, None);
		assert!(matches!(state, LibraryState::Loading));
		assert!(start);
		let (state, start) = cache.snapshot(now, None);
		assert!(matches!(state, LibraryState::Loading));
		assert!(!start);
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
		assert_eq!(library.holds_all(id(1), &["t1".to_owned()]), None);
		gate.notify_one();
		task.await.unwrap();
		let library = ready(cache.held());
		assert!(library.is_complete());
		assert_eq!(library.holds_all(id(1), &["t1".to_owned()]), Some(true));
		assert_eq!(library.holds_all(id(1), &["t1".to_owned(), "t9".to_owned()]), Some(false));
		assert_eq!(library.holds_all(id(2), &["t1".to_owned()]), None);
		assert_eq!(library.liked_holds_all(&["t2".to_owned()]), Some(true));
		assert_eq!(library.liked_holds_all(&["t1".to_owned()]), Some(false));
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
	async fn unchanged_playlists_are_not_read_again() {
		let source = fake();
		let cache = LibraryCache::default();
		let now = Instant::now();
		cache.fetch(&source, now).await;
		cache.fetch(&source, now).await;
		assert_eq!(source.rootlist_calls.load(Ordering::SeqCst), 2);
		assert_eq!(source.contents_calls.load(Ordering::SeqCst), 1);

		source.playlists.lock().unwrap()[0].revision = b"r3".to_vec();
		source.contents.lock().unwrap().insert(id(1), contents("r3", &["t1", "t3"]));
		cache.fetch(&source, now).await;
		assert_eq!(source.contents_calls.load(Ordering::SeqCst), 2);
		let library = ready(cache.held());
		assert_eq!(library.holds_all(id(1), &["t3".to_owned()]), Some(true));
	}

	#[tokio::test]
	async fn a_fresh_library_starts_no_fetch_and_a_stale_one_does() {
		let source = fake();
		let cache = LibraryCache::default();
		let start = Instant::now();
		assert!(cache.snapshot(start, None).1);
		cache.fetch(&source, start).await;
		let (state, fetch) = cache.snapshot(start + Duration::from_secs(30), None);
		assert!(is_ready(&state));
		assert!(!fetch);
		let (state, fetch) = cache.snapshot(start + LIBRARY_REFRESH + Duration::from_secs(1), None);
		assert!(is_ready(&state));
		assert!(fetch);
	}

	#[tokio::test]
	async fn a_failed_first_fetch_says_why_and_tries_again() {
		let source = fake();
		source.fail_rootlist.store(true, Ordering::SeqCst);
		let cache = LibraryCache::default();
		let now = Instant::now();
		assert!(cache.snapshot(now, None).1);
		cache.fetch(&source, now).await;
		let (state, fetch) = cache.snapshot(now, None);
		assert!(
			matches!(&state, LibraryState::Failed(reason) if reason == "no network"),
			"{state:?}"
		);
		assert!(fetch);
	}

	#[tokio::test]
	async fn a_failed_refresh_keeps_the_complete_library() {
		let source = fake();
		let cache = LibraryCache::default();
		let start = Instant::now();
		cache.fetch(&source, start).await;
		source.fail_rootlist.store(true, Ordering::SeqCst);
		let later = start + LIBRARY_REFRESH + Duration::from_secs(1);
		let (_, fetch) = cache.snapshot(later, None);
		assert!(fetch);
		cache.fetch(&source, later).await;
		assert!(ready(cache.held()).is_complete());
		cache.fail(&CoreError::NotLoggedIn);
		assert!(ready(cache.held()).is_complete());
	}

	#[tokio::test]
	async fn a_listener_runs_once_when_the_fetch_publishes() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let now = Instant::now();
		let calls = Arc::new(AtomicUsize::new(0));
		let listener = {
			let calls = Arc::clone(&calls);
			Box::new(move || {
				calls.fetch_add(1, Ordering::SeqCst);
			})
		};
		assert!(cache.snapshot(now, Some(listener)).1);
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, now).await }
		});
		until(|| calls.load(Ordering::SeqCst) == 1).await;
		gate.notify_one();
		task.await.unwrap();
		assert_eq!(calls.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn a_listener_without_a_fetch_is_dropped() {
		let source = fake();
		let cache = LibraryCache::default();
		let start = Instant::now();
		cache.fetch(&source, start).await;
		let calls = Arc::new(AtomicUsize::new(0));
		let listener = {
			let calls = Arc::clone(&calls);
			Box::new(move || {
				calls.fetch_add(1, Ordering::SeqCst);
			})
		};
		let (_, fetch) = cache.snapshot(start + Duration::from_secs(1), Some(listener));
		assert!(!fetch);
		cache.fetch(&source, start).await;
		assert_eq!(calls.load(Ordering::SeqCst), 0);
	}

	#[tokio::test]
	async fn logging_out_forgets_the_library() {
		let source = fake();
		let cache = LibraryCache::default();
		let now = Instant::now();
		cache.fetch(&source, now).await;
		cache.clear();
		assert!(matches!(cache.held(), LibraryState::Loading));
		let (state, fetch) = cache.snapshot(now, None);
		assert!(matches!(state, LibraryState::Loading));
		assert!(fetch);
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
		source.contents.lock().unwrap().insert(id(1), contents("r3", &["t1", "t3"]));
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
		assert_eq!(library.holds_all(id(1), &["t1".to_owned()]), Some(true));
		assert_eq!(library.holds_all(id(1), &["t3".to_owned()]), Some(false));
		// The next refresh reads it again.
		source.fail_contents.store(false, Ordering::SeqCst);
		cache.fetch(&source, start).await;
		assert_eq!(ready(cache.held()).holds_all(id(1), &["t3".to_owned()]), Some(true));
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
		assert_eq!(ready(cache.held()).holds_all(id(1), &["t3".to_owned()]), Some(true));
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
		assert!(cache.snapshot(now, None).1);
		let result =
			tokio::time::timeout(Duration::from_millis(20), cache.fetch(&source, now)).await;
		assert!(result.is_err());
		assert!(cache.snapshot(now, None).1);
	}

	#[tokio::test]
	async fn a_clear_during_a_fetch_lets_the_next_snapshot_start_one() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let now = Instant::now();
		assert!(cache.snapshot(now, None).1);
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch(&*source, now).await }
		});
		until(|| is_ready(&cache.held())).await;
		cache.clear();
		assert!(cache.snapshot(now, None).1);
		gate.notify_one();
		task.await.unwrap();
		assert!(matches!(cache.held(), LibraryState::Loading));
		// The new fetch still runs: the old one's end must not mark it finished.
		assert!(!cache.snapshot(now, None).1);
	}

	#[tokio::test]
	async fn clearing_runs_a_waiting_listener_once() {
		let cache = LibraryCache::default();
		let calls = Arc::new(AtomicUsize::new(0));
		let listener = {
			let calls = Arc::clone(&calls);
			Box::new(move || {
				calls.fetch_add(1, Ordering::SeqCst);
			})
		};
		assert!(cache.snapshot(Instant::now(), Some(listener)).1);
		cache.clear();
		cache.clear();
		assert_eq!(calls.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn a_failure_with_an_old_ticket_leaves_a_newer_fetch_alone() {
		let cache = LibraryCache::default();
		let now = Instant::now();
		let (_, old) = cache.snapshot_ticket(now, None);
		let old = old.unwrap();
		cache.clear();
		let (_, new) = cache.snapshot_ticket(now, None);
		assert!(new.is_some());
		cache.fail_of(old, &CoreError::Unavailable("too slow".to_owned()));
		assert!(matches!(cache.held(), LibraryState::Loading));
		assert!(cache.snapshot_ticket(now, None).1.is_none());
	}

	#[tokio::test]
	async fn a_finished_fetch_leaves_the_mark_of_a_fetch_its_listener_started() {
		let gate = Arc::new(Notify::new());
		let source = Arc::new(FakeLibrary { gate: Some(Arc::clone(&gate)), ..fake() });
		let cache = LibraryCache::default();
		let now = Instant::now();
		let (_, ticket) = cache.snapshot_ticket(now, None);
		let ticket = ticket.unwrap();
		let task = tokio::spawn({
			let (cache, source) = (cache.clone(), Arc::clone(&source));
			async move { cache.fetch_of(ticket, &*source).await }
		});
		until(|| is_ready(&cache.held())).await;
		let later = now + LIBRARY_REFRESH + Duration::from_secs(1);
		let started = Arc::new(AtomicUsize::new(0));
		let listener = {
			let (cache, started) = (cache.clone(), Arc::clone(&started));
			Box::new(move || {
				if cache.snapshot_ticket(later, None).1.is_some() {
					started.fetch_add(1, Ordering::SeqCst);
				}
			})
		};
		assert!(cache.snapshot_ticket(later, Some(listener)).1.is_none());
		gate.notify_one();
		task.await.unwrap();
		assert_eq!(started.load(Ordering::SeqCst), 1);
		let much_later = later + LIBRARY_REFRESH + Duration::from_secs(1);
		assert!(cache.snapshot_ticket(much_later, None).1.is_none());
	}

	#[tokio::test(start_paused = true)]
	async fn a_fetch_that_runs_out_of_time_fails_and_runs_the_listeners() {
		let source = fake();
		source.hang_rootlist.store(true, Ordering::SeqCst);
		let cache = LibraryCache::default();
		let now = Instant::now();
		let calls = Arc::new(AtomicUsize::new(0));
		let listener = {
			let calls = Arc::clone(&calls);
			Box::new(move || {
				calls.fetch_add(1, Ordering::SeqCst);
			})
		};
		let ticket = cache.snapshot_ticket(now, Some(listener)).1.unwrap();
		cache.run_within(ticket, LIBRARY_FETCH_TIMEOUT, cache.fetch_of(ticket, &source)).await;
		assert!(
			matches!(cache.held(), LibraryState::Failed(reason) if reason == SLOW_REASON),
			"{:?}",
			cache.held()
		);
		assert_eq!(calls.load(Ordering::SeqCst), 1);
		assert!(cache.snapshot_ticket(now, None).1.is_some());
	}
}
