//! Turning Spotify errors into the foobar2000 exceptions that explain them.

use foo_librespot_core::CoreError;
use oxibar::cxx::ExceptionKind;
use oxibar::error::Error;

/// What the user reads when nobody is logged in. Phase 4's preferences page replaces the advice.
const NOT_LOGGED_IN: &str =
	"Not logged in to Spotify. For now, log in with the spotify_dev tool; see TESTING.md.";

/// The foobar2000 exception for `error`, with a message the user can act on.
#[allow(clippy::needless_pass_by_value, reason = "made for map_err")]
pub fn host_error(error: CoreError) -> Error {
	let kind = match &error {
		CoreError::Aborted => return Error::aborted(),
		CoreError::NotLoggedIn => return Error::new(ExceptionKind::Io, NOT_LOGGED_IN),
		CoreError::Unavailable(_) => ExceptionKind::IoNotFound,
		CoreError::Decode(_) => ExceptionKind::IoData,
		CoreError::Link(_) => ExceptionKind::IoUnsupportedFormat,
		CoreError::NotPremium { .. }
		| CoreError::Spotify(_)
		| CoreError::Io(_)
		| CoreError::Profile(_)
		| CoreError::Login(_) => ExceptionKind::Io,
	};
	Error::new(kind, format!("Spotify: {error}"))
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::CoreError;
	use foo_librespot_core::link::LinkError;
	use oxibar::cxx::ExceptionKind;

	use super::host_error;

	#[test]
	fn not_logged_in_reaches_the_host_as_an_io_error_with_advice() {
		let error = host_error(CoreError::NotLoggedIn);
		assert_eq!(error.kind, ExceptionKind::Io);
		assert!(error.message.contains("spotify_dev"), "{}", error.message);
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
