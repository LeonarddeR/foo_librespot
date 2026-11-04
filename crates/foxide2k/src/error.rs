//! The error type Rust-implemented services return; the boundary turns it into a C++ exception.

use std::borrow::Cow;
use std::fmt;

use crate::cxx::{ExceptionKind, HostException};

/// An error to report to foobar2000 as a C++ exception of class `kind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
	pub kind: ExceptionKind,
	pub message: Cow<'static, str>,
	/// The RTTI name of the foobar2000 exception this error came from, if it came from one.
	pub host_type: Option<String>,
}

pub type Result<T> = core::result::Result<T, Error>;

impl Error {
	pub fn new(kind: ExceptionKind, message: impl Into<Cow<'static, str>>) -> Self {
		Self { kind, message: message.into(), host_type: None }
	}

	/// `exception_aborted`: the user or foobar2000 cancelled the operation.
	pub fn aborted() -> Self {
		Self::new(ExceptionKind::Aborted, ExceptionKind::Aborted.default_message())
	}

	/// `exception_io_unsupported_format`: this input does not handle the path or file.
	pub fn unsupported_format(message: impl Into<Cow<'static, str>>) -> Self {
		Self::new(ExceptionKind::IoUnsupportedFormat, message)
	}

	/// `pfc::exception_not_implemented`.
	pub fn not_implemented() -> Self {
		Self::new(ExceptionKind::NotImplemented, ExceptionKind::NotImplemented.default_message())
	}

	/// `exception_tagging_unsupported`: the input cannot write tags.
	pub fn tagging_unsupported() -> Self {
		Self::new(
			ExceptionKind::TaggingUnsupported,
			ExceptionKind::TaggingUnsupported.default_message(),
		)
	}
}

impl From<HostException> for Error {
	fn from(exception: HostException) -> Self {
		Self {
			kind: exception.kind,
			message: exception.message.into(),
			host_type: Some(exception.type_name),
		}
	}
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.message)
	}
}

impl std::error::Error for Error {}
