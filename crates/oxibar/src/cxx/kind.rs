//! The foobar2000 exception types OxiBar can throw and recognise.
//!
//! Every foobar2000 exception is declared with `PFC_DECLARE_EXCEPTION` (`pfc\primitives.h:18-31`),
//! which adds no data and no virtual functions to `std::exception`. They differ only in their
//! MSVC RTTI type names, so one object layout serves them all.

/// A foobar2000 exception class. The discriminant indexes [`TYPES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExceptionKind {
	/// `std::exception`, which is also `pfc::exception`.
	Std,
	/// `foobar2000_io::exception_aborted` (`SDK\abort_callback.h:16`).
	Aborted,
	/// `foobar2000_io::exception_io` (`SDK\exception_io.h:7`).
	Io,
	/// `foobar2000_io::exception_io_not_found` (`SDK\exception_io.h:9`).
	IoNotFound,
	/// `foobar2000_io::exception_io_data` (`SDK\exception_io.h:16`).
	IoData,
	/// `foobar2000_io::exception_io_unsupported_format` (`SDK\exception_io.h:20`).
	IoUnsupportedFormat,
	/// `foobar2000_io::exception_io_bad_subsong_index` (`SDK\exception_io.h:22`).
	IoBadSubsongIndex,
	/// `foobar2000_io::exception_io_no_handler_for_path` (`SDK\exception_io.h:36`).
	IoNoHandlerForPath,
	/// `exception_tagging_unsupported`, in the global namespace (`SDK\input.h:7`).
	TaggingUnsupported,
	/// `pfc::exception_bug_check` (`pfc\primitives.h:193`).
	BugCheck,
	/// `pfc::exception_invalid_params` (`pfc\primitives.h:194`).
	InvalidParams,
	/// `pfc::exception_not_implemented` (`pfc\primitives.h:196`).
	NotImplemented,
}

/// One C++ exception class: its decorated RTTI name, its base class and its default message.
pub(crate) struct TypeInfo {
	pub name: &'static str,
	pub base: Option<ExceptionKind>,
	pub default_message: &'static str,
}

/// Indexed by `ExceptionKind as usize`.
pub(crate) const TYPES: [TypeInfo; 12] = [
	TypeInfo { name: ".?AVexception@std@@", base: None, default_message: "Unknown exception" },
	TypeInfo {
		name: ".?AVexception_aborted@foobar2000_io@@",
		base: Some(ExceptionKind::Std),
		default_message: "User abort",
	},
	TypeInfo {
		name: ".?AVexception_io@foobar2000_io@@",
		base: Some(ExceptionKind::Std),
		default_message: "I/O error",
	},
	TypeInfo {
		name: ".?AVexception_io_not_found@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Object not found",
	},
	TypeInfo {
		name: ".?AVexception_io_data@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Unsupported format or corrupted file",
	},
	TypeInfo {
		name: ".?AVexception_io_unsupported_format@foobar2000_io@@",
		base: Some(ExceptionKind::IoData),
		default_message: "Unsupported file format",
	},
	TypeInfo {
		name: ".?AVexception_io_bad_subsong_index@foobar2000_io@@",
		base: Some(ExceptionKind::IoData),
		default_message: "Unexpected subsong index",
	},
	TypeInfo {
		name: ".?AVexception_io_no_handler_for_path@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Invalid path",
	},
	TypeInfo {
		name: ".?AVexception_tagging_unsupported@@",
		base: Some(ExceptionKind::IoData),
		default_message: "Tagging of this file format is not supported",
	},
	TypeInfo {
		name: ".?AVexception_bug_check@pfc@@",
		base: Some(ExceptionKind::Std),
		default_message: "Bug check",
	},
	TypeInfo {
		name: ".?AVexception_invalid_params@pfc@@",
		base: Some(ExceptionKind::BugCheck),
		default_message: "Invalid parameters",
	},
	TypeInfo {
		name: ".?AVexception_not_implemented@pfc@@",
		base: Some(ExceptionKind::BugCheck),
		default_message: "Feature not implemented",
	},
];

/// The longest class chain, `exception_io_unsupported_format` and friends.
pub(crate) const MAX_CHAIN: usize = 4;

const fn chain_of(kind: ExceptionKind) -> ([&'static str; MAX_CHAIN], usize) {
	let mut names = [""; MAX_CHAIN];
	let mut len = 0;
	let mut current = Some(kind);
	while let Some(k) = current {
		names[len] = TYPES[k as usize].name;
		len += 1;
		current = TYPES[k as usize].base;
	}
	(names, len)
}

const CHAINS: [([&str; MAX_CHAIN], usize); 12] = {
	let mut chains = [([""; MAX_CHAIN], 0); 12];
	let mut index = 0;
	while index < 12 {
		chains[index] = chain_of(ExceptionKind::ALL[index]);
		index += 1;
	}
	chains
};

impl ExceptionKind {
	pub const ALL: [Self; 12] = [
		Self::Std,
		Self::Aborted,
		Self::Io,
		Self::IoNotFound,
		Self::IoData,
		Self::IoUnsupportedFormat,
		Self::IoBadSubsongIndex,
		Self::IoNoHandlerForPath,
		Self::TaggingUnsupported,
		Self::BugCheck,
		Self::InvalidParams,
		Self::NotImplemented,
	];

	/// The MSVC RTTI names of this class and all its bases, most-derived first.
	pub fn type_names(self) -> &'static [&'static str] {
		let (names, len) = &CHAINS[self as usize];
		&names[..*len]
	}

	/// The message the SDK's default constructor gives this class.
	pub fn default_message(self) -> &'static str {
		TYPES[self as usize].default_message
	}

	/// The class with this RTTI name, if OxiBar knows it.
	pub fn from_type_name(name: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|kind| TYPES[*kind as usize].name == name)
	}
}

#[cfg(test)]
mod tests {
	use super::ExceptionKind;

	#[test]
	fn unsupported_format_chain() {
		assert_eq!(
			ExceptionKind::IoUnsupportedFormat.type_names(),
			[
				".?AVexception_io_unsupported_format@foobar2000_io@@",
				".?AVexception_io_data@foobar2000_io@@",
				".?AVexception_io@foobar2000_io@@",
				".?AVexception@std@@",
			]
		);
	}

	#[test]
	fn aborted_chain() {
		assert_eq!(
			ExceptionKind::Aborted.type_names(),
			[".?AVexception_aborted@foobar2000_io@@", ".?AVexception@std@@"]
		);
	}

	#[test]
	fn tagging_unsupported_is_global() {
		assert_eq!(
			ExceptionKind::TaggingUnsupported.type_names(),
			[
				".?AVexception_tagging_unsupported@@",
				".?AVexception_io_data@foobar2000_io@@",
				".?AVexception_io@foobar2000_io@@",
				".?AVexception@std@@",
			]
		);
	}

	#[test]
	fn not_implemented_chain() {
		assert_eq!(
			ExceptionKind::NotImplemented.type_names(),
			[
				".?AVexception_not_implemented@pfc@@",
				".?AVexception_bug_check@pfc@@",
				".?AVexception@std@@"
			]
		);
	}

	#[test]
	fn bad_subsong_chain() {
		assert_eq!(
			ExceptionKind::IoBadSubsongIndex.type_names(),
			[
				".?AVexception_io_bad_subsong_index@foobar2000_io@@",
				".?AVexception_io_data@foobar2000_io@@",
				".?AVexception_io@foobar2000_io@@",
				".?AVexception@std@@",
			]
		);
	}
}
