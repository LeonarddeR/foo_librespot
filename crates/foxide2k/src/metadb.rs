//! The tracks foobar2000 passes to a component, such as the selection under a right-click menu.

use std::borrow::Cow;
use std::ffi::CStr;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::abi::metadb_handle::{HandleListRaw, MetadbHandleVtbl};
use crate::cxx::try_host;
use crate::error::Result;
use crate::service::ServicePtr;

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

	/// Calls `f` with each item's path, in order, borrowed from the item. Invalid UTF-8 in a path
	/// is replaced; an item without a path gives an empty one.
	pub fn for_each_path(&self, mut f: impl FnMut(&str)) -> Result<()> {
		let raw = self.as_raw();
		for index in 0..self.len()? {
			let item = try_host(|| unsafe {
				let mut handle = std::ptr::null_mut();
				((*(*raw).vtbl).get_item_ex)(raw, &raw mut handle, index);
				let handle = ServicePtr::from_raw_owned(handle)?;
				let vtbl = &*(*handle.as_raw()).vtbl.cast::<MetadbHandleVtbl>();
				let location = (vtbl.get_location)(handle.as_raw());
				let path = ((*(*location).vtbl).get_path)(location);
				Some((handle, path))
			})?;
			// `item` keeps the handle, which owns the path, alive until `f` returns.
			let path = match &item {
				Some((_, path)) if !path.is_null() => {
					unsafe { CStr::from_ptr(*path) }.to_string_lossy()
				}
				_ => Cow::Borrowed(""),
			};
			f(&path);
		}
		Ok(())
	}

	/// Each item's path, in order.
	pub fn paths(&self) -> Result<Vec<String>> {
		let mut paths = Vec::new();
		self.for_each_path(|path| paths.push(path.to_owned()))?;
		Ok(paths)
	}
}

#[cfg(test)]
mod tests {
	use super::HandleList;
	use crate::testing::FakeHandleList;

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
	fn an_empty_list_has_no_paths() {
		let list = FakeHandleList::new(&[]);
		let handles = unsafe { HandleList::from_raw(list.as_raw()) };
		assert!(handles.paths().unwrap().is_empty());
		assert_eq!(handles.len().unwrap(), 0);
		assert!(handles.is_empty().unwrap());
	}
}
