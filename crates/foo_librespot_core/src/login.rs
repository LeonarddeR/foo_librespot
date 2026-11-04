//! Logging in through the browser with Spotify's OAuth device flow.

use std::future::Future;

use librespot_core::SessionConfig;
use librespot_oauth::{DeviceAuthClient, DeviceAuthClientBuilder, DeviceAuthorization};

/// The scopes Spotify's desktop app asks for, copied in order from librespot's `src/main.rs`.
/// The device flow only works with Spotify's own client id, which accepts these.
pub const OAUTH_SCOPES: &[&str] = &[
	"app-remote-control",
	"playlist-modify",
	"playlist-modify-private",
	"playlist-modify-public",
	"playlist-read",
	"playlist-read-collaborative",
	"playlist-read-private",
	"streaming",
	"ugc-image-upload",
	"user-follow-modify",
	"user-follow-read",
	"user-library-modify",
	"user-library-read",
	"user-modify",
	"user-modify-playback-state",
	"user-modify-private",
	"user-personalized",
	"user-read-birthdate",
	"user-read-currently-playing",
	"user-read-email",
	"user-read-play-history",
	"user-read-playback-position",
	"user-read-playback-state",
	"user-read-private",
	"user-read-recently-played",
	"user-top-read",
];

/// What the user needs to confirm the login: a code and the page to enter it on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceCode {
	pub user_code: String,
	pub verification_uri: String,
	/// The page with the code already filled in, when Spotify offers one.
	pub url: String,
}

/// Why a login didn't finish.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LoginError {
	#[error("the login was declined in the browser")]
	Denied,
	#[error("the login code expired before it was confirmed")]
	Expired,
	#[error("the login failed: {0}")]
	Failed(String),
}

/// The two halves of the device flow, so the flow can be tested without Spotify.
pub trait Authorizer: Send + Sync {
	type Pending: Send;

	fn request_code(
		&self,
	) -> impl Future<Output = Result<(DeviceCode, Self::Pending), LoginError>> + Send;

	/// Waits until the user confirms in the browser, and returns the access token.
	fn wait_for_token(
		&self,
		pending: &Self::Pending,
	) -> impl Future<Output = Result<String, LoginError>> + Send;
}

/// Runs the device flow: asks Spotify for a code, hands it to `on_code` to show the user, and
/// waits for the user to confirm. Cancel it by dropping the future.
pub async fn device_login<A: Authorizer>(
	authorizer: &A,
	on_code: impl FnOnce(&DeviceCode) + Send,
) -> Result<String, LoginError> {
	let (code, pending) = authorizer.request_code().await?;
	on_code(&code);
	authorizer.wait_for_token(&pending).await
}

/// oauth2 reports the device flow's outcome only as text, so look for the standard error codes.
pub fn classify_oauth_error(message: &str) -> LoginError {
	if message.contains("access_denied") {
		LoginError::Denied
	} else if message.contains("expired_token") {
		LoginError::Expired
	} else {
		LoginError::Failed(message.to_owned())
	}
}

/// The real device flow, through librespot's OAuth client.
pub struct LibrespotAuthorizer {
	client: DeviceAuthClient,
}

impl LibrespotAuthorizer {
	pub fn new() -> Result<Self, LoginError> {
		let client_id = SessionConfig::default().client_id;
		let client = DeviceAuthClientBuilder::new(&client_id, OAUTH_SCOPES.to_vec())
			.build()
			.map_err(|error| classify_oauth_error(&error.to_string()))?;
		Ok(Self { client })
	}
}

impl Authorizer for LibrespotAuthorizer {
	type Pending = DeviceAuthorization;

	async fn request_code(&self) -> Result<(DeviceCode, DeviceAuthorization), LoginError> {
		let authorization = self
			.client
			.request_device_code_async()
			.await
			.map_err(|error| classify_oauth_error(&error.to_string()))?;
		let code = DeviceCode {
			user_code: authorization.user_code().to_owned(),
			verification_uri: authorization.verification_uri().to_owned(),
			url: authorization.url().to_owned(),
		};
		Ok((code, authorization))
	}

	async fn wait_for_token(&self, pending: &DeviceAuthorization) -> Result<String, LoginError> {
		self.client
			.poll_for_token_async(pending)
			.await
			.map(|token| token.access_token)
			.map_err(|error| classify_oauth_error(&error.to_string()))
	}
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::sync::{Arc, Mutex};
	use std::time::{Duration, Instant};

	use super::{Authorizer, DeviceCode, LoginError, classify_oauth_error, device_login};

	fn code() -> DeviceCode {
		DeviceCode {
			user_code: "ABCD-EFGH".to_owned(),
			verification_uri: "https://spotify.com/pair".to_owned(),
			url: "https://spotify.com/pair?code=ABCD-EFGH".to_owned(),
		}
	}

	/// Sets its flag when dropped, to prove a pending login was dropped.
	struct DropFlag(Arc<AtomicBool>);

	impl Drop for DropFlag {
		fn drop(&mut self) {
			self.0.store(true, Ordering::SeqCst);
		}
	}

	#[derive(Default)]
	struct FakeAuthorizer {
		events: Arc<Mutex<Vec<&'static str>>>,
		fail_request: bool,
		never_finish: Option<Arc<AtomicBool>>,
	}

	impl Authorizer for FakeAuthorizer {
		type Pending = ();

		async fn request_code(&self) -> Result<(DeviceCode, ()), LoginError> {
			tokio::task::yield_now().await;
			self.events.lock().unwrap().push("request");
			if self.fail_request {
				return Err(LoginError::Failed("no network".to_owned()));
			}
			Ok((code(), ()))
		}

		async fn wait_for_token(&self, (): &()) -> Result<String, LoginError> {
			self.events.lock().unwrap().push("wait");
			if let Some(dropped) = &self.never_finish {
				let _guard = DropFlag(Arc::clone(dropped));
				std::future::pending::<()>().await;
			}
			Ok("tok".to_owned())
		}
	}

	#[tokio::test]
	async fn device_login_reports_the_code_before_waiting() {
		let authorizer = FakeAuthorizer::default();
		let events = Arc::clone(&authorizer.events);
		device_login(&authorizer, |shown| {
			assert_eq!(*shown, code());
			events.lock().unwrap().push("on_code");
		})
		.await
		.unwrap();
		assert_eq!(*authorizer.events.lock().unwrap(), ["request", "on_code", "wait"]);
	}

	#[tokio::test]
	async fn device_login_returns_the_token() {
		let token = device_login(&FakeAuthorizer::default(), |_| {}).await;
		assert_eq!(token, Ok("tok".to_owned()));
	}

	#[tokio::test]
	async fn code_request_failure_skips_on_code() {
		let authorizer = FakeAuthorizer { fail_request: true, ..FakeAuthorizer::default() };
		let result = device_login(&authorizer, |_| panic!("no code was issued")).await;
		assert_eq!(result, Err(LoginError::Failed("no network".to_owned())));
	}

	#[test]
	fn denied_and_expired_are_recognised() {
		assert_eq!(
			classify_oauth_error("Server returned error response: access_denied"),
			LoginError::Denied
		);
		assert_eq!(
			classify_oauth_error("Server returned error response: expired_token: code expired"),
			LoginError::Expired
		);
		assert_eq!(classify_oauth_error("timeout"), LoginError::Failed("timeout".to_owned()));
	}

	#[test]
	fn dropping_device_login_cancels_it() {
		let runtime = tokio::runtime::Runtime::new().unwrap();
		let dropped = Arc::new(AtomicBool::new(false));
		let authorizer = FakeAuthorizer {
			never_finish: Some(Arc::clone(&dropped)),
			..FakeAuthorizer::default()
		};
		let task = runtime.spawn(async move { device_login(&authorizer, |_| {}).await });
		std::thread::sleep(Duration::from_millis(50));
		task.abort();
		let started = Instant::now();
		while !dropped.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(1) {
			std::thread::sleep(Duration::from_millis(10));
		}
		assert!(dropped.load(Ordering::SeqCst));
	}
}
