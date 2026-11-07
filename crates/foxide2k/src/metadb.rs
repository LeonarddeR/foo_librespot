//! The tracks foobar2000 passes to a component, such as the selection under a right-click menu.

use std::borrow::Cow;
use std::convert::Infallible;
use std::ffi::CStr;
use std::marker::PhantomData;
use std::ops::ControlFlow;
use std::ptr::NonNull;

use crate::abi::metadb_handle::{HandleListRaw, MetadbHandleVtbl};
use crate::cxx::try_host;
use crate::error::Result;
use crate::file_info_impl::FileInfoImpl;
use crate::service::ServicePtr;

/// One track foobar2000 handed over, a `metadb_handle_ptr`. Holds a reference of its own.
pub struct Track(ServicePtr);

impl Track {
	/// # Safety
	/// `handle` must be a `metadb_handle`.
	pub(crate) unsafe fn from_handle(handle: ServicePtr) -> Self {
		Self(handle)
	}

	fn vtbl(&self) -> &MetadbHandleVtbl {
		// SAFETY: `from_handle` requires a `metadb_handle`.
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<MetadbHandleVtbl>() }
	}

	/// Calls `f` with the track's path, borrowed from the track. Invalid UTF-8 in the path is
	/// replaced; a track without a path gives an empty one.
	fn with_path<R>(&self, f: impl FnOnce(&str) -> R) -> Result<R> {
		let raw = self.0.as_raw();
		let get_location = self.vtbl().get_location;
		let path = try_host(|| unsafe {
			let location = get_location(raw);
			((*(*location).vtbl).get_path)(location)
		})?;
		// The handle owns the path, and `self` keeps the handle alive until `f` returns.
		let path = if path.is_null() {
			Cow::Borrowed("")
		} else {
			unsafe { CStr::from_ptr(path) }.to_string_lossy()
		};
		Ok(f(&path))
	}

	/// The track's path, such as `sptf://spotify:track:<id>`.
	pub fn path(&self) -> Result<String> {
		self.with_path(str::to_owned)
	}

	/// Every value of field `name`, such as ARTIST, matched case-insensitively. Empty when the
	/// track has none, or when foobar2000 doesn't know the track's info yet.
	pub fn meta(&self, name: &str) -> Result<Vec<String>> {
		let raw = self.0.as_raw();
		let get_info = self.vtbl().get_info;
		let mut info = FileInfoImpl::new();
		let target = info.as_raw();
		let known = try_host(|| unsafe { get_info(raw, target) })?;
		Ok(if known { info.meta(name) } else { Vec::new() })
	}
}

/// A `metadb_handle_list_cref` foobar2000 passed in, borrowed for the call.
pub struct HandleList<'a> {
	raw: NonNull<HandleListRaw>,
	_borrow: PhantomData<&'a HandleListRaw>,
}

impl HandleList<'_> {
	/// # Safety
	/// `raw` must point to a live list that outlives the value.
	pub unsafe fn from_raw(raw: *const HandleListRaw) -> Self {
		Self { raw: NonNull::from(unsafe { &*raw }), _borrow: PhantomData }
	}

	pub const fn as_raw(&self) -> *const HandleListRaw {
		self.raw.as_ptr()
	}

	pub fn len(&self) -> Result<usize> {
		let raw = self.as_raw();
		Ok(try_host(|| unsafe { ((*(*raw).vtbl).get_count)(raw) })?)
	}

	pub fn is_empty(&self) -> Result<bool> {
		Ok(self.len()? == 0)
	}

	/// The item at `index`, holding a reference of its own; `None` for a null item.
	fn item(&self, index: usize) -> Result<Option<Track>> {
		let raw = self.as_raw();
		let handle = try_host(|| unsafe {
			let mut handle = std::ptr::null_mut();
			((*(*raw).vtbl).get_item_ex)(raw, &raw mut handle, index);
			ServicePtr::from_raw_owned(handle)
		})?;
		// SAFETY: a `metadb_handle_list` holds `metadb_handle`s.
		Ok(handle.map(|handle| unsafe { Track::from_handle(handle) }))
	}

	/// Calls `f` with each item's path, in order, borrowed from the item, until `f` breaks; returns
	/// where it broke, or `Continue` when it never did. Invalid UTF-8 in a path is replaced; an item
	/// without a path gives an empty one.
	pub fn try_for_each_path<B>(
		&self,
		mut f: impl FnMut(&str) -> ControlFlow<B>,
	) -> Result<ControlFlow<B>> {
		for index in 0..self.len()? {
			let flow = match self.item(index)? {
				Some(track) => track.with_path(&mut f)?,
				None => f(""),
			};
			if flow.is_break() {
				return Ok(flow);
			}
		}
		Ok(ControlFlow::Continue(()))
	}

	/// Calls `f` with each item's path, in order, as [`try_for_each_path`](Self::try_for_each_path)
	/// does without a break.
	pub fn for_each_path(&self, mut f: impl FnMut(&str)) -> Result<()> {
		let ControlFlow::Continue(()) = self.try_for_each_path(|path| {
			f(path);
			ControlFlow::<Infallible>::Continue(())
		})?;
		Ok(())
	}

	/// Each item's path, in order.
	pub fn paths(&self) -> Result<Vec<String>> {
		let mut paths = Vec::new();
		self.for_each_path(|path| paths.push(path.to_owned()))?;
		Ok(paths)
	}

	/// Each item, in order, holding a reference of its own. Null items are left out.
	pub fn tracks(&self) -> Result<Vec<Track>> {
		(0..self.len()?).filter_map(|index| self.item(index).transpose()).collect()
	}
}

#[cfg(test)]
mod tests {
	use std::ops::ControlFlow;

	use super::HandleList;
	use crate::testing::FakeHandleList;

	const PATH: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

	#[test]
	fn paths_reads_each_items_path_in_order() {
		let list = FakeHandleList::new(&[
			"sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO",
			"file://C:\\a.mp3",
		]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		assert_eq!(
			handles.paths().unwrap(),
			["sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO", "file://C:\\a.mp3"]
		);
		assert_eq!(handles.len().unwrap(), 2);
		assert!(!handles.is_empty().unwrap());
	}

	#[test]
	fn for_each_path_lends_each_path_in_order() {
		let list = FakeHandleList::new(&["sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO", ""]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		let mut seen = Vec::new();
		handles.for_each_path(|path| seen.push(path.len())).unwrap();
		assert_eq!(seen, [43, 0]);
	}

	#[test]
	fn try_for_each_path_stops_at_the_first_break() {
		let list = FakeHandleList::new(&["file://C:\\a.mp3", PATH, "", PATH]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		let mut seen = Vec::new();
		let flow = handles.try_for_each_path(|path| {
			seen.push(path.len());
			if path.starts_with("sptf://") {
				ControlFlow::Break(path.len())
			} else {
				ControlFlow::Continue(())
			}
		});
		assert_eq!(flow.unwrap(), ControlFlow::Break(43));
		assert_eq!(seen, [15, 43]);
	}

	#[test]
	fn try_for_each_path_without_a_break_lends_every_path() {
		let list = FakeHandleList::new(&["file://C:\\a.mp3", ""]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		let mut seen = Vec::new();
		let flow = handles.try_for_each_path(|path| {
			seen.push(path.to_owned());
			ControlFlow::<()>::Continue(())
		});
		assert_eq!(flow.unwrap(), ControlFlow::Continue(()));
		assert_eq!(seen, ["file://C:\\a.mp3", ""]);
	}

	#[test]
	fn a_tracks_fields_come_through_get_info() {
		let list = FakeHandleList::with_meta(&[(
			PATH,
			&[("ARTIST", &["Keith Getty", "Kristyn Getty"]), ("TITLE", &["In Christ Alone"])],
		)]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		let tracks = handles.tracks().unwrap();
		assert_eq!(tracks.len(), 1);
		assert_eq!(tracks[0].meta("ARTIST").unwrap(), ["Keith Getty", "Kristyn Getty"]);
		assert_eq!(tracks[0].meta("title").unwrap(), ["In Christ Alone"]);
		assert_eq!(tracks[0].path().unwrap(), PATH);
	}

	#[test]
	fn a_missing_field_is_empty() {
		let list = FakeHandleList::with_meta(&[(PATH, &[("ARTIST", &["Keith Getty"])])]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		assert!(handles.tracks().unwrap()[0].meta("ALBUM").unwrap().is_empty());
	}

	#[test]
	fn a_track_without_info_has_no_fields() {
		let list = FakeHandleList::new(&[PATH, "file://C:\\a.mp3"]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		let tracks = handles.tracks().unwrap();
		let paths: Vec<String> = tracks.iter().map(|track| track.path().unwrap()).collect();
		assert_eq!(paths, [PATH, "file://C:\\a.mp3"]);
		assert!(tracks[0].meta("ARTIST").unwrap().is_empty());
	}

	#[test]
	fn an_empty_list_has_no_paths() {
		let list = FakeHandleList::new(&[]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		assert!(handles.paths().unwrap().is_empty());
		assert_eq!(handles.len().unwrap(), 0);
		assert!(handles.is_empty().unwrap());
	}
}
