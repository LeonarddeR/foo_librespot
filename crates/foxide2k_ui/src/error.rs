//! The error a dialog function returns when Windows refuses it.

use std::fmt;

/// What went wrong, in words the user can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
	pub message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.message)
	}
}

impl std::error::Error for Error {}
