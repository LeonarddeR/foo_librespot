//! The foobar2000 exception types Foxide2k can throw and recognise.
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
	/// `foobar2000_io::exception_io_not_directory` (`SDK\exception_io.h:62`).
	IoNotDirectory,
	/// `foobar2000_io::exception_io_denied` (`SDK\exception_io.h:12`).
	IoDenied,
	/// `foobar2000_io::exception_io_object_not_seekable` (`SDK\exception_io.h:32`).
	IoObjectNotSeekable,
	/// `foobar2000_io::exception_io_data_truncation` (`SDK\exception_io.h:18`).
	IoDataTruncation,
	/// `exception_album_art_not_found`, in the global namespace (`SDK\album_art.h:38`).
	AlbumArtNotFound,
	/// `foobar2000_io::exception_io_denied_readonly` (`SDK\exception_io.h:14`).
	IoDeniedReadonly,
	/// `foobar2000_io::exception_io_object_is_remote` (`SDK\exception_io.h:24`).
	IoObjectIsRemote,
	/// `foobar2000_io::exception_io_sharing_violation` (`SDK\exception_io.h:26`).
	IoSharingViolation,
	/// `foobar2000_io::exception_io_device_full` (`SDK\exception_io.h:28`).
	IoDeviceFull,
	/// `foobar2000_io::exception_io_seek_out_of_range` (`SDK\exception_io.h:30`).
	IoSeekOutOfRange,
	/// `foobar2000_io::exception_io_no_length` (`SDK\exception_io.h:34`).
	IoNoLength,
	/// `foobar2000_io::exception_io_already_exists` (`SDK\exception_io.h:38`).
	IoAlreadyExists,
	/// `foobar2000_io::exception_io_no_data` (`SDK\exception_io.h:40`).
	IoNoData,
	/// `foobar2000_io::exception_io_network_not_reachable` (`SDK\exception_io.h:42`).
	IoNetworkNotReachable,
	/// `foobar2000_io::exception_io_write_protected` (`SDK\exception_io.h:44`).
	IoWriteProtected,
	/// `foobar2000_io::exception_io_file_corrupted` (`SDK\exception_io.h:46`).
	IoFileCorrupted,
	/// `foobar2000_io::exception_io_disk_change` (`SDK\exception_io.h:48`).
	IoDiskChange,
	/// `foobar2000_io::exception_io_directory_not_empty` (`SDK\exception_io.h:50`).
	IoDirectoryNotEmpty,
	/// `foobar2000_io::exception_io_net` (`SDK\exception_io.h:52`).
	IoNet,
	/// `foobar2000_io::exception_io_net_security` (`SDK\exception_io.h:54`).
	IoNetSecurity,
	/// `foobar2000_io::exception_io_net_timeout` (`SDK\exception_io.h:56`).
	IoNetTimeout,
	/// `foobar2000_io::exception_io_net_credentials` (`SDK\exception_io.h:58`).
	IoNetCredentials,
	/// `foobar2000_io::exception_io_dns` (`SDK\exception_io.h:60`).
	IoDns,
	/// `foobar2000_io::exception_io_unsupported_feature` (`SDK\exception_io.h:64`).
	IoUnsupportedFeature,
	/// `foobar2000_io::exception_io_timeout` (`SDK\exception_io.h:66`).
	IoTimeout,
}

/// One C++ exception class: its decorated RTTI name, its base class and its default message.
pub(crate) struct TypeInfo {
	pub name: &'static str,
	pub base: Option<ExceptionKind>,
	pub default_message: &'static str,
}

/// How many classes Foxide2k knows.
pub(crate) const COUNT: usize = 37;

/// Indexed by `ExceptionKind as usize`.
pub(crate) const TYPES: [TypeInfo; COUNT] = [
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
	TypeInfo {
		name: ".?AVexception_io_not_directory@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Not a directory",
	},
	TypeInfo {
		name: ".?AVexception_io_denied@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Access denied",
	},
	TypeInfo {
		name: ".?AVexception_io_object_not_seekable@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Object is not seekable",
	},
	TypeInfo {
		name: ".?AVexception_io_data_truncation@foobar2000_io@@",
		base: Some(ExceptionKind::IoData),
		default_message: "Unsupported format or corrupted file",
	},
	TypeInfo {
		name: ".?AVexception_album_art_not_found@@",
		base: Some(ExceptionKind::IoNotFound),
		default_message: "Attached picture not found",
	},
	TypeInfo {
		name: ".?AVexception_io_denied_readonly@foobar2000_io@@",
		base: Some(ExceptionKind::IoDenied),
		default_message: "File is read-only",
	},
	TypeInfo {
		name: ".?AVexception_io_object_is_remote@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "This operation is not supported on remote objects",
	},
	TypeInfo {
		name: ".?AVexception_io_sharing_violation@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "File is already in use",
	},
	TypeInfo {
		name: ".?AVexception_io_device_full@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Device full",
	},
	TypeInfo {
		name: ".?AVexception_io_seek_out_of_range@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Seek offset out of range",
	},
	TypeInfo {
		name: ".?AVexception_io_no_length@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Length of object is unknown",
	},
	TypeInfo {
		name: ".?AVexception_io_already_exists@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Object already exists",
	},
	TypeInfo {
		name: ".?AVexception_io_no_data@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "The process receiving or sending data has terminated",
	},
	TypeInfo {
		name: ".?AVexception_io_network_not_reachable@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Network not reachable",
	},
	TypeInfo {
		name: ".?AVexception_io_write_protected@foobar2000_io@@",
		base: Some(ExceptionKind::IoDenied),
		default_message: "The media is write protected",
	},
	TypeInfo {
		name: ".?AVexception_io_file_corrupted@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "The file is corrupted",
	},
	TypeInfo {
		name: ".?AVexception_io_disk_change@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Disc not available",
	},
	TypeInfo {
		name: ".?AVexception_io_directory_not_empty@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Directory not empty",
	},
	TypeInfo {
		name: ".?AVexception_io_net@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Network error",
	},
	TypeInfo {
		name: ".?AVexception_io_net_security@foobar2000_io@@",
		base: Some(ExceptionKind::IoNet),
		default_message: "Network security error",
	},
	TypeInfo {
		name: ".?AVexception_io_net_timeout@foobar2000_io@@",
		base: Some(ExceptionKind::IoNet),
		default_message: "Network operation timed out",
	},
	TypeInfo {
		name: ".?AVexception_io_net_credentials@foobar2000_io@@",
		base: Some(ExceptionKind::IoDenied),
		default_message: "Invalid network credentials",
	},
	TypeInfo {
		name: ".?AVexception_io_dns@foobar2000_io@@",
		base: Some(ExceptionKind::IoNet),
		default_message: "DNS error",
	},
	TypeInfo {
		name: ".?AVexception_io_unsupported_feature@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Unsupported feature",
	},
	TypeInfo {
		name: ".?AVexception_io_timeout@foobar2000_io@@",
		base: Some(ExceptionKind::Io),
		default_message: "Operation timed out",
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

const CHAINS: [([&str; MAX_CHAIN], usize); COUNT] = {
	let mut chains = [([""; MAX_CHAIN], 0); COUNT];
	let mut index = 0;
	while index < COUNT {
		chains[index] = chain_of(ExceptionKind::ALL[index]);
		index += 1;
	}
	chains
};

impl ExceptionKind {
	pub const ALL: [Self; COUNT] = [
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
		Self::IoNotDirectory,
		Self::IoDenied,
		Self::IoObjectNotSeekable,
		Self::IoDataTruncation,
		Self::AlbumArtNotFound,
		Self::IoDeniedReadonly,
		Self::IoObjectIsRemote,
		Self::IoSharingViolation,
		Self::IoDeviceFull,
		Self::IoSeekOutOfRange,
		Self::IoNoLength,
		Self::IoAlreadyExists,
		Self::IoNoData,
		Self::IoNetworkNotReachable,
		Self::IoWriteProtected,
		Self::IoFileCorrupted,
		Self::IoDiskChange,
		Self::IoDirectoryNotEmpty,
		Self::IoNet,
		Self::IoNetSecurity,
		Self::IoNetTimeout,
		Self::IoNetCredentials,
		Self::IoDns,
		Self::IoUnsupportedFeature,
		Self::IoTimeout,
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

	/// The class with this RTTI name, if Foxide2k knows it.
	pub fn from_type_name(name: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|kind| TYPES[*kind as usize].name == name)
	}
}

#[cfg(test)]
mod tests {
	use super::ExceptionKind;
	use crate::cxx::{throw, try_host};

	#[test]
	fn new_kinds_have_their_class_chains() {
		let io = ".?AVexception_io@foobar2000_io@@";
		let std = ".?AVexception@std@@";
		assert_eq!(
			ExceptionKind::IoNotDirectory.type_names(),
			[".?AVexception_io_not_directory@foobar2000_io@@", io, std]
		);
		assert_eq!(
			ExceptionKind::IoDenied.type_names(),
			[".?AVexception_io_denied@foobar2000_io@@", io, std]
		);
		assert_eq!(
			ExceptionKind::IoObjectNotSeekable.type_names(),
			[".?AVexception_io_object_not_seekable@foobar2000_io@@", io, std]
		);
		assert_eq!(
			ExceptionKind::IoDataTruncation.type_names(),
			[
				".?AVexception_io_data_truncation@foobar2000_io@@",
				".?AVexception_io_data@foobar2000_io@@",
				io,
				std
			]
		);
		assert_eq!(
			ExceptionKind::AlbumArtNotFound.type_names(),
			[
				".?AVexception_album_art_not_found@@",
				".?AVexception_io_not_found@foobar2000_io@@",
				io,
				std
			]
		);
	}

	/// Every class `SDK\exception_io.h` declares, with its base (empty for `pfc::exception`) and
	/// its default message, as written there.
	const EXCEPTION_IO_H: [(&str, &str, &str); 30] = [
		("exception_io", "", "I/O error"),
		("exception_io_not_found", "exception_io", "Object not found"),
		("exception_io_denied", "exception_io", "Access denied"),
		("exception_io_denied_readonly", "exception_io_denied", "File is read-only"),
		("exception_io_data", "exception_io", "Unsupported format or corrupted file"),
		(
			"exception_io_data_truncation",
			"exception_io_data",
			"Unsupported format or corrupted file",
		),
		("exception_io_unsupported_format", "exception_io_data", "Unsupported file format"),
		("exception_io_bad_subsong_index", "exception_io_data", "Unexpected subsong index"),
		(
			"exception_io_object_is_remote",
			"exception_io",
			"This operation is not supported on remote objects",
		),
		("exception_io_sharing_violation", "exception_io", "File is already in use"),
		("exception_io_device_full", "exception_io", "Device full"),
		("exception_io_seek_out_of_range", "exception_io", "Seek offset out of range"),
		("exception_io_object_not_seekable", "exception_io", "Object is not seekable"),
		("exception_io_no_length", "exception_io", "Length of object is unknown"),
		("exception_io_no_handler_for_path", "exception_io", "Invalid path"),
		("exception_io_already_exists", "exception_io", "Object already exists"),
		(
			"exception_io_no_data",
			"exception_io",
			"The process receiving or sending data has terminated",
		),
		("exception_io_network_not_reachable", "exception_io", "Network not reachable"),
		("exception_io_write_protected", "exception_io_denied", "The media is write protected"),
		("exception_io_file_corrupted", "exception_io", "The file is corrupted"),
		("exception_io_disk_change", "exception_io", "Disc not available"),
		("exception_io_directory_not_empty", "exception_io", "Directory not empty"),
		("exception_io_net", "exception_io", "Network error"),
		("exception_io_net_security", "exception_io_net", "Network security error"),
		("exception_io_net_timeout", "exception_io_net", "Network operation timed out"),
		("exception_io_net_credentials", "exception_io_denied", "Invalid network credentials"),
		("exception_io_dns", "exception_io_net", "DNS error"),
		("exception_io_not_directory", "exception_io", "Not a directory"),
		("exception_io_unsupported_feature", "exception_io", "Unsupported feature"),
		("exception_io_timeout", "exception_io", "Operation timed out"),
	];

	#[test]
	fn every_exception_io_class_is_known_with_its_base_and_message() {
		for (class, base, message) in EXCEPTION_IO_H {
			let name = format!(".?AV{class}@foobar2000_io@@");
			let kind = ExceptionKind::from_type_name(&name)
				.unwrap_or_else(|| panic!("{class} is not known"));
			let expected_base = if base.is_empty() {
				".?AVexception@std@@".to_owned()
			} else {
				format!(".?AV{base}@foobar2000_io@@")
			};
			assert_eq!(kind.type_names().get(1).copied(), Some(expected_base.as_str()), "{class}");
			assert_eq!(kind.default_message(), message, "{class}");
		}
	}

	#[test]
	fn album_art_not_found_is_caught_as_not_found() {
		let error =
			try_host(|| unsafe { throw(ExceptionKind::AlbumArtNotFound, "no cover") }).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::AlbumArtNotFound);
		assert_eq!(error.type_name, ".?AVexception_album_art_not_found@@");
		assert_eq!(error.message, "no cover");
	}

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
