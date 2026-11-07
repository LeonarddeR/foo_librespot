//! The logged-in account's playlists, the items of the ones it can change, and its Liked Songs.
//! A background fetch fills a shared cache, and readers take what it holds at that moment.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use librespot_core::Session;
use librespot_protocol::playlist4_external::SelectedListContent;
use protobuf::Message;

use crate::link::{Link, LinkKind, decode_search};
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
						library.items.insert(id, Held::new(contents));
					}
					Err(error) => {
						// A playlist that was read before keeps its old rows.
						log::warn!("couldn't read a Spotify playlist: {error}");
						first_error.get_or_insert_with(|| error.to_string());
					}
				}
			}
			match liked {
				Ok(liked) => library.liked = Some(liked),
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
			let next_token = inner.next_token;
			*inner = Inner { next_token, ..Inner::default() };
			listeners
		};
		run(listeners);
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
		LIBRARY_FETCH_TIMEOUT, LIBRARY_REFRESH, LibraryCache, LibraryPlaylist, LibrarySource,
		LibraryState, PlaylistContents, PlaylistRow, SLOW_REASON, playlist_contents,
		rootlist_playlists,
	};
	use crate::link::{Link, LinkKind};
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
			self.contents_calls.fetch_add(1, Ordering::SeqCst);
			if let Some(gate) = &self.gate {
				gate.notified().await;
			}
			if self.fail_contents.load(Ordering::SeqCst) {
				return Err(CoreError::Unavailable("playlist failed".to_owned()));
			}
			Ok(self.contents.lock().unwrap().get(&id).expect("unknown playlist").clone())
		}

		async fn liked(&self) -> Result<HashSet<String>, CoreError> {
			self.liked_calls.fetch_add(1, Ordering::SeqCst);
			tokio::task::yield_now().await;
			Ok(self.liked.lock().unwrap().clone())
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
