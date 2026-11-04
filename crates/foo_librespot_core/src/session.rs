//! Connecting to Spotify, rejecting accounts librespot can't play with, and keeping one live
//! session that is replaced when it drops.

use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use librespot_core::cache::Cache;
use librespot_core::error::ErrorKind;
use librespot_core::{Session, SessionConfig};

use crate::profile::{Profile, ProfileError};
use crate::{CoreError, Credentials};

/// How long to wait after connecting for Spotify to say which kind of account this is and which
/// country it is in.
pub const ACCOUNT_TYPE_TIMEOUT: Duration = Duration::from_secs(10);

const ATTRIBUTE_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// What [`SessionManager`] needs from a connected session.
pub trait SessionHandle: Clone + Send + Sync + 'static {
	/// True once the connection has dropped or was shut down; the session can't be reused.
	fn is_invalid(&self) -> bool;
	fn shutdown(&self);
}

impl SessionHandle for Session {
	fn is_invalid(&self) -> bool {
		Self::is_invalid(self)
	}

	fn shutdown(&self) {
		Self::shutdown(self);
	}
}

/// A session that has logged in, with the reusable login Spotify handed back.
pub struct Connected<S> {
	pub session: S,
	pub credentials: Credentials,
	pub username: String,
}

/// Makes a new logged-in session.
pub trait Connector: Send + Sync + 'static {
	type Session: SessionHandle;

	fn connect(
		&self,
		credentials: Credentials,
	) -> impl Future<Output = Result<Connected<Self::Session>, CoreError>> + Send;
}

/// How long a connect may take before the caller gets a timeout. librespot itself has no limit
/// on some of the steps, and the session lock is held meanwhile.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a failed connect answers for the callers after it. They are usually waiting for the
/// same connect (foobar2000 reads the info of many tracks at once), and without this each would
/// connect again in turn, up to [`CONNECT_TIMEOUT`] each.
const FAILED_CONNECT_HOLD: Duration = Duration::from_secs(5);

/// A connect that failed, and the logout count it was made under.
struct Failure {
	at: tokio::time::Instant,
	logouts: u64,
	error: CoreError,
}

/// Keeps one live session and replaces it when it drops, using the stored login.
pub struct SessionManager<C: Connector> {
	connector: C,
	profile: Arc<Profile>,
	connect_timeout: Duration,
	// Held for the whole connect, so callers that arrive together share one connection. The
	// session is kept with the logout count it was made under: a logout that can't take this lock
	// leaves it here, and the next call ends it.
	current: tokio::sync::Mutex<Option<(C::Session, u64)>>,
	// Bumped by every logout, so a connect that finishes afterwards is thrown away.
	logouts: AtomicU64,
	// Held while the logout count is read or bumped together with reading, saving or deleting the
	// stored login, so a logout can't fall between a connect's check and its save and have the
	// login written back.
	stored: std::sync::Mutex<()>,
	// Only used while `current` is held.
	failure: std::sync::Mutex<Option<Failure>>,
}

impl<C: Connector> SessionManager<C> {
	pub fn new(connector: C, profile: Arc<Profile>) -> Self {
		Self::with_connect_timeout(connector, profile, CONNECT_TIMEOUT)
	}

	pub fn with_connect_timeout(
		connector: C,
		profile: Arc<Profile>,
		connect_timeout: Duration,
	) -> Self {
		Self {
			connector,
			profile,
			connect_timeout,
			current: tokio::sync::Mutex::new(None),
			logouts: AtomicU64::new(0),
			stored: std::sync::Mutex::new(()),
			failure: std::sync::Mutex::new(None),
		}
	}

	fn lock_stored(&self) -> std::sync::MutexGuard<'_, ()> {
		self.stored.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	fn lock_failure(&self) -> std::sync::MutexGuard<'_, Option<Failure>> {
		self.failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	/// The error of a connect that failed less than [`FAILED_CONNECT_HOLD`] ago, with no logout
	/// since.
	fn recent_failure(&self, logouts: u64) -> Option<CoreError> {
		self.lock_failure()
			.as_ref()
			.filter(|failure| {
				failure.logouts == logouts && failure.at.elapsed() < FAILED_CONNECT_HOLD
			})
			.map(|failure| repeat(&failure.error))
	}

	/// A live session, connecting with the stored login when there is none.
	pub async fn session(&self) -> Result<C::Session, CoreError> {
		let mut current = self.current.lock().await;
		match current.take() {
			Some((session, made_under))
				if !session.is_invalid() && made_under == self.logouts.load(Ordering::SeqCst) =>
			{
				*current = Some((session.clone(), made_under));
				return Ok(session);
			}
			// Ended by a logout that couldn't take the lock.
			Some((session, _)) if !session.is_invalid() => session.shutdown(),
			_ => {}
		}
		let (logouts, loaded) = {
			let _stored = self.lock_stored();
			(self.logouts.load(Ordering::SeqCst), self.profile.load_credentials())
		};
		let credentials = match loaded {
			Ok(Some(credentials)) => credentials,
			Ok(None) => return Err(CoreError::NotLoggedIn),
			Err(ProfileError::Unreadable(reason)) => {
				// It will never become readable; remove it so this is reported only once.
				log::warn!("the stored Spotify login can't be read ({reason}); log in again");
				self.profile.clear_credentials()?;
				return Err(CoreError::NotLoggedIn);
			}
			Err(error) => return Err(error.into()),
		};
		if let Some(error) = self.recent_failure(logouts) {
			return Err(error);
		}
		match self.connect(credentials).await {
			Ok(connected) => {
				{
					let _stored = self.lock_stored();
					if self.logouts.load(Ordering::SeqCst) != logouts {
						connected.session.shutdown();
						return Err(CoreError::NotLoggedIn);
					}
					self.save(&connected.credentials);
				}
				*current = Some((connected.session.clone(), logouts));
				Ok(connected.session)
			}
			Err(CoreError::Spotify(error)) if rejects_credentials(&error) => {
				log::warn!("Spotify rejected the stored login: {error}");
				self.profile.clear_credentials()?;
				Err(CoreError::NotLoggedIn)
			}
			Err(error) => {
				*self.lock_failure() = Some(Failure {
					at: tokio::time::Instant::now(),
					logouts,
					error: repeat(&error),
				});
				Err(error)
			}
		}
	}

	/// Logs in with an OAuth access token, stores the reusable login Spotify returns and keeps
	/// the session. Returns the Spotify username.
	pub async fn login_with_token(&self, access_token: String) -> Result<String, CoreError> {
		let mut current = self.current.lock().await;
		let logouts = self.logouts.load(Ordering::SeqCst);
		let connected = self.connect(Credentials::with_access_token(access_token)).await?;
		{
			let _stored = self.lock_stored();
			if self.logouts.load(Ordering::SeqCst) != logouts {
				connected.session.shutdown();
				return Err(CoreError::NotLoggedIn);
			}
			self.profile.save_credentials(&connected.credentials)?;
		}
		*self.lock_failure() = None;
		if let Some((previous, _)) = current.replace((connected.session, logouts)) {
			previous.shutdown();
		}
		Ok(connected.username)
	}

	/// Forgets the stored login and ends the session. Never waits for a connect in progress,
	/// because it runs on foobar2000's UI thread; that connect is thrown away when it finishes,
	/// and a session another call is handing out ends at the next call. When the stored login
	/// can't be deleted, the session still ends and the error is returned.
	pub fn logout(&self) -> Result<(), CoreError> {
		let cleared = {
			let _stored = self.lock_stored();
			let cleared = self.profile.clear_credentials();
			self.logouts.fetch_add(1, Ordering::SeqCst);
			cleared
		};
		if let Ok(mut current) = self.current.try_lock()
			&& let Some((session, _)) = current.take()
		{
			session.shutdown();
		}
		Ok(cleared?)
	}

	/// True when a stored login exists and can be read.
	pub fn is_logged_in(&self) -> bool {
		matches!(self.profile.load_credentials(), Ok(Some(_)))
	}

	/// The Spotify username in the stored login, or `None` when logged out.
	pub fn username(&self) -> Option<String> {
		self.profile.load_credentials().ok().flatten().and_then(|credentials| credentials.username)
	}

	async fn connect(&self, credentials: Credentials) -> Result<Connected<C::Session>, CoreError> {
		tokio::time::timeout(self.connect_timeout, self.connector.connect(credentials))
			.await
			.map_err(|_| {
				CoreError::Io(io::Error::new(
					io::ErrorKind::TimedOut,
					format!(
						"Spotify didn't answer within {} seconds",
						self.connect_timeout.as_secs()
					),
				))
			})?
	}

	fn save(&self, credentials: &Credentials) {
		// Playing matters more than refreshing the stored login, which still works next time.
		if let Err(error) = self.profile.save_credentials(credentials) {
			log::warn!("couldn't store the refreshed Spotify login: {error}");
		}
	}
}

/// The same error again, for a caller that shares a failed connect. Errors from librespot and
/// std can't be cloned, so they keep their kind and message.
fn repeat(error: &CoreError) -> CoreError {
	match error {
		CoreError::NotPremium { account_type } => {
			CoreError::NotPremium { account_type: account_type.clone() }
		}
		CoreError::Spotify(error) => CoreError::Spotify(librespot_core::Error {
			kind: error.kind,
			error: error.error.to_string().into(),
		}),
		CoreError::Io(error) => CoreError::Io(io::Error::new(error.kind(), error.to_string())),
		other => CoreError::Unavailable(other.to_string()),
	}
}

/// What librespot says when Spotify refuses the login itself.
const BAD_CREDENTIALS: &str = "Login failed with reason: Bad credentials";

/// True when Spotify refused the stored login, so it will never work again. librespot reports
/// every access-point login failure as `PermissionDenied`, including temporary ones such as "try
/// another access point" after six tries, and keeps the error type that names the reason
/// private, so its message is the only way to tell them apart.
fn rejects_credentials(error: &librespot_core::Error) -> bool {
	error.kind == ErrorKind::PermissionDenied && error.error.to_string() == BAD_CREDENTIALS
}

/// librespot can only play with Premium accounts. An unknown type is let through: Spotify
/// sometimes doesn't send it, and playback then fails with its own error if it must.
pub fn check_account_type(account_type: Option<&str>) -> Result<(), CoreError> {
	match account_type {
		Some(kind) if kind != "premium" => {
			Err(CoreError::NotPremium { account_type: kind.to_owned() })
		}
		_ => Ok(()),
	}
}

/// Polls `get` until it returns a value or `timeout` passes. librespot stores user attributes
/// when a packet arrives after `connect` returns, and offers no way to wait for them.
pub async fn wait_for_attribute(
	get: impl Fn() -> Option<String> + Send,
	timeout: Duration,
) -> Option<String> {
	let deadline = Instant::now() + timeout;
	loop {
		if let Some(value) = get() {
			return Some(value);
		}
		if Instant::now() >= deadline {
			return None;
		}
		tokio::time::sleep(ATTRIBUTE_POLL_INTERVAL).await;
	}
}

/// Connects real librespot sessions.
pub struct LibrespotConnector {
	config: SessionConfig,
}

impl LibrespotConnector {
	pub fn new(device_id: String) -> Self {
		Self { config: SessionConfig { device_id, ..SessionConfig::default() } }
	}
}

impl Connector for LibrespotConnector {
	type Session = Session;

	async fn connect(&self, credentials: Credentials) -> Result<Connected<Session>, CoreError> {
		// Credentials stay in memory: foo_librespot stores them itself, encrypted.
		let cache = Cache::new(None::<PathBuf>, None, None, None)?.with_memory_credentials();
		let session = Session::new(self.config.clone(), Some(cache));
		session.connect(credentials, true).await?;
		// The account type and the country arrive in separate packets after the login; which
		// tracks are available depends on the country.
		let _ = wait_for_attribute(
			|| {
				(!session.country().is_empty())
					.then(|| session.get_user_attribute("type"))
					.flatten()
			},
			ACCOUNT_TYPE_TIMEOUT,
		)
		.await;
		let account_type = session.get_user_attribute("type");
		if account_type.is_none() {
			log::warn!("Spotify didn't say which kind of account this is; trying anyway");
		}
		if session.country().is_empty() {
			log::warn!(
				"Spotify didn't say which country the account is in; tracks aren't ruled out by country"
			);
		}
		if let Err(error) = check_account_type(account_type.as_deref()) {
			session.shutdown();
			return Err(error);
		}
		let credentials = session
			.cache()
			.and_then(|cache| cache.credentials())
			.ok_or_else(|| CoreError::Unavailable("Spotify sent no reusable login".to_owned()))?;
		let username = session.username();
		Ok(Connected { session, credentials, username })
	}
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
	use std::sync::{Arc, Mutex};
	use std::time::{Duration, Instant};

	use super::{
		Connected, Connector, SessionHandle, SessionManager, check_account_type, wait_for_attribute,
	};
	use crate::profile::Profile;
	use crate::{CoreError, Credentials};

	#[derive(Clone, Debug, Default)]
	struct FakeSession {
		invalid: Arc<AtomicBool>,
		shut_down: Arc<AtomicBool>,
	}

	impl SessionHandle for FakeSession {
		fn is_invalid(&self) -> bool {
			self.invalid.load(Ordering::SeqCst)
		}

		fn shutdown(&self) {
			self.shut_down.store(true, Ordering::SeqCst);
			self.invalid.store(true, Ordering::SeqCst);
		}
	}

	enum Outcome {
		Connect { username: &'static str, credentials: Credentials },
		Fail(fn() -> CoreError),
	}

	struct FakeState {
		calls: AtomicUsize,
		received: Mutex<Vec<Credentials>>,
		sessions: Mutex<Vec<FakeSession>>,
		outcome: Mutex<Outcome>,
		delay: Duration,
	}

	#[derive(Clone)]
	struct FakeConnector(Arc<FakeState>);

	impl FakeConnector {
		fn new(outcome: Outcome) -> Self {
			Self::with_delay(outcome, Duration::ZERO)
		}

		fn with_delay(outcome: Outcome, delay: Duration) -> Self {
			Self(Arc::new(FakeState {
				calls: AtomicUsize::new(0),
				received: Mutex::new(Vec::new()),
				sessions: Mutex::new(Vec::new()),
				outcome: Mutex::new(outcome),
				delay,
			}))
		}

		fn calls(&self) -> usize {
			self.0.calls.load(Ordering::SeqCst)
		}

		fn last_session(&self) -> FakeSession {
			self.0.sessions.lock().unwrap().last().unwrap().clone()
		}
	}

	impl Connector for FakeConnector {
		type Session = FakeSession;

		async fn connect(
			&self,
			credentials: Credentials,
		) -> Result<Connected<FakeSession>, CoreError> {
			let state = Arc::clone(&self.0);
			state.calls.fetch_add(1, Ordering::SeqCst);
			state.received.lock().unwrap().push(credentials);
			tokio::time::sleep(state.delay).await;
			let result = match &*state.outcome.lock().unwrap() {
				Outcome::Connect { username, credentials } => Ok((*username, credentials.clone())),
				Outcome::Fail(error) => Err(error()),
			};
			let (username, credentials) = result?;
			let session = FakeSession::default();
			state.sessions.lock().unwrap().push(session.clone());
			Ok(Connected { session, credentials, username: username.to_owned() })
		}
	}

	fn stored() -> Credentials {
		Credentials::with_password("alice", "stored")
	}

	fn refreshed() -> Credentials {
		Credentials::with_password("alice", "new")
	}

	fn connects() -> Outcome {
		Outcome::Connect { username: "alice", credentials: refreshed() }
	}

	struct Setup {
		_dir: tempfile::TempDir,
		profile: Arc<Profile>,
		connector: FakeConnector,
		manager: SessionManager<FakeConnector>,
	}

	fn setup(outcome: Outcome, logged_in: bool) -> Setup {
		setup_with(FakeConnector::new(outcome), logged_in)
	}

	fn setup_with(connector: FakeConnector, logged_in: bool) -> Setup {
		let dir = tempfile::tempdir().unwrap();
		let profile = Arc::new(Profile::new(dir.path()));
		if logged_in {
			profile.save_credentials(&stored()).unwrap();
		}
		let manager = SessionManager::new(connector.clone(), Arc::clone(&profile));
		Setup { _dir: dir, profile, connector, manager }
	}

	#[test]
	fn username_comes_from_the_stored_login() {
		assert_eq!(setup(connects(), true).manager.username().as_deref(), Some("alice"));
		assert_eq!(setup(connects(), false).manager.username(), None);
	}

	#[tokio::test]
	async fn session_without_stored_login_is_not_logged_in() {
		let setup = setup(connects(), false);
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		assert_eq!(setup.connector.calls(), 0);
	}

	#[tokio::test]
	async fn session_connects_with_stored_credentials_once_and_reuses_it() {
		let setup = setup(connects(), true);
		setup.manager.session().await.unwrap();
		setup.manager.session().await.unwrap();
		assert_eq!(setup.connector.calls(), 1);
		assert_eq!(*setup.connector.0.received.lock().unwrap(), vec![stored()]);
	}

	#[tokio::test]
	async fn invalid_session_is_replaced() {
		let setup = setup(connects(), true);
		setup.manager.session().await.unwrap();
		setup.connector.last_session().invalid.store(true, Ordering::SeqCst);
		setup.manager.session().await.unwrap();
		assert_eq!(setup.connector.calls(), 2);
	}

	#[tokio::test]
	async fn refreshed_credentials_are_saved_after_connect() {
		let setup = setup(connects(), true);
		setup.manager.session().await.unwrap();
		assert_eq!(setup.profile.load_credentials().unwrap(), Some(refreshed()));
	}

	// librespot's AuthenticationError is private; these are the messages it gives for the codes.
	fn login_failed(reason: &'static str) -> CoreError {
		CoreError::Spotify(librespot_core::Error::permission_denied(format!(
			"Login failed with reason: {reason}"
		)))
	}

	#[tokio::test]
	async fn rejected_stored_credentials_are_cleared() {
		let setup = setup(Outcome::Fail(|| login_failed("Bad credentials")), true);
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		assert!(!setup.profile.has_credentials());
	}

	#[tokio::test]
	async fn other_login_failures_keep_stored_credentials() {
		for reason in [
			|| login_failed("Try another access point"),
			|| login_failed("Protocol error"),
			|| login_failed("Travel restriction"),
			|| login_failed("Could not validate credentials"),
			|| login_failed("Premium account required"),
		] {
			let setup = setup(Outcome::Fail(reason), true);
			let result = setup.manager.session().await;
			assert!(matches!(result, Err(CoreError::Spotify(_))), "{result:?}");
			assert!(setup.profile.has_credentials(), "{result:?}");
		}
	}

	#[tokio::test]
	async fn network_error_keeps_stored_credentials() {
		let setup = setup(
			Outcome::Fail(|| CoreError::Spotify(librespot_core::Error::unavailable("down"))),
			true,
		);
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::Spotify(_))), "{result:?}");
		assert!(setup.profile.has_credentials());
	}

	#[tokio::test]
	async fn unreadable_stored_login_counts_as_not_logged_in() {
		let setup = setup(connects(), false);
		std::fs::write(setup.profile.dir().join("credentials.bin"), b"garbage").unwrap();
		assert!(!setup.manager.is_logged_in());
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		assert_eq!(setup.connector.calls(), 0);
		// Removed, so it is reported once instead of on every call.
		assert!(!setup.profile.has_credentials());
	}

	#[tokio::test]
	async fn logout_does_not_wait_for_a_connect_in_progress() {
		let setup = setup_with(FakeConnector::with_delay(connects(), Duration::from_secs(1)), true);
		let logout = async {
			tokio::time::sleep(Duration::from_millis(50)).await;
			let started = Instant::now();
			setup.manager.logout().unwrap();
			started.elapsed()
		};
		let (connected, waited) = tokio::join!(setup.manager.session(), logout);
		assert!(waited < Duration::from_millis(500), "logout waited {waited:?}");
		// The connect that finished after the logout is thrown away, not stored again.
		assert!(matches!(connected, Err(CoreError::NotLoggedIn)), "{connected:?}");
		assert!(!setup.profile.has_credentials());
		assert!(setup.connector.last_session().shut_down.load(Ordering::SeqCst));
	}

	#[tokio::test]
	async fn connect_that_never_finishes_times_out() {
		let connector = FakeConnector::with_delay(connects(), Duration::from_secs(3600));
		let dir = tempfile::tempdir().unwrap();
		let profile = Arc::new(Profile::new(dir.path()));
		profile.save_credentials(&stored()).unwrap();
		let manager = SessionManager::with_connect_timeout(
			connector,
			Arc::clone(&profile),
			Duration::from_millis(100),
		);
		let result = manager.session().await;
		assert!(
			matches!(&result, Err(CoreError::Io(error)) if error.kind() == std::io::ErrorKind::TimedOut),
			"{result:?}"
		);
		assert!(profile.has_credentials());
	}

	#[tokio::test]
	async fn concurrent_callers_share_one_connect() {
		let setup =
			setup_with(FakeConnector::with_delay(connects(), Duration::from_millis(50)), true);
		let manager = &setup.manager;
		let results = tokio::join!(
			manager.session(),
			manager.session(),
			manager.session(),
			manager.session(),
			manager.session()
		);
		for result in [results.0, results.1, results.2, results.3, results.4] {
			result.unwrap();
		}
		assert_eq!(setup.connector.calls(), 1);
	}

	#[tokio::test]
	async fn login_with_token_saves_reusable_credentials_and_returns_username() {
		let setup = setup(connects(), false);
		let username = setup.manager.login_with_token("tok".to_owned()).await.unwrap();
		assert_eq!(username, "alice");
		assert_eq!(
			*setup.connector.0.received.lock().unwrap(),
			vec![Credentials::with_access_token("tok")]
		);
		assert_eq!(setup.profile.load_credentials().unwrap(), Some(refreshed()));
		setup.manager.session().await.unwrap();
		assert_eq!(setup.connector.calls(), 1);
	}

	#[tokio::test]
	async fn non_premium_login_saves_nothing() {
		let setup = setup(
			Outcome::Fail(|| CoreError::NotPremium { account_type: "free".to_owned() }),
			false,
		);
		let result = setup.manager.login_with_token("tok".to_owned()).await;
		assert!(
			matches!(&result, Err(CoreError::NotPremium { account_type }) if account_type == "free"),
			"{result:?}"
		);
		assert!(!setup.profile.has_credentials());
	}

	#[tokio::test]
	async fn logout_clears_credentials_and_shuts_down_the_session() {
		let setup = setup(connects(), true);
		setup.manager.session().await.unwrap();
		setup.manager.logout().unwrap();
		assert!(!setup.profile.has_credentials());
		assert!(!setup.manager.is_logged_in());
		assert!(setup.connector.last_session().shut_down.load(Ordering::SeqCst));
	}

	fn unavailable() -> CoreError {
		CoreError::Spotify(librespot_core::Error::unavailable("down"))
	}

	#[tokio::test]
	async fn callers_waiting_together_share_a_failed_connect() {
		let connector =
			FakeConnector::with_delay(Outcome::Fail(unavailable), Duration::from_millis(50));
		let setup = setup_with(connector, true);
		let (first, second) = tokio::join!(setup.manager.session(), setup.manager.session());
		assert!(matches!(first, Err(CoreError::Spotify(_))), "{first:?}");
		assert!(
			matches!(&second, Err(error) if error.to_string() == unavailable().to_string()),
			"{second:?}"
		);
		assert_eq!(setup.connector.calls(), 1);
	}

	#[tokio::test(start_paused = true)]
	async fn a_failed_connect_is_tried_again_after_a_while() {
		let setup = setup(Outcome::Fail(unavailable), true);
		let _ = setup.manager.session().await;
		tokio::time::advance(Duration::from_secs(6)).await;
		let _ = setup.manager.session().await;
		assert_eq!(setup.connector.calls(), 2);
	}

	#[tokio::test]
	async fn a_logout_after_a_failed_connect_reports_not_logged_in() {
		let setup = setup(Outcome::Fail(unavailable), true);
		let _ = setup.manager.session().await;
		setup.manager.logout().unwrap();
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
	}

	#[tokio::test]
	async fn logout_while_a_caller_holds_the_session_still_ends_it() {
		let setup = setup(connects(), true);
		setup.manager.session().await.unwrap();
		{
			// A playback thread taking the live session at the moment the user logs out.
			let _held = setup.manager.current.lock().await;
			setup.manager.logout().unwrap();
		}
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		assert!(setup.connector.last_session().shut_down.load(Ordering::SeqCst));
		assert_eq!(setup.connector.calls(), 1);
	}

	#[tokio::test]
	async fn logout_ends_the_session_even_when_the_login_file_cant_be_deleted() {
		use std::os::windows::fs::OpenOptionsExt;

		let setup = setup(connects(), true);
		setup.manager.session().await.unwrap();
		// Another process holding the file open without sharing, as antivirus sometimes does.
		let _locked = std::fs::OpenOptions::new()
			.read(true)
			.share_mode(0)
			.open(setup.profile.dir().join("credentials.bin"))
			.unwrap();
		assert!(setup.manager.logout().is_err());
		assert!(setup.connector.last_session().shut_down.load(Ordering::SeqCst));
	}

	#[test]
	fn check_account_type_accepts_premium_and_unknown() {
		assert!(check_account_type(Some("premium")).is_ok());
		assert!(check_account_type(None).is_ok());
	}

	#[test]
	fn check_account_type_rejects_free_and_open() {
		for kind in ["free", "open"] {
			let result = check_account_type(Some(kind));
			assert!(
				matches!(&result, Err(CoreError::NotPremium { account_type }) if account_type == kind),
				"{result:?}"
			);
		}
	}

	#[tokio::test]
	async fn wait_for_attribute_returns_once_set() {
		let calls = AtomicUsize::new(0);
		let value = wait_for_attribute(
			|| (calls.fetch_add(1, Ordering::SeqCst) >= 2).then(|| "premium".to_owned()),
			Duration::from_secs(5),
		)
		.await;
		assert_eq!(value.as_deref(), Some("premium"));
		assert_eq!(calls.load(Ordering::SeqCst), 3);
	}

	#[tokio::test]
	async fn wait_for_attribute_gives_up_after_timeout() {
		let started = Instant::now();
		let value = wait_for_attribute(|| None, Duration::from_millis(200)).await;
		assert_eq!(value, None);
		assert!(started.elapsed() >= Duration::from_millis(200));
		assert!(started.elapsed() < Duration::from_secs(2));
	}
}
