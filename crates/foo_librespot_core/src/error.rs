//! The one error type every fallible call in this crate returns.

use crate::link::LinkError;
use crate::login::LoginError;
use crate::profile::ProfileError;

/// What went wrong, in words a foobar2000 user can act on.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
	#[error("not logged in to Spotify")]
	NotLoggedIn,
	#[error("this Spotify account is {account_type}, but playing needs Spotify Premium")]
	NotPremium { account_type: String },
	#[error("aborted")]
	Aborted,
	#[error(transparent)]
	Link(#[from] LinkError),
	#[error("{0}")]
	Unavailable(String),
	/// Spotify refused the key that decrypts a track's audio. The code is Spotify's own, which it
	/// doesn't document. librespot asks again for about a minute before it passes on a code 2.
	#[error("Spotify refused the key that decrypts this track (code {code})")]
	KeyRefused { code: u16 },
	#[error("{0}")]
	Spotify(#[from] librespot_core::Error),
	#[error("can't decode the audio: {0}")]
	Decode(String),
	#[error(transparent)]
	Io(#[from] std::io::Error),
	#[error(transparent)]
	Profile(#[from] ProfileError),
	#[error(transparent)]
	Login(#[from] LoginError),
}

pub type Result<T> = std::result::Result<T, CoreError>;

#[cfg(test)]
mod tests {
	use super::CoreError;

	#[test]
	fn an_unavailable_item_shows_only_its_reason() {
		let error = CoreError::Unavailable("not available in your country (NL)".to_owned());
		assert_eq!(error.to_string(), "not available in your country (NL)");
	}

	#[test]
	fn a_librespot_error_shows_only_its_own_message() {
		let error = CoreError::from(librespot_core::Error::unavailable("gone"));
		assert_eq!(error.to_string(), librespot_core::Error::unavailable("gone").to_string());
	}
}
