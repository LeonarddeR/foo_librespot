//! Connecting to Spotify, rejecting accounts librespot can't play with, and keeping one live
//! session that is replaced when it drops.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use librespot_core::cache::Cache;
use librespot_core::error::ErrorKind;
use librespot_core::{Session, SessionConfig};

use crate::profile::Profile;
use crate::{CoreError, Credentials};

/// How long to wait after connecting for Spotify to say which kind of account this is.
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

/// Keeps one live session and replaces it when it drops, using the stored login.
pub struct SessionManager<C: Connector> {
	connector: C,
	profile: Arc<Profile>,
	// Held for the whole connect, so callers that arrive together share one connection.
	current: tokio::sync::Mutex<Option<C::Session>>,
}

impl<C: Connector> SessionManager<C> {
	pub fn new(connector: C, profile: Arc<Profile>) -> Self {
		Self { connector, profile, current: tokio::sync::Mutex::new(None) }
	}

	/// A live session, connecting with the stored login when there is none.
	pub async fn session(&self) -> Result<C::Session, CoreError> {
		let mut current = self.current.lock().await;
		if let Some(session) = current.as_ref().filter(|session| !session.is_invalid()) {
			return Ok(session.clone());
		}
		*current = None;
		let credentials = match self.profile.load_credentials() {
			Ok(Some(credentials)) => credentials,
			Ok(None) => return Err(CoreError::NotLoggedIn),
			Err(error) => {
				log::warn!("{error}; log in again");
				return Err(CoreError::NotLoggedIn);
			}
		};
		match self.connector.connect(credentials).await {
			Ok(connected) => {
				self.save(&connected.credentials);
				*current = Some(connected.session.clone());
				Ok(connected.session)
			}
			Err(CoreError::Spotify(error)) if error.kind == ErrorKind::PermissionDenied => {
				log::warn!("Spotify rejected the stored login: {error}");
				self.profile.clear_credentials()?;
				Err(CoreError::NotLoggedIn)
			}
			Err(error) => Err(error),
		}
	}

	/// Logs in with an OAuth access token, stores the reusable login Spotify returns and keeps
	/// the session. Returns the Spotify username.
	pub async fn login_with_token(&self, access_token: String) -> Result<String, CoreError> {
		let mut current = self.current.lock().await;
		let connected =
			self.connector.connect(Credentials::with_access_token(access_token)).await?;
		self.profile.save_credentials(&connected.credentials)?;
		if let Some(previous) = current.replace(connected.session) {
			previous.shutdown();
		}
		Ok(connected.username)
	}

	pub async fn logout(&self) -> Result<(), CoreError> {
		let mut current = self.current.lock().await;
		if let Some(session) = current.take() {
			session.shutdown();
		}
		self.profile.clear_credentials()?;
		Ok(())
	}

	pub fn is_logged_in(&self) -> bool {
		self.profile.has_credentials()
	}

	fn save(&self, credentials: &Credentials) {
		// Playing matters more than refreshing the stored login, which still works next time.
		if let Err(error) = self.profile.save_credentials(credentials) {
			log::warn!("couldn't store the refreshed Spotify login: {error}");
		}
	}
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
		let account_type =
			wait_for_attribute(|| session.get_user_attribute("type"), ACCOUNT_TYPE_TIMEOUT).await;
		if account_type.is_none() {
			log::warn!("Spotify didn't say which kind of account this is; trying anyway");
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

	#[tokio::test]
	async fn rejected_stored_credentials_are_cleared() {
		let setup = setup(
			Outcome::Fail(|| CoreError::Spotify(librespot_core::Error::permission_denied("bad"))),
			true,
		);
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		assert!(!setup.profile.has_credentials());
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
		let result = setup.manager.session().await;
		assert!(matches!(result, Err(CoreError::NotLoggedIn)), "{result:?}");
		assert_eq!(setup.connector.calls(), 0);
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
		setup.manager.logout().await.unwrap();
		assert!(!setup.profile.has_credentials());
		assert!(!setup.manager.is_logged_in());
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
