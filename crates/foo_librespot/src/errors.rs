//! Turning Spotify errors into the foobar2000 exceptions that explain them.

use std::sync::atomic::{AtomicBool, Ordering};

use foo_librespot_core::CoreError;
use foxide2k::console;
use foxide2k::cxx::ExceptionKind;
use foxide2k::error::Error;

/// What the user reads when nobody is logged in.
const NOT_LOGGED_IN: &str =
	"Not logged in to Spotify. Log in from File > Preferences > Tools > Spotify.";

/// Whether the console was told about the missing login yet; once per session is enough.
static TOLD_NOT_LOGGED_IN: AtomicBool = AtomicBool::new(false);

/// The foobar2000 exception for `error`, with a message the user can act on.
#[allow(clippy::needless_pass_by_value, reason = "made for map_err")]
pub fn host_error(error: CoreError) -> Error {
	let kind = match &error {
		CoreError::Aborted => return Error::aborted(),
		CoreError::NotLoggedIn => {
			if !TOLD_NOT_LOGGED_IN.swap(true, Ordering::SeqCst) {
				console::print(
					"foo_librespot: not logged in to Spotify; log in from File > Preferences > Tools > Spotify.",
				);
			}
			return Error::new(ExceptionKind::Io, NOT_LOGGED_IN);
		}
		CoreError::Unavailable(_) => ExceptionKind::IoNotFound,
		CoreError::Decode(_) => ExceptionKind::IoData,
		CoreError::Link(_) => ExceptionKind::IoUnsupportedFormat,
		CoreError::NotPremium { .. }
		| CoreError::Spotify(_)
		| CoreError::Io(_)
		| CoreError::Profile(_)
		| CoreError::Login(_) => ExceptionKind::Io,
	};
	let message = error.to_string();
	if message.starts_with("Spotify") {
		Error::new(kind, message)
	} else {
		Error::new(kind, format!("Spotify: {message}"))
	}
}

/// What to do when `foo_youtube` is loaded: its host filter takes every web link by default,
/// `open.spotify.com` included, so in some sessions it gets them before foo_librespot does.
pub fn youtube_hint(components: &[String]) -> Option<String> {
	components.iter().any(|name| name.eq_ignore_ascii_case("foo_youtube")).then(|| {
		"foo_youtube may take open.spotify.com links before foo_librespot sees them. Add the line \"deny spotify.com\" to foo_youtube's host filter in its preferences; spotify: links work either way.".to_owned()
	})
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::CoreError;
	use foo_librespot_core::link::LinkError;
	use foxide2k::cxx::ExceptionKind;

	use foxide2k::testing::FakeApi;

	use super::{host_error, youtube_hint};

	#[test]
	fn not_logged_in_message_points_to_the_preferences_page() {
		let error = host_error(CoreError::NotLoggedIn);
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(
			error.message,
			"Not logged in to Spotify. Log in from File > Preferences > Tools > Spotify."
		);
	}

	#[test]
	fn not_logged_in_is_told_on_the_console_once() {
		let api = FakeApi::new();
		api.install();
		host_error(CoreError::NotLoggedIn);
		host_error(CoreError::NotLoggedIn);
		let told = api
			.console_lines()
			.iter()
			.filter(|line| {
				*line
					== "foo_librespot: not logged in to Spotify; log in from File > Preferences > Tools > Spotify."
			})
			.count();
		assert_eq!(told, 1, "{:?}", api.console_lines());
	}

	#[test]
	fn youtube_hint_only_with_foo_youtube() {
		let loaded =
			|names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
		assert_eq!(youtube_hint(&loaded(&["foo_input_std", "foo_librespot"])), None);
		assert_eq!(
			youtube_hint(&loaded(&["foo_youtube", "foo_librespot"])).as_deref(),
			Some(
				"foo_youtube may take open.spotify.com links before foo_librespot sees them. Add the line \"deny spotify.com\" to foo_youtube's host filter in its preferences; spotify: links work either way."
			)
		);
	}

	#[test]
	fn aborted_becomes_exception_aborted() {
		assert_eq!(host_error(CoreError::Aborted).kind, ExceptionKind::Aborted);
	}

	#[test]
	fn unavailable_becomes_not_found_with_the_reason() {
		let error =
			host_error(CoreError::Unavailable("not available in your country (NL)".to_owned()));
		assert_eq!(error.kind, ExceptionKind::IoNotFound);
		assert!(error.message.contains("your country"), "{}", error.message);
	}

	#[test]
	fn an_unavailable_reason_says_spotify_once() {
		let error =
			host_error(CoreError::Unavailable("not available in your country (NL)".to_owned()));
		assert_eq!(error.message, "Spotify: not available in your country (NL)");
	}

	#[test]
	fn a_message_that_starts_with_spotify_says_it_once() {
		let error =
			host_error(CoreError::Unavailable("Spotify has no audio for this track".to_owned()));
		assert_eq!(error.message, "Spotify has no audio for this track");
	}

	#[test]
	fn link_errors_become_unsupported_format() {
		assert_eq!(
			host_error(CoreError::Link(LinkError::NotSpotify)).kind,
			ExceptionKind::IoUnsupportedFormat
		);
	}

	#[test]
	fn decode_errors_become_data_errors() {
		assert_eq!(host_error(CoreError::Decode("bad".to_owned())).kind, ExceptionKind::IoData);
	}

	#[test]
	fn not_premium_explains_itself() {
		let error = host_error(CoreError::NotPremium { account_type: "free".to_owned() });
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("Premium"), "{}", error.message);
	}

	#[test]
	fn io_errors_keep_their_message() {
		let timeout = std::io::Error::new(std::io::ErrorKind::TimedOut, "Spotify didn't answer");
		let error = host_error(CoreError::Io(timeout));
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("didn't answer"), "{}", error.message);
	}
}
