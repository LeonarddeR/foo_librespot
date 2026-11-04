//! The Spotify login, driven from one controller per process. The preferences page and the
//! login window are views of it: a login keeps running when they close, and a view that opens
//! shows where it is.
//!
//! The browser never opens by itself: only [`LoginController::open_browser`], which the "Open
//! browser" button calls, opens Spotify's page.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use foo_librespot_core::CoreError;
use foo_librespot_core::login::{DeviceCode, LoginError};

use crate::backend::{LoginTask, backend};

/// How often a waiting import checks whether foobar2000 wants it to stop.
#[cfg_attr(not(test), expect(dead_code, reason = "logging in on first use (Task 9) waits"))]
const WAIT_POLL: Duration = Duration::from_millis(50);

/// Where the login is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginState {
	LoggedOut,
	/// Asking Spotify for a code.
	Asking,
	/// Waiting for the user to confirm `DeviceCode` in a browser.
	Confirming(DeviceCode),
	/// The Spotify username, when the stored login has one.
	LoggedIn(Option<String>),
	/// Why the last login didn't work.
	Failed(String),
}

impl LoginState {
	const fn is_running(&self) -> bool {
		matches!(self, Self::Asking | Self::Confirming(_))
	}
}

/// What the account controls show for a state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginView {
	pub button: &'static str,
	pub status: String,
	pub code: String,
	pub browser_enabled: bool,
}

const NO_LOGIN: &str = "No login in progress.";

pub fn view(state: &LoginState) -> LoginView {
	let (button, status, code) = match state {
		LoginState::LoggedOut => {
			("&Log in", "Not logged in to Spotify.".to_owned(), NO_LOGIN.to_owned())
		}
		LoginState::Asking => (
			"&Cancel login",
			"Asking Spotify for a login code…".to_owned(),
			"Waiting for Spotify's code…".to_owned(),
		),
		LoginState::Confirming(code) => (
			"&Cancel login",
			"Press Open browser, or open the page below yourself, and confirm the code there."
				.to_owned(),
			format!("Code: {}\r\nPage: {}", code.user_code, code.url),
		),
		LoginState::LoggedIn(Some(name)) => {
			("Log &out", format!("Logged in to Spotify as {name}."), NO_LOGIN.to_owned())
		}
		LoginState::LoggedIn(None) => {
			("Log &out", "Logged in to Spotify.".to_owned(), NO_LOGIN.to_owned())
		}
		LoginState::Failed(message) => {
			("&Log in", format!("Login failed: {message}."), NO_LOGIN.to_owned())
		}
	};
	LoginView { button, status, code, browser_enabled: matches!(state, LoginState::Confirming(_)) }
}

type Notify = Arc<dyn Fn() + Send + Sync>;

struct Inner {
	state: LoginState,
	task: Option<Box<dyn LoginTask>>,
	/// Which login the backend's answers belong to; a cancelled or replaced login's answers
	/// carry an older number and are ignored.
	generation: u64,
	subscribers: Vec<(u64, Notify)>,
	next_subscriber: u64,
}

struct Shared {
	inner: Mutex<Inner>,
	/// Signalled whenever a login ends, for [`LoginController::wait_for_login`].
	ended: Condvar,
	open_url: Box<dyn Fn(&str) + Send + Sync>,
}

impl Shared {
	fn lock(&self) -> MutexGuard<'_, Inner> {
		self.inner.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Sets the state, then tells every subscriber, outside the lock so they may read it.
	fn set(&self, mut inner: MutexGuard<'_, Inner>, state: LoginState) {
		let ended = inner.state.is_running() && !state.is_running();
		inner.state = state;
		let subscribers: Vec<Notify> =
			inner.subscribers.iter().map(|(_, notify)| Arc::clone(notify)).collect();
		drop(inner);
		if ended {
			self.ended.notify_all();
		}
		for notify in subscribers {
			notify();
		}
	}
}

/// The one Spotify login of the process.
pub struct LoginController(Arc<Shared>);

impl LoginController {
	/// `open_url` opens a page in the browser; only [`Self::open_browser`] calls it.
	pub fn new(open_url: Box<dyn Fn(&str) + Send + Sync>) -> Self {
		Self(Arc::new(Shared {
			inner: Mutex::new(Inner {
				state: LoginState::LoggedOut,
				task: None,
				generation: 0,
				subscribers: Vec::new(),
				next_subscriber: 0,
			}),
			ended: Condvar::new(),
			open_url,
		}))
	}

	pub fn state(&self) -> LoginState {
		self.0.lock().state.clone()
	}

	/// Reads the stored login, unless a login is running. A failure stays shown until a login
	/// is stored.
	pub fn refresh(&self) {
		let username = backend().username();
		let inner = self.0.lock();
		let state = match (&inner.state, username) {
			(running, _) if running.is_running() => return,
			(_, Some(name)) => LoginState::LoggedIn(Some(name)),
			(LoginState::Failed(_), None) => return,
			(_, None) => LoginState::LoggedOut,
		};
		if inner.state != state {
			self.0.set(inner, state);
		}
	}

	/// What the account button does: log in, cancel the login, or log out.
	pub fn press(&self) {
		let state = self.state();
		match state {
			LoginState::LoggedOut | LoginState::Failed(_) => self.begin(),
			LoginState::Asking | LoginState::Confirming(_) => self.cancel(),
			LoginState::LoggedIn(_) => self.logout(),
		}
	}

	/// Starts a login, but only when logged out and none is running.
	#[cfg_attr(not(test), expect(dead_code, reason = "logging in on first use (Task 9) starts it"))]
	pub fn start(&self) {
		self.refresh();
		if matches!(self.state(), LoginState::LoggedOut | LoginState::Failed(_)) {
			self.begin();
		}
	}

	fn begin(&self) {
		let generation = {
			let mut inner = self.0.lock();
			if inner.state.is_running() {
				return;
			}
			inner.generation += 1;
			let generation = inner.generation;
			self.0.set(inner, LoginState::Asking);
			generation
		};
		let on_code = {
			let shared = Arc::clone(&self.0);
			Box::new(move |code: DeviceCode| {
				let inner = shared.lock();
				if inner.generation == generation && inner.state == LoginState::Asking {
					shared.set(inner, LoginState::Confirming(code));
				}
			})
		};
		let on_done = {
			let shared = Arc::clone(&self.0);
			Box::new(move |result: Result<String, CoreError>| {
				let mut inner = shared.lock();
				if inner.generation == generation && inner.state.is_running() {
					inner.task = None;
					let state = match result {
						Ok(name) => LoginState::LoggedIn(Some(name)),
						Err(error) => LoginState::Failed(error.to_string()),
					};
					shared.set(inner, state);
				}
			})
		};
		// Called without the lock: a backend may answer on this thread before it returns.
		let started = backend().start_login(on_code, on_done);
		let mut inner = self.0.lock();
		match started {
			Ok(task) if inner.generation == generation && inner.state.is_running() => {
				inner.task = Some(task);
			}
			// Cancelled, or already finished, while starting.
			Ok(task) => {
				drop(inner);
				task.cancel();
			}
			Err(error) if inner.generation == generation => {
				self.0.set(inner, LoginState::Failed(error.to_string()));
			}
			Err(_) => {}
		}
	}

	/// Cancels a running login; otherwise does nothing.
	pub fn cancel(&self) {
		let mut inner = self.0.lock();
		if !inner.state.is_running() {
			return;
		}
		inner.generation += 1;
		let task = inner.task.take();
		self.0.set(inner, LoginState::LoggedOut);
		if let Some(task) = task {
			task.cancel();
		}
	}

	fn logout(&self) {
		let state = match backend().logout() {
			Ok(()) => LoginState::LoggedOut,
			Err(error) => LoginState::Failed(error.to_string()),
		};
		let inner = self.0.lock();
		self.0.set(inner, state);
	}

	/// Opens Spotify's page for the code being confirmed. Does nothing in any other state.
	pub fn open_browser(&self) {
		let LoginState::Confirming(code) = self.state() else { return };
		(self.0.open_url)(&code.url);
	}

	#[cfg_attr(not(test), expect(dead_code, reason = "logging in on first use (Task 9) waits"))]
	/// Starts a login if none is running, then waits for it to end. Returns the username,
	/// [`CoreError::NotLoggedIn`] when the user cancelled it, and [`CoreError::Aborted`] when
	/// `is_aborting` turned true. An aborted wait leaves the login running: the user may still
	/// confirm it.
	pub fn wait_for_login(&self, is_aborting: &dyn Fn() -> bool) -> Result<String, CoreError> {
		self.start();
		let mut inner = self.0.lock();
		loop {
			match &inner.state {
				LoginState::LoggedIn(name) => return Ok(name.clone().unwrap_or_default()),
				LoginState::Failed(message) => {
					return Err(CoreError::Login(LoginError::Failed(message.clone())));
				}
				LoginState::LoggedOut => return Err(CoreError::NotLoggedIn),
				LoginState::Asking | LoginState::Confirming(_) => {}
			}
			if is_aborting() {
				return Err(CoreError::Aborted);
			}
			inner = self
				.0
				.ended
				.wait_timeout(inner, WAIT_POLL)
				.unwrap_or_else(PoisonError::into_inner)
				.0;
		}
	}

	/// Calls `notify` after every change, on whichever thread made it. Returns the id to
	/// [`Self::unsubscribe`] with.
	pub fn subscribe(&self, notify: Box<dyn Fn() + Send + Sync>) -> u64 {
		let mut inner = self.0.lock();
		let id = inner.next_subscriber;
		inner.next_subscriber += 1;
		inner.subscribers.push((id, Arc::from(notify)));
		id
	}

	pub fn unsubscribe(&self, id: u64) {
		self.0.lock().subscribers.retain(|(subscriber, _)| *subscriber != id);
	}
}

static CONTROLLER: OnceLock<LoginController> = OnceLock::new();

/// The process's login, which opens pages with the user's browser.
pub fn controller() -> &'static LoginController {
	CONTROLLER.get_or_init(|| {
		LoginController::new(Box::new(|url: &str| {
			if let Err(error) = oxibar::shell::open_url(url) {
				oxibar::console::print(&format!("foo_librespot: {}", error.message));
			}
		}))
	})
}

/// Makes `controller()` return `controller` in this test process.
#[cfg(test)]
pub fn install_controller(controller: LoginController) {
	assert!(CONTROLLER.set(controller).is_ok(), "a controller was already in use");
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
	use std::sync::{Arc, Mutex, PoisonError};
	use std::time::{Duration, Instant};

	use foo_librespot_core::CoreError;
	use foo_librespot_core::login::{DeviceCode, LoginError};

	use super::{LoginController, LoginState, view};
	use crate::backend;
	use crate::testing::FakeBackend;

	fn installed() -> FakeBackend {
		let fake = FakeBackend::default();
		backend::install(Box::new(fake.clone()));
		fake
	}

	/// A controller whose browser only records the pages it was asked to open.
	fn controller() -> (Arc<LoginController>, Arc<Mutex<Vec<String>>>) {
		let opened = Arc::new(Mutex::new(Vec::new()));
		let record = Arc::clone(&opened);
		let controller = LoginController::new(Box::new(move |url: &str| {
			record.lock().unwrap_or_else(PoisonError::into_inner).push(url.to_owned());
		}));
		(Arc::new(controller), opened)
	}

	fn code() -> DeviceCode {
		DeviceCode {
			user_code: "KKGT-2024".to_owned(),
			verification_uri: "https://spotify.com/pair".to_owned(),
			url: "https://spotify.com/pair?code=KKGT-2024".to_owned(),
		}
	}

	fn opened(record: &Mutex<Vec<String>>) -> Vec<String> {
		record.lock().unwrap_or_else(PoisonError::into_inner).clone()
	}

	/// Waits up to a second for `condition`, for tests that start a login on another thread.
	fn eventually(condition: impl Fn() -> bool) {
		let deadline = Instant::now() + Duration::from_secs(1);
		while !condition() {
			assert!(Instant::now() < deadline, "timed out");
			std::thread::sleep(Duration::from_millis(5));
		}
	}

	#[test]
	fn view_strings_for_every_state() {
		let logged_out = view(&LoginState::LoggedOut);
		assert_eq!(logged_out.button, "&Log in");
		assert_eq!(logged_out.status, "Not logged in to Spotify.");
		assert_eq!(logged_out.code, "No login in progress.");
		assert!(!logged_out.browser_enabled);

		let asking = view(&LoginState::Asking);
		assert_eq!(asking.button, "&Cancel login");
		assert_eq!(asking.status, "Asking Spotify for a login code…");
		assert_eq!(asking.code, "Waiting for Spotify's code…");
		assert!(!asking.browser_enabled);

		let confirming = view(&LoginState::Confirming(code()));
		assert_eq!(confirming.button, "&Cancel login");
		assert_eq!(
			confirming.status,
			"Press Open browser, or open the page below yourself, and confirm the code there."
		);
		assert_eq!(
			confirming.code,
			"Code: KKGT-2024\r\nPage: https://spotify.com/pair?code=KKGT-2024"
		);
		assert!(confirming.browser_enabled);

		let named = view(&LoginState::LoggedIn(Some("Keith Getty".to_owned())));
		assert_eq!(named.button, "Log &out");
		assert_eq!(named.status, "Logged in to Spotify as Keith Getty.");
		assert_eq!(named.code, "No login in progress.");
		assert_eq!(view(&LoginState::LoggedIn(None)).status, "Logged in to Spotify.");

		let failed = view(&LoginState::Failed("the login was declined in the browser".to_owned()));
		assert_eq!(failed.button, "&Log in");
		assert_eq!(failed.status, "Login failed: the login was declined in the browser.");
		assert!(!failed.browser_enabled);
	}

	#[test]
	fn press_when_logged_out_starts_and_asks() {
		let fake = installed();
		let (controller, _opened) = controller();
		controller.press();
		assert_eq!(fake.logins_started(), 1);
		assert_eq!(controller.state(), LoginState::Asking);
	}

	#[test]
	fn code_arrival_shows_the_code_but_opens_nothing() {
		let fake = installed();
		let (controller, opened_pages) = controller();
		controller.press();
		fake.send_code(code());
		assert_eq!(controller.state(), LoginState::Confirming(code()));
		assert!(view(&controller.state()).browser_enabled);
		assert!(opened(&opened_pages).is_empty(), "the browser opened by itself");
	}

	#[test]
	fn open_browser_opens_the_page_only_while_confirming() {
		let fake = installed();
		let (controller, opened_pages) = controller();
		controller.open_browser();
		controller.press();
		controller.open_browser();
		assert!(opened(&opened_pages).is_empty());
		fake.send_code(code());
		controller.open_browser();
		assert_eq!(opened(&opened_pages), ["https://spotify.com/pair?code=KKGT-2024"]);
	}

	#[test]
	fn success_shows_the_username() {
		let fake = installed();
		let (controller, _opened) = controller();
		controller.press();
		fake.send_code(code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
		assert_eq!(controller.state(), LoginState::LoggedIn(Some("Keith Getty".to_owned())));
		assert_eq!(view(&controller.state()).status, "Logged in to Spotify as Keith Getty.");
	}

	#[test]
	fn failed_login_explains_and_offers_log_in_again() {
		let fake = installed();
		let (controller, _opened) = controller();
		controller.press();
		fake.finish_login(Err(CoreError::NotPremium { account_type: "free".to_owned() }));
		let shown = view(&controller.state());
		assert_eq!(
			shown.status,
			"Login failed: this Spotify account is free, but playing needs Spotify Premium."
		);
		assert_eq!(shown.button, "&Log in");
		controller.press();
		assert_eq!(fake.logins_started(), 2);
	}

	#[test]
	fn press_while_waiting_cancels() {
		let fake = installed();
		let (controller, _opened) = controller();
		controller.press();
		controller.press();
		assert!(fake.last_login_cancelled());
		assert_eq!(controller.state(), LoginState::LoggedOut);
	}

	#[test]
	fn late_answers_from_a_cancelled_login_are_ignored() {
		let fake = installed();
		let (controller, _opened) = controller();
		controller.press();
		controller.cancel();
		fake.send_code(code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
		assert_eq!(controller.state(), LoginState::LoggedOut);
	}

	#[test]
	fn press_when_logged_in_logs_out() {
		let fake = installed();
		fake.set_username(Some("Keith Getty"));
		let (controller, _opened) = controller();
		controller.refresh();
		controller.press();
		assert_eq!(fake.logouts(), 1);
		assert_eq!(controller.state(), LoginState::LoggedOut);
	}

	#[test]
	fn refresh_reads_the_stored_username() {
		let fake = installed();
		let (controller, _opened) = controller();
		controller.refresh();
		assert_eq!(controller.state(), LoginState::LoggedOut);
		fake.set_username(Some("Kristyn Getty"));
		controller.refresh();
		assert_eq!(controller.state(), LoginState::LoggedIn(Some("Kristyn Getty".to_owned())));
	}

	#[test]
	fn start_does_nothing_while_a_login_runs_or_when_logged_in() {
		let fake = installed();
		let (controller, _opened) = controller();
		controller.start();
		controller.start();
		assert_eq!(fake.logins_started(), 1);
		fake.finish_login(Ok("Keith Getty".to_owned()));
		controller.start();
		assert_eq!(fake.logins_started(), 1);
		assert!(matches!(controller.state(), LoginState::LoggedIn(_)));
	}

	#[test]
	fn wait_for_login_starts_one_login_and_returns_the_username() {
		let fake = installed();
		let (controller, _opened) = controller();
		let waiter = {
			let controller = Arc::clone(&controller);
			std::thread::spawn(move || controller.wait_for_login(&|| false))
		};
		eventually(|| fake.logins_started() == 1);
		fake.send_code(code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
		assert_eq!(waiter.join().unwrap().unwrap(), "Keith Getty");
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn two_waiters_share_one_login() {
		let fake = installed();
		let (controller, _opened) = controller();
		let waiters: Vec<_> = (0..2)
			.map(|_| {
				let controller = Arc::clone(&controller);
				std::thread::spawn(move || controller.wait_for_login(&|| false))
			})
			.collect();
		eventually(|| fake.logins_started() == 1);
		std::thread::sleep(Duration::from_millis(50));
		fake.finish_login(Ok("Keith Getty".to_owned()));
		for waiter in waiters {
			assert_eq!(waiter.join().unwrap().unwrap(), "Keith Getty");
		}
		assert_eq!(fake.logins_started(), 1);
	}

	#[test]
	fn wait_for_login_stops_when_aborted_and_leaves_the_login_running() {
		let fake = installed();
		let (controller, _opened) = controller();
		let aborting = Arc::new(AtomicBool::new(false));
		let waiter = {
			let controller = Arc::clone(&controller);
			let aborting = Arc::clone(&aborting);
			std::thread::spawn(move || {
				controller.wait_for_login(&|| aborting.load(Ordering::SeqCst))
			})
		};
		eventually(|| fake.logins_started() == 1);
		let started = Instant::now();
		aborting.store(true, Ordering::SeqCst);
		let result = waiter.join().unwrap();
		assert!(matches!(result, Err(CoreError::Aborted)), "{result:?}");
		assert!(started.elapsed() < Duration::from_secs(1));
		assert_eq!(controller.state(), LoginState::Asking);
		assert!(!fake.last_login_cancelled());
	}

	#[test]
	fn wait_for_login_reports_a_failed_or_cancelled_login() {
		let fake = installed();
		let (controller, _opened) = controller();
		let wait = || {
			let controller = Arc::clone(&controller);
			std::thread::spawn(move || controller.wait_for_login(&|| false))
		};
		let waiter = wait();
		eventually(|| fake.logins_started() == 1);
		fake.finish_login(Err(CoreError::Login(LoginError::Denied)));
		let result = waiter.join().unwrap();
		assert!(
			matches!(&result, Err(CoreError::Login(LoginError::Failed(message))) if message.contains("declined")),
			"{result:?}"
		);
		let waiter = wait();
		eventually(|| fake.logins_started() == 2);
		controller.cancel();
		let result = waiter.join().unwrap();
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
	}

	fn counting(controller: &LoginController) -> (u64, Arc<AtomicUsize>) {
		let count = Arc::new(AtomicUsize::new(0));
		let counted = Arc::clone(&count);
		let id = controller.subscribe(Box::new(move || {
			counted.fetch_add(1, Ordering::SeqCst);
		}));
		(id, count)
	}

	#[test]
	fn subscriber_hears_every_change() {
		let fake = installed();
		let (controller, _opened) = controller();
		let (_id, count) = counting(&controller);
		controller.press();
		fake.send_code(code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
		assert_eq!(count.load(Ordering::SeqCst), 3);
	}

	#[test]
	fn two_subscribers_both_hear_and_leave_separately() {
		let _fake = installed();
		let (controller, _opened) = controller();
		let (first, first_count) = counting(&controller);
		let (_second, second_count) = counting(&controller);
		controller.press();
		controller.unsubscribe(first);
		controller.press();
		assert_eq!(first_count.load(Ordering::SeqCst), 1);
		assert_eq!(second_count.load(Ordering::SeqCst), 2);
	}

	#[test]
	fn login_continues_without_a_page() {
		let fake = installed();
		let (controller, _opened) = controller();
		let (id, _count) = counting(&controller);
		controller.press();
		controller.unsubscribe(id);
		fake.send_code(code());
		fake.finish_login(Ok("Keith Getty".to_owned()));
		assert_eq!(controller.state(), LoginState::LoggedIn(Some("Keith Getty".to_owned())));
	}

	#[test]
	fn refresh_after_unsubscribe_reaches_nobody() {
		let fake = installed();
		let (controller, _opened) = controller();
		let (id, count) = counting(&controller);
		controller.unsubscribe(id);
		fake.set_username(Some("Keith Getty"));
		controller.refresh();
		controller.press();
		assert_eq!(count.load(Ordering::SeqCst), 0);
	}
}
