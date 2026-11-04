//! The one error type every fallible call in this crate returns.

use crate::link::LinkError;
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
	#[error("not available on Spotify: {0}")]
	Unavailable(String),
	#[error("Spotify: {0}")]
	Spotify(#[from] librespot_core::Error),
	#[error("can't decode the audio: {0}")]
	Decode(String),
	#[error(transparent)]
	Io(#[from] std::io::Error),
	#[error(transparent)]
	Profile(#[from] ProfileError),
}

pub type Result<T> = std::result::Result<T, CoreError>;
