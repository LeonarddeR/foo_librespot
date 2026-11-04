//! A fake Spotify for the service tests: one Keith & Kristyn Getty track whose audio is the
//! chirp fixture of `foo_librespot_core`.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use foo_librespot_core::audio::decoder::PcmDecoder;
use foo_librespot_core::link::Link;
use foo_librespot_core::login::DeviceCode;
use foo_librespot_core::metadata::TrackInfo;
use foo_librespot_core::replaygain::ReplayGain;
use foo_librespot_core::resolve::Resolved;
use foo_librespot_core::{CoreError, SpotifyId};

use crate::backend::{Backend, LoginTask, OnCode, OnDone, PcmTrack};

pub const CHIRP: &[u8] = include_bytes!("../../foo_librespot_core/tests/fixtures/chirp.ogg");

/// "In Christ Alone" by Keith & Kristyn Getty.
pub const TRACK_ID: &str = "2hQxLiI93xdjqENJuKeXjO";

pub fn track_id() -> SpotifyId {
	SpotifyId::from_base62(TRACK_ID).unwrap()
}

pub fn track_info(id: SpotifyId, title: &str) -> TrackInfo {
	TrackInfo {
		id,
		title: title.to_owned(),
		artists: vec!["Keith Getty".to_owned(), "Kristyn Getty".to_owned()],
		album: "In Christ Alone".to_owned(),
		album_artists: vec!["Keith & Kristyn Getty".to_owned()],
		track_number: 4,
		disc_number: 1,
		date: Some("2007-01-02".to_owned()),
		duration_ms: 279_000,
		covers: Vec::new(),
		has_audio: true,
		unavailable: None,
		replay_gain: Some(ReplayGain {
			track_gain_db: -6.62,
			track_peak: 1.0127,
			album_gain_db: -8.51,
			album_peak: 1.0724,
		}),
	}
}

#[derive(Default)]
struct State {
	calls: AtomicUsize,
	open_calls: AtomicUsize,
	failure: Mutex<Option<fn() -> CoreError>>,
	resolved: Mutex<Option<Resolved>>,
	cover: Mutex<Option<Vec<u8>>>,
	/// While set, reads and seeks block, like a Spotify download that has stalled.
	stalled: AtomicBool,
	logins: Mutex<Vec<ScriptedLogin>>,
	username: Mutex<Option<String>>,
	logouts: AtomicUsize,
	/// While set, Spotify calls fail with `NotLoggedIn` until a login is stored.
	require_login: AtomicBool,
}

/// A login the test answers itself, with [`FakeBackend::send_code`] and
/// [`FakeBackend::finish_login`].
struct ScriptedLogin {
	on_code: Option<OnCode>,
	on_done: Option<OnDone>,
	cancelled: Arc<AtomicBool>,
}

struct ScriptedTask(Arc<AtomicBool>);

impl LoginTask for ScriptedTask {
	fn cancel(&self) {
		self.0.store(true, Ordering::SeqCst);
	}
}

/// Clones share their state, so a test can install one and inspect another.
#[derive(Clone, Default)]
pub struct FakeBackend(Arc<State>);

impl FakeBackend {
	/// Every call fails with `error()` from now on.
	pub fn fail_with(&self, error: fn() -> CoreError) {
		*self.0.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
	}

	pub fn resolve_to(&self, resolved: Resolved) {
		*self.0.resolved.lock().unwrap_or_else(PoisonError::into_inner) = Some(resolved);
	}

	pub fn set_cover(&self, bytes: Vec<u8>) {
		*self.0.cover.lock().unwrap_or_else(PoisonError::into_inner) = Some(bytes);
	}

	/// How many backend calls there have been.
	pub fn calls(&self) -> usize {
		self.0.calls.load(Ordering::SeqCst)
	}

	pub fn open_calls(&self) -> usize {
		self.0.open_calls.load(Ordering::SeqCst)
	}

	/// Makes reads and seeks of opened tracks block until `stall(false)`, at most 5 s.
	pub fn stall(&self, stalled: bool) {
		self.0.stalled.store(stalled, Ordering::SeqCst);
	}

	/// Spotify calls fail with `NotLoggedIn` until a login is stored, as for a new user.
	pub fn require_login(&self) {
		self.0.require_login.store(true, Ordering::SeqCst);
	}

	/// The stored login's username, as if a login had happened before.
	pub fn set_username(&self, username: Option<&str>) {
		*self.0.username.lock().unwrap_or_else(PoisonError::into_inner) =
			username.map(str::to_owned);
	}

	pub fn logins_started(&self) -> usize {
		self.0.logins.lock().unwrap_or_else(PoisonError::into_inner).len()
	}

	pub fn last_login_cancelled(&self) -> bool {
		self.0
			.logins
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.last()
			.is_some_and(|login| login.cancelled.load(Ordering::SeqCst))
	}

	pub fn logouts(&self) -> usize {
		self.0.logouts.load(Ordering::SeqCst)
	}

	/// The latest login receives its code from Spotify. Unlike the real one, the fake answers
	/// even after it was cancelled, so tests can check that late answers are ignored.
	pub fn send_code(&self, code: DeviceCode) {
		let on_code = self.latest(|login| login.on_code.take());
		if let Some(on_code) = on_code {
			on_code(code);
		}
	}

	/// The latest login ends. Success stores the login, as the real one does, unless it was
	/// cancelled.
	pub fn finish_login(&self, result: Result<String, CoreError>) {
		let (on_done, cancelled) =
			self.latest(|login| (login.on_done.take(), login.cancelled.load(Ordering::SeqCst)));
		if let (Ok(username), false) = (&result, cancelled) {
			self.set_username(Some(username));
		}
		if let Some(on_done) = on_done {
			on_done(result);
		}
	}

	fn latest<R>(&self, take: impl FnOnce(&mut ScriptedLogin) -> R) -> R {
		let mut logins = self.0.logins.lock().unwrap_or_else(PoisonError::into_inner);
		take(logins.last_mut().expect("no login was started"))
	}

	fn enter(&self, is_aborting: &dyn Fn() -> bool) -> Result<(), CoreError> {
		self.0.calls.fetch_add(1, Ordering::SeqCst);
		if is_aborting() {
			return Err(CoreError::Aborted);
		}
		if self.0.require_login.load(Ordering::SeqCst) && self.username().is_none() {
			return Err(CoreError::NotLoggedIn);
		}
		let failure = *self.0.failure.lock().unwrap_or_else(PoisonError::into_inner);
		failure.map_or(Ok(()), |error| Err(error()))
	}
}

impl Backend for FakeBackend {
	fn resolve(&self, _link: &Link, is_aborting: &dyn Fn() -> bool) -> Result<Resolved, CoreError> {
		self.enter(is_aborting)?;
		Ok(self
			.0
			.resolved
			.lock()
			.unwrap_or_else(PoisonError::into_inner)
			.clone()
			.expect("resolve_to"))
	}

	fn track_info(
		&self,
		id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Arc<TrackInfo>, CoreError> {
		self.enter(is_aborting)?;
		Ok(Arc::new(track_info(id, "In Christ Alone")))
	}

	fn open_track(
		&self,
		_id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Box<dyn PcmTrack>, CoreError> {
		self.0.open_calls.fetch_add(1, Ordering::SeqCst);
		self.enter(is_aborting)?;
		let decoder = PcmDecoder::new(Box::new(Cursor::new(CHIRP)), Some("audio/ogg"))?;
		Ok(Box::new(FixtureTrack { decoder, state: Arc::clone(&self.0) }))
	}

	fn cover(
		&self,
		_id: SpotifyId,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<Option<Vec<u8>>, CoreError> {
		self.enter(is_aborting)?;
		Ok(self.0.cover.lock().unwrap_or_else(PoisonError::into_inner).clone())
	}

	fn start_login(
		&self,
		on_code: OnCode,
		on_done: OnDone,
	) -> Result<Box<dyn LoginTask>, CoreError> {
		let cancelled = Arc::new(AtomicBool::new(false));
		self.0.logins.lock().unwrap_or_else(PoisonError::into_inner).push(ScriptedLogin {
			on_code: Some(on_code),
			on_done: Some(on_done),
			cancelled: Arc::clone(&cancelled),
		});
		Ok(Box::new(ScriptedTask(cancelled)))
	}

	fn logout(&self) -> Result<(), CoreError> {
		self.0.logouts.fetch_add(1, Ordering::SeqCst);
		self.set_username(None);
		Ok(())
	}

	fn username(&self) -> Option<String> {
		self.0.username.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}
}

/// The fixture, decoded.
struct FixtureTrack {
	decoder: PcmDecoder,
	state: Arc<State>,
}

impl FixtureTrack {
	fn wait_while_stalled(&self) {
		let started = std::time::Instant::now();
		while self.state.stalled.load(Ordering::SeqCst)
			&& started.elapsed() < std::time::Duration::from_secs(5)
		{
			std::thread::sleep(std::time::Duration::from_millis(10));
		}
	}
}

impl PcmTrack for FixtureTrack {
	fn read(&mut self, out: &mut Vec<f64>, max_frames: usize) -> Result<usize, CoreError> {
		self.wait_while_stalled();
		self.decoder.read(out, max_frames)
	}

	fn seek(&mut self, frame: u64) -> Result<(), CoreError> {
		self.wait_while_stalled();
		self.decoder.seek(frame)
	}

	fn codec(&self) -> &'static str {
		"Vorbis"
	}

	/// Not the preferred 320, so tests see the file's own bitrate.
	fn kbps(&self) -> Option<u32> {
		Some(160)
	}
}

/// The whole fixture, decoded straight through.
pub fn fixture_samples() -> Vec<f64> {
	let mut decoder = PcmDecoder::new(Box::new(Cursor::new(CHIRP)), Some("audio/ogg")).unwrap();
	let (mut all, mut buffer) = (Vec::new(), Vec::new());
	while decoder.read(&mut buffer, 4096).unwrap() > 0 {
		all.extend_from_slice(&buffer);
	}
	all
}

/// A login prompt that only counts how often it was shown.
#[derive(Clone, Default)]
pub struct FakePrompt(Arc<AtomicUsize>);

impl FakePrompt {
	pub fn shown(&self) -> usize {
		self.0.load(Ordering::SeqCst)
	}
}

impl crate::first_use::LoginPrompt for FakePrompt {
	fn show(&self) {
		self.0.fetch_add(1, Ordering::SeqCst);
	}
}

/// Installs a [`FakePrompt`] for this test process.
pub fn installed_prompt() -> FakePrompt {
	let prompt = FakePrompt::default();
	crate::first_use::install_prompt(Box::new(prompt.clone()));
	prompt
}

/// A login code from Spotify.
pub fn device_code() -> DeviceCode {
	DeviceCode {
		user_code: "KKGT-2024".to_owned(),
		verification_uri: "https://spotify.com/pair".to_owned(),
		url: "https://spotify.com/pair?code=KKGT-2024".to_owned(),
	}
}

/// Plays the user: once `logins` logins have started, confirms the latest in the browser.
pub fn confirm_login_when_started(
	fake: &FakeBackend,
	logins: usize,
) -> std::thread::JoinHandle<()> {
	let fake = fake.clone();
	std::thread::spawn(move || {
		let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
		while fake.logins_started() < logins {
			assert!(std::time::Instant::now() < deadline, "no login started");
			std::thread::sleep(std::time::Duration::from_millis(5));
		}
		fake.send_code(device_code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
	})
}

/// Plays the user: once a login has started, cancels it, as Close in the login window does.
pub fn cancel_login_when_started(fake: &FakeBackend) -> std::thread::JoinHandle<()> {
	let fake = fake.clone();
	std::thread::spawn(move || {
		let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
		while fake.logins_started() == 0 {
			assert!(std::time::Instant::now() < deadline, "no login started");
			std::thread::sleep(std::time::Duration::from_millis(5));
		}
		crate::login::controller().cancel();
	})
}

/// Lets another thread press Stop on a `FakeAbort` the test keeps alive.
struct StopLater(*const foxide2k::testing::FakeAbort);

// SAFETY: every test joins the thread before its FakeAbort is dropped.
unsafe impl Send for StopLater {}

/// Presses Stop on `abort` after `delay`, from another thread. Join the handle before `abort`
/// goes out of scope.
pub fn stop_after(
	abort: &foxide2k::testing::FakeAbort,
	delay: std::time::Duration,
) -> std::thread::JoinHandle<()> {
	let target = StopLater(abort);
	std::thread::spawn(move || {
		let target = target;
		std::thread::sleep(delay);
		unsafe { &*target.0 }.set_aborting(true);
	})
}
