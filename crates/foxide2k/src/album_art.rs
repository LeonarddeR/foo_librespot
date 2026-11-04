//! Registering an album-art extractor: the service foobar2000 asks for a track's pictures.
//!
//! foobar2000 opens the extractor whose `album_art_extractor_v2::get_guid` matches the input
//! that plays a path, gets an instance for that path and asks it for each kind of picture it
//! wants to show.

use std::ffi::{c_char, c_void};
use std::marker::PhantomData;

use crate::abi::abort_callback::AbortCallback;
use crate::abi::album_art::{
	ARTIST, AlbumArtExtractorInstanceVtbl, AlbumArtExtractorVtbl, COVER_BACK, COVER_FRONT,
	DATA_IID, DISC, EXTRACTOR_IID, EXTRACTOR_V2_IID, ICON, INSTANCE_IID, MemBlockVtbl,
};
use crate::abi::input::ByValueServicePtr;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abort::Abort;
use crate::boundary::guard;
use crate::component::Factory;
use crate::cxx::ExceptionKind;
use crate::error::{Error, Result};
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};
use crate::strings::str_of;

/// Which picture foobar2000 wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtKind {
	FrontCover,
	BackCover,
	Disc,
	Icon,
	Artist,
	/// A kind this version of Foxide2k has no name for.
	Other(Guid),
}

impl ArtKind {
	pub fn from_guid(guid: Guid) -> Self {
		match guid {
			COVER_FRONT => Self::FrontCover,
			COVER_BACK => Self::BackCover,
			DISC => Self::Disc,
			ICON => Self::Icon,
			ARTIST => Self::Artist,
			other => Self::Other(other),
		}
	}
}

/// A component's album art.
pub trait AlbumArt: 'static {
	/// The GUID of the input that plays these paths, which pairs the two.
	const INPUT_GUID: Guid;
	fn is_our_path(path: &str, extension: &str) -> bool;
	/// The picture's file bytes (usually JPEG), or `None` when there is none of this kind.
	fn query(path: &str, kind: ArtKind, abort: &Abort) -> Result<Option<Vec<u8>>>;
}

/// The factory that registers `T` with foobar2000.
pub const fn album_art_factory<T: AlbumArt>() -> Factory {
	Factory::per_instance(&EXTRACTOR_IID, album_art_service::<T>)
}

/// A new `album_art_extractor_v2` service for `T`.
pub fn album_art_service<T: AlbumArt>() -> ServicePtr {
	new_service(Extractor::<T>(PhantomData))
}

struct Extractor<T>(PhantomData<fn() -> T>);

unsafe impl<T: AlbumArt> ServiceImpl for Extractor<T> {
	type Vtbl = AlbumArtExtractorVtbl;
	const IIDS: &'static [Guid] = &[EXTRACTOR_V2_IID, EXTRACTOR_IID];
	const VTBL: &'static AlbumArtExtractorVtbl = &AlbumArtExtractorVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		is_our_path: is_our_path::<T>,
		open: open::<T>,
		get_guid: get_guid::<T>,
	};
}

unsafe extern "C-unwind" fn is_our_path<T: AlbumArt>(
	_this: *mut ServiceBase,
	path: *const c_char,
	extension: *const c_char,
) -> bool {
	unsafe { guard(|| Ok(T::is_our_path(str_of(path), str_of(extension)))) }
}

unsafe extern "C-unwind" fn open<T: AlbumArt>(
	_this: *mut ServiceBase,
	ret: *mut *mut ServiceBase,
	file: ByValueServicePtr,
	path: *const c_char,
	_abort: *mut AbortCallback,
) -> *mut *mut ServiceBase {
	unsafe {
		guard(|| {
			// Passed by value: ours to release. Spotify paths aren't files, so it is unused.
			drop(file);
			let instance =
				new_service(Instance::<T> { path: str_of(path).to_owned(), _art: PhantomData });
			// The hidden return slot is uninitialized storage for a service_ptr_t.
			ret.write(instance.into_raw());
			Ok(ret)
		})
	}
}

unsafe extern "C-unwind" fn get_guid<T: AlbumArt>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::INPUT_GUID) };
	ret
}

struct Instance<T> {
	path: String,
	_art: PhantomData<fn() -> T>,
}

unsafe impl<T: AlbumArt> ServiceImpl for Instance<T> {
	type Vtbl = AlbumArtExtractorInstanceVtbl;
	const IIDS: &'static [Guid] = &[INSTANCE_IID];
	const VTBL: &'static AlbumArtExtractorInstanceVtbl =
		&AlbumArtExtractorInstanceVtbl { base: ServiceBaseVtbl::new::<Self>(), query: query::<T> };
}

unsafe extern "C-unwind" fn query<T: AlbumArt>(
	this: *mut ServiceBase,
	ret: *mut *mut ServiceBase,
	what: *const Guid,
	abort: *mut AbortCallback,
) -> *mut *mut ServiceBase {
	unsafe {
		guard(|| {
			let instance = ServiceObject::<Instance<T>>::value(this);
			let abort = Abort::from_raw(abort);
			match T::query(&instance.path, ArtKind::from_guid(*what), &abort)? {
				Some(bytes) => {
					ret.write(new_service(MemBlock(bytes)).into_raw());
					Ok(ret)
				}
				None => Err(Error::new(
					ExceptionKind::AlbumArtNotFound,
					ExceptionKind::AlbumArtNotFound.default_message(),
				)),
			}
		})
	}
}

/// `album_art_data`: picture bytes owned by Rust.
struct MemBlock(Vec<u8>);

unsafe impl ServiceImpl for MemBlock {
	type Vtbl = MemBlockVtbl;
	const IIDS: &'static [Guid] = &[DATA_IID];
	const VTBL: &'static MemBlockVtbl =
		&MemBlockVtbl { base: ServiceBaseVtbl::new::<Self>(), data, size };
}

unsafe extern "C-unwind" fn data(this: *mut ServiceBase) -> *const c_void {
	unsafe { ServiceObject::<MemBlock>::value(this) }.0.as_ptr().cast()
}

unsafe extern "C-unwind" fn size(this: *mut ServiceBase) -> usize {
	unsafe { ServiceObject::<MemBlock>::value(this) }.0.len()
}

#[cfg(test)]
mod tests {
	use super::{AlbumArt, ArtKind, album_art_service};
	use crate::abi::album_art::{ARTIST, COVER_BACK, COVER_FRONT, EXTRACTOR_V2_IID};
	use crate::abort::Abort;
	use crate::cxx::ExceptionKind;
	use crate::error::Result;
	use crate::guid::Guid;
	use crate::testing::{AlbumArtHandle, CountedService};

	struct TestArt;

	impl AlbumArt for TestArt {
		const INPUT_GUID: Guid = Guid::from_u128(0x11111111_2222_3333_4444_555555555555);

		fn is_our_path(path: &str, _extension: &str) -> bool {
			path.starts_with("test://")
		}

		fn query(_path: &str, kind: ArtKind, _abort: &Abort) -> Result<Option<Vec<u8>>> {
			Ok(match kind {
				ArtKind::FrontCover => Some(b"\xff\xd8 front".to_vec()),
				ArtKind::Other(guid) if guid == Guid::from_u128(7) => Some(b"other".to_vec()),
				_ => None,
			})
		}
	}

	fn extractor() -> AlbumArtHandle {
		AlbumArtHandle::new(album_art_service::<TestArt>())
	}

	#[test]
	fn extractor_answers_v2_with_the_input_guid() {
		let extractor = extractor();
		assert!(extractor.service().query(&EXTRACTOR_V2_IID).is_some());
		assert_eq!(extractor.guid(), TestArt::INPUT_GUID);
		assert!(extractor.is_our_path("test://track", ""));
		assert!(!extractor.is_our_path("file://C:\\a.mp3", "mp3"));
	}

	#[test]
	fn front_cover_bytes_come_back_through_mem_block() {
		let instance = extractor().open("test://track", None).unwrap();
		assert_eq!(instance.query(&COVER_FRONT).unwrap(), b"\xff\xd8 front");
	}

	#[test]
	fn missing_art_throws_album_art_not_found() {
		let instance = extractor().open("test://track", None).unwrap();
		for kind in [COVER_BACK, ARTIST] {
			assert_eq!(instance.query(&kind).unwrap_err().kind, ExceptionKind::AlbumArtNotFound);
		}
	}

	#[test]
	fn unknown_kinds_arrive_as_other() {
		let instance = extractor().open("test://track", None).unwrap();
		assert_eq!(instance.query(&Guid::from_u128(7)).unwrap(), b"other");
	}

	#[test]
	fn open_releases_the_file_hint() {
		let (hint, destroyed) = CountedService::new();
		let _instance = extractor().open("test://track", Some(hint)).unwrap();
		assert_eq!(destroyed.get(), 1);
	}
}
