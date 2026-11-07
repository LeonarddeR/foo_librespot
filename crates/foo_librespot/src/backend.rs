//! The Spotify side of the component: `foo_librespot_core::Core` behind a small trait, which the
//! services' tests replace with a fake.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use foo_librespot_core::audio::format::FileFormat;
use foo_librespot_core::audio::track::SpotifyTrack;
use foo_librespot_core::library::{Edit, LibraryState, OnChanged, Subscribers, Target};
use foo_librespot_core::link::{ItemId, Link};
use foo_librespot_core::login::DeviceCode;
use foo_librespot_core::metadata::TrackInfo;
use foo_librespot_core::related::{Found, Related};
use foo_librespot_core::resolve::{ArtistTracks, Resolved};
use foo_librespot_core::{Core, CoreError, LoginHandle};
use foxide2k::host::host;

use crate::lifecycle::{Lifecycle, Phase};
use crate::settings;

/// Gets the code a login shows the user.
pub type OnCode = Box<dyn FnOnce(DeviceCode) + Send>;
/// Gets the Spotify username, or why a login failed.
pub type OnDone = Box<dyn FnOnce(Result<String, CoreError>) + Send>;
/// Gets what a lookup found, or why it failed.
pub type OnRelated = Box<dyn FnOnce(Result<Found, CoreError>) + Send>;

/// Everything the services need from Spotify. Every call that takes `is_aborting` blocks and
/// returns [`CoreError::Aborted`] soon after it turns true.
pub trait Backend: Send + Sync {
	fn resolve(
		&self,
		link: &Link,
		artist_tracks: ArtistTracks,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Resolved, CoreError>;
	fn track_info(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError>;
	fn open_track(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Box<dyn PcmTrack>, CoreError>;
	fn cover(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError>;
	/// Starts a browser login in the background. `on_code` gets the code to show; `on_done` gets
	/// the Spotify username or why it failed. Neither runs after [`LoginTask::cancel`] returns true.
	fn start_login(
		&self,
		on_code: OnCode,
		on_done: OnDone,
	) -> Result<Box<dyn LoginTask>, CoreError>;
	fn logout(&self) -> Result<(), CoreError>;
	/// The Spotify username of the stored login, or `None` when logged out. An error means the
	/// stored login can't be read, for example because Spotify's core couldn't start.
	fn username(&self) -> Result<Option<String>, CoreError>;
	/// The account's playlists as last read, without waiting for Spotify. Starts a read when none
	/// runs and the held one is missing, incomplete or old, or the last read failed. Before
	/// Spotify's core runs, a thread starts it and the answer is `Loading`; the subscribers hear
	/// once it is up, or couldn't start, which makes the answer `Failed`.
	fn library(&self) -> LibraryState;
	/// What [`Backend::library`] would answer, without starting a read.
	fn held_library(&self) -> LibraryState;
	/// Calls `notify` after each publish and failure of a library read, and after each logout
	/// and login, on any thread. Returns the id to
	/// [`unsubscribe_library`](Self::unsubscribe_library) with.
	fn subscribe_library(&self, notify: Box<dyn Fn() + Send + Sync>) -> u64;
	fn unsubscribe_library(&self, id: u64);
	/// Starts adding `items` to a playlist or Liked Songs, leaving out what is already there, or
	/// removing them from it, once every change begun before has ended, and returns at once.
	/// `on_done` gets what was done, on another thread, which waits for Spotify's core to start
	/// when it doesn't run yet; a change still running when foobar2000 quits is dropped
	/// unreported.
	fn change(&self, edit: Edit, target: Target, items: &[ItemId], on_done: OnChanged);
	/// Starts finding the album, artist or show `item` belongs to, or a track's radio, and
	/// returns at once. `on_done` gets what was found, on another thread, which waits for
	/// Spotify's core to start when it doesn't run yet; a lookup still running when foobar2000
	/// quits is dropped unreported.
	fn related(&self, item: ItemId, what: Related, on_done: OnRelated);
}

/// A login running in the background.
pub trait LoginTask: Send {
	/// Stops the login. True when it was stopped in time: nothing is stored and `on_done` isn't
	/// called. False when it had already finished; `on_done` gets its result.
	fn cancel(&self) -> bool;
}

impl LoginTask for LoginHandle {
	fn cancel(&self) -> bool {
		Self::cancel(self)
	}
}

/// A track opened for decoding: 44.1 kHz stereo, interleaved `f64`.
pub trait PcmTrack: Send {
	fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError>;
	fn seek(&mut self, frame: u64) -> Result<(), CoreError>;
	/// The codec and bitrate of the audio file Spotify sent.
	fn file_format(&self) -> FileFormat;
}

impl PcmTrack for SpotifyTrack {
	fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError> {
		Self::read(self, out, max_frames)
	}

	fn seek(&mut self, frame: u64) -> Result<(), CoreError> {
		Self::seek(self, frame)
	}

	fn file_format(&self) -> FileFormat {
		self.format().into()
	}
}

/// Where foo_librespot keeps its state: `foo_librespot` in the foobar2000 profile, which the
/// host reports with a `file://` prefix.
pub fn profile_folder(profile_path: &str) -> PathBuf {
	let path = profile_path.strip_prefix("file://").unwrap_or(profile_path);
	Path::new(path.trim_end_matches(['\\', '/'])).join("foo_librespot")
}

/// How long quitting waits for Spotify calls still running.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// A `Core`, started the first time anything asks Spotify and stopped when foobar2000 quits,
/// with the subscribers of its library, which every core it starts tells.
struct CoreSlot {
	core: Lifecycle<Core>,
	start: fn() -> Result<Core, CoreError>,
	subscribers: Subscribers,
	/// A thread is bringing the core up for the library.
	starting: AtomicBool,
}

impl CoreSlot {
	const fn new(start: fn() -> Result<Core, CoreError>) -> Self {
		Self {
			core: Lifecycle::new(),
			start,
			subscribers: Subscribers::new(),
			starting: AtomicBool::new(false),
		}
	}

	/// Brings the core up on a new thread, unless one is doing so already. Once it is up, that
	/// thread reads the library; either way it then tells the subscribers.
	fn start_in_background(&'static self) {
		if self.starting.swap(true, Ordering::SeqCst) {
			return;
		}
		let spawned =
			std::thread::Builder::new().name("foo_librespot start".to_owned()).spawn(move || {
				let started = {
					let _ended = StartEnded(self);
					self.core()
				};
				if let Ok(core) = started {
					core.library();
				}
				self.subscribers.notify();
			});
		if let Err(error) = spawned {
			self.starting.store(false, Ordering::SeqCst);
			log::warn!("couldn't start Spotify's core: {error}");
		}
	}

	/// What the library holds when the core is at `phase`: `Loading` until it runs, or why it
	/// couldn't start.
	fn held(phase: Phase<Core>) -> LibraryState {
		match phase {
			Phase::Running(core) => core.held_library(),
			Phase::NotStarted | Phase::Starting => LibraryState::Loading,
			Phase::Failed(reason) => LibraryState::Failed(reason),
			Phase::ShutDown => LibraryState::Failed(CoreError::Aborted.to_string()),
		}
	}

	/// The running core, started on this thread when none runs.
	fn core(&'static self) -> Result<Arc<Core>, CoreError> {
		self.core.get_or_start(|| {
			let core = (self.start)()?;
			core.subscribe_library(Box::new(|| self.subscribers.notify()));
			Ok(core)
		})
	}
}

/// Clears [`CoreSlot::starting`] when a background start ends, also when it panics.
struct StartEnded(&'static CoreSlot);

impl Drop for StartEnded {
	fn drop(&mut self) {
		self.0.starting.store(false, Ordering::SeqCst);
	}
}

/// The `Core` of foobar2000's profile.
static SPOTIFY: CoreSlot = CoreSlot::new(|| Core::new(profile_folder(&host().profile_path())));

/// Stops `Core` for good, if it was started; later calls fail instead of starting it again.
pub fn shutdown() {
	SPOTIFY.core.shutdown(SHUTDOWN_WAIT, Core::shutdown);
}

/// Spotify through the core in `slot`.
#[derive(Clone, Copy)]
struct CoreBackend {
	slot: &'static CoreSlot,
}

impl Backend for CoreBackend {
	fn resolve(
		&self,
		link: &Link,
		artist_tracks: ArtistTracks,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Resolved, CoreError> {
		self.slot.core()?.resolve(link, artist_tracks, is_aborting)
	}

	fn track_info(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError> {
		self.slot.core()?.track_info(id, is_aborting)
	}

	fn open_track(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Box<dyn PcmTrack>, CoreError> {
		Ok(Box::new(self.slot.core()?.open_track(id, settings::bitrate(), is_aborting)?))
	}

	fn cover(
		&self,
		id: ItemId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError> {
		self.slot.core()?.cover(id, is_aborting)
	}

	fn start_login(
		&self,
		on_code: OnCode,
		on_done: OnDone,
	) -> Result<Box<dyn LoginTask>, CoreError> {
		Ok(Box::new(self.slot.core()?.start_login(move |code| on_code(code.clone()), on_done)))
	}

	fn logout(&self) -> Result<(), CoreError> {
		self.slot.core()?.logout()
	}

	fn username(&self) -> Result<Option<String>, CoreError> {
		Ok(self.slot.core()?.username())
	}

	/// Before the core runs, starts it on another thread and answers what it holds.
	fn library(&self) -> LibraryState {
		let phase = self.slot.core.now();
		if let Phase::Running(core) = &phase {
			return core.library();
		}
		if !matches!(phase, Phase::ShutDown) {
			self.slot.start_in_background();
		}
		CoreSlot::held(phase)
	}

	fn held_library(&self) -> LibraryState {
		CoreSlot::held(self.slot.core.now())
	}

	fn subscribe_library(&self, notify: Box<dyn Fn() + Send + Sync>) -> u64 {
		self.slot.subscribers.subscribe(notify)
	}

	fn unsubscribe_library(&self, id: u64) {
		self.slot.subscribers.unsubscribe(id);
	}

	/// Before the core runs, waits for it on another thread. Once the core is shut down,
	/// `on_done` isn't called.
	fn change(&self, edit: Edit, target: Target, items: &[ItemId], on_done: OnChanged) {
		let slot = self.slot;
		let on_done: OnChanged = Box::new(move |result| {
			if !matches!(slot.core.now(), Phase::ShutDown) {
				on_done(result);
			}
		});
		if let Phase::Running(core) = self.slot.core.now() {
			return core.change(edit, target, items, on_done);
		}
		let items = items.to_vec();
		let spawned =
			std::thread::Builder::new().name("foo_librespot change".to_owned()).spawn(move || {
				match slot.core() {
					Ok(core) => core.change(edit, target, &items, on_done),
					Err(error) => on_done(Err(error)),
				}
			});
		if let Err(error) = spawned {
			log::warn!("couldn't start a Spotify playlist change: {error}");
		}
	}

	/// Looks up on a thread of its own, which starts the core when it doesn't run yet. Once the
	/// core is shut down, the lookup stops and `on_done` isn't called.
	fn related(&self, item: ItemId, what: Related, on_done: OnRelated) {
		let slot = self.slot;
		let spawned =
			std::thread::Builder::new().name("foo_librespot related".to_owned()).spawn(move || {
				let shut_down = || matches!(slot.core.now(), Phase::ShutDown);
				let result = slot.core().and_then(|core| core.related(item, &what, &shut_down));
				if !shut_down() {
					on_done(result);
				}
			});
		if let Err(error) = spawned {
			log::warn!("couldn't start a Spotify lookup: {error}");
		}
	}
}

static BACKEND: OnceLock<Box<dyn Backend>> = OnceLock::new();

/// The backend the services use: Spotify, or the one a test installed.
pub fn backend() -> &'static dyn Backend {
	BACKEND.get_or_init(|| Box::new(CoreBackend { slot: &SPOTIFY })).as_ref()
}

/// Makes `backend()` return `fake` in this test process.
#[cfg(test)]
pub fn install(fake: Box<dyn Backend>) {
	assert!(BACKEND.set(fake).is_ok(), "a backend was already in use");
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::sync::mpsc;
	use std::time::{Duration, Instant};

	use foo_librespot_core::library::{Edit, LibraryState, Target};
	use foo_librespot_core::link::ItemId;
	use foo_librespot_core::related::Related;
	use foo_librespot_core::{Core, CoreError};

	use super::{Backend, CoreBackend, CoreSlot, profile_folder};
	use crate::lifecycle::Phase;
	use crate::testing::{track_id, wait_until};

	/// Takes a while, then fails as a core whose profile folder is busy does.
	fn slow_start() -> Result<Core, CoreError> {
		std::thread::sleep(Duration::from_millis(500));
		Err(CoreError::Unavailable("the profile folder is busy".to_owned()))
	}

	/// How many times the slot's library subscribers were told.
	fn heard(backend: CoreBackend) -> Arc<AtomicUsize> {
		let heard = Arc::new(AtomicUsize::new(0));
		let count = Arc::clone(&heard);
		backend.subscribe_library(Box::new(move || {
			count.fetch_add(1, Ordering::SeqCst);
		}));
		heard
	}

	fn is_busy(state: &LibraryState) -> bool {
		matches!(state, LibraryState::Failed(reason) if reason == "the profile folder is busy")
	}

	#[test]
	fn before_the_core_starts_a_library_call_answers_loading_at_once() {
		static SLOW: CoreSlot = CoreSlot::new(slow_start);
		let backend = CoreBackend { slot: &SLOW };
		let heard = heard(backend);
		let asked = Instant::now();
		assert!(matches!(backend.held_library(), LibraryState::Loading));
		assert!(matches!(backend.library(), LibraryState::Loading));
		assert!(matches!(backend.held_library(), LibraryState::Loading));
		assert!(asked.elapsed() < Duration::from_millis(250), "{:?}", asked.elapsed());
		wait_until(|| heard.load(Ordering::SeqCst) == 1);
		assert!(is_busy(&backend.held_library()), "{:?}", backend.held_library());
	}

	#[test]
	fn a_change_before_the_core_starts_waits_for_it_off_the_calling_thread() {
		static SLOW: CoreSlot = CoreSlot::new(slow_start);
		let backend = CoreBackend { slot: &SLOW };
		let (done, result) = mpsc::channel();
		let asked = Instant::now();
		backend.change(
			Edit::Add,
			Target::LikedSongs,
			&[ItemId::Track(track_id())],
			Box::new(move |result| {
				let _ = done.send((result, std::thread::current().id()));
			}),
		);
		assert!(asked.elapsed() < Duration::from_millis(250), "{:?}", asked.elapsed());
		let (result, thread) = result.recv_timeout(Duration::from_secs(5)).unwrap();
		assert!(
			matches!(&result, Err(CoreError::Unavailable(reason)) if reason == "the profile folder is busy"),
			"{result:?}"
		);
		assert_ne!(thread, std::thread::current().id());
	}

	#[test]
	fn a_change_that_ends_after_shutdown_reports_nothing() {
		static QUITTING: CoreSlot = CoreSlot::new(slow_start);
		let backend = CoreBackend { slot: &QUITTING };
		let (done, result) = mpsc::channel();
		backend.change(
			Edit::Add,
			Target::LikedSongs,
			&[ItemId::Track(track_id())],
			Box::new(move |result| {
				let _ = done.send(result);
			}),
		);
		wait_until(|| matches!(QUITTING.core.now(), Phase::Starting));
		QUITTING.core.shutdown(Duration::ZERO, Core::shutdown);
		assert!(result.recv_timeout(Duration::from_secs(2)).is_err());
	}

	#[test]
	fn a_lookup_before_the_core_starts_waits_for_it_off_the_calling_thread() {
		static SLOW: CoreSlot = CoreSlot::new(slow_start);
		let backend = CoreBackend { slot: &SLOW };
		let (done, result) = mpsc::channel();
		let asked = Instant::now();
		backend.related(
			ItemId::Track(track_id()),
			Related::Album,
			Box::new(move |result| {
				let _ = done.send((result, std::thread::current().id()));
			}),
		);
		assert!(asked.elapsed() < Duration::from_millis(250), "{:?}", asked.elapsed());
		let (result, thread) = result.recv_timeout(Duration::from_secs(5)).unwrap();
		assert!(
			matches!(&result, Err(CoreError::Unavailable(reason)) if reason == "the profile folder is busy"),
			"{result:?}"
		);
		assert_ne!(thread, std::thread::current().id());
	}

	#[test]
	fn a_lookup_that_ends_after_shutdown_reports_nothing() {
		static QUITTING: CoreSlot = CoreSlot::new(slow_start);
		let backend = CoreBackend { slot: &QUITTING };
		let (done, result) = mpsc::channel();
		backend.related(
			ItemId::Track(track_id()),
			Related::Radio,
			Box::new(move |result| {
				let _ = done.send(result);
			}),
		);
		wait_until(|| matches!(QUITTING.core.now(), Phase::Starting));
		QUITTING.core.shutdown(Duration::ZERO, Core::shutdown);
		assert!(result.recv_timeout(Duration::from_secs(2)).is_err());
	}

	static PANICKED_STARTS: AtomicUsize = AtomicUsize::new(0);

	fn panicking_start() -> Result<Core, CoreError> {
		PANICKED_STARTS.fetch_add(1, Ordering::SeqCst);
		panic!("the start panicked");
	}

	#[test]
	fn a_start_that_panics_lets_the_next_library_call_try_again() {
		static PANICS: CoreSlot = CoreSlot::new(panicking_start);
		let backend = CoreBackend { slot: &PANICS };
		backend.library();
		wait_until(|| {
			PANICKED_STARTS.load(Ordering::SeqCst) == 1 && !PANICS.starting.load(Ordering::SeqCst)
		});
		backend.library();
		wait_until(|| PANICKED_STARTS.load(Ordering::SeqCst) == 2);
	}

	#[test]
	fn a_library_read_after_shutdown_says_why() {
		static GONE: CoreSlot = CoreSlot::new(slow_start);
		GONE.core.shutdown(Duration::ZERO, Core::shutdown);
		let backend = CoreBackend { slot: &GONE };
		let state = backend.library();
		assert!(matches!(state, LibraryState::Failed(_)), "{state:?}");
		let state = backend.held_library();
		assert!(matches!(state, LibraryState::Failed(_)), "{state:?}");
	}

	#[test]
	fn profile_folder_strips_the_file_scheme() {
		assert_eq!(
			profile_folder(r"file://C:\fb2k\profile"),
			PathBuf::from(r"C:\fb2k\profile\foo_librespot")
		);
		assert_eq!(
			profile_folder(r"C:\fb2k\profile\"),
			PathBuf::from(r"C:\fb2k\profile\foo_librespot")
		);
	}
}
