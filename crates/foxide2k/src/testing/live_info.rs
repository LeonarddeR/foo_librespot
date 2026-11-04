//! The list foobar2000 passes to `input_decoder_v5::poll_live_info`.

use std::cell::RefCell;

use crate::abi::file::FileStats;
use crate::abi::live_info::{FileInfoFilterVtbl, InputLiveInfo};
use crate::abi::pfc_list::{ListBase, ListBaseConst, ListBaseVtbl};
use crate::abi::service::ServiceBase;
use crate::cxx::{HostException, try_host};
use crate::service::ServicePtr;
use crate::testing::FakeFileInfo;

/// A `pfc::list_base_t<input_live_info_t>`. Like pfc's own lists, it copies the items it is
/// given and holds a reference to each payload until it is dropped.
#[repr(C)]
pub struct FakeLiveInfoList {
	vtbl: *const ListBaseVtbl<InputLiveInfo>,
	items: RefCell<Vec<InputLiveInfo>>,
}

impl FakeLiveInfoList {
	#[allow(clippy::new_ret_no_self)]
	pub fn new() -> Box<Self> {
		Box::new(Self { vtbl: &raw const LIST_VTBL, items: RefCell::new(Vec::new()) })
	}

	pub fn as_raw(&self) -> *mut ListBase<InputLiveInfo> {
		std::ptr::from_ref(self).cast_mut().cast()
	}

	/// The scope of each item, in order.
	pub fn scopes(&self) -> Vec<u32> {
		self.items.borrow().iter().map(|item| item.scope).collect()
	}

	/// Applies item `index`'s `file_info_filter` to `info`, as foobar2000 does to the info it
	/// shows, with a null track and unknown file stats.
	pub fn apply(&self, index: usize, info: &mut FakeFileInfo) -> Result<bool, HostException> {
		let payload = self.items.borrow()[index].payload;
		let vtbl = unsafe { &*(*payload).vtbl.cast::<FileInfoFilterVtbl>() };
		let mut location: *mut ServiceBase = std::ptr::null_mut();
		let stats = FileStats::INVALID;
		try_host(|| unsafe {
			(vtbl.apply_filter)(payload, &raw mut location, &raw const stats, info.as_raw())
		})
	}
}

impl Drop for FakeLiveInfoList {
	fn drop(&mut self) {
		for item in self.items.get_mut().drain(..) {
			drop(unsafe { ServicePtr::from_raw_owned(item.payload) });
		}
	}
}

unsafe fn of<'a>(this: *const ListBase<InputLiveInfo>) -> &'a FakeLiveInfoList {
	unsafe { &*this.cast::<FakeLiveInfoList>() }
}

unsafe extern "C-unwind" fn get_count(this: *const ListBase<InputLiveInfo>) -> usize {
	unsafe { of(this) }.items.borrow().len()
}

unsafe extern "C-unwind" fn get_item_ex(
	this: *const ListBase<InputLiveInfo>,
	out: *mut InputLiveInfo,
	n: usize,
) {
	unsafe { (*out).assign_from(&of(this).items.borrow()[n]) };
}

unsafe extern "C-unwind" fn insert_items(
	this: *mut ListBase<InputLiveInfo>,
	items: *const ListBaseConst<InputLiveInfo>,
	base: usize,
) -> usize {
	let list = unsafe { of(this) };
	let source = unsafe { &*(*items).vtbl };
	let count = unsafe { (source.get_count)(items) };
	let mut list_items = list.items.borrow_mut();
	let at = base.min(list_items.len());
	for n in 0..count {
		let mut copy = InputLiveInfo::default();
		unsafe { (source.get_item_ex)(items, &raw mut copy, n) };
		list_items.insert(at + n, copy);
	}
	at
}

unsafe extern "C-unwind" fn filter_mask(
	_this: *mut ListBase<InputLiveInfo>,
	_mask: *const std::ffi::c_void,
) {
	unimplemented!("poll_live_info only adds items");
}

unsafe extern "C-unwind" fn reorder_partial(
	_this: *mut ListBase<InputLiveInfo>,
	_base: usize,
	_order: *const usize,
	_count: usize,
) {
	unimplemented!("poll_live_info only adds items");
}

unsafe extern "C-unwind" fn sort(
	_this: *mut ListBase<InputLiveInfo>,
	_callback: *mut std::ffi::c_void,
) {
	unimplemented!("poll_live_info only adds items");
}

unsafe extern "C-unwind" fn replace_item(
	_this: *mut ListBase<InputLiveInfo>,
	_index: usize,
	_item: *const InputLiveInfo,
) {
	unimplemented!("poll_live_info only adds items");
}

unsafe extern "C-unwind" fn swap_item_with(
	_this: *mut ListBase<InputLiveInfo>,
	_index: usize,
	_item: *mut InputLiveInfo,
) {
	unimplemented!("poll_live_info only adds items");
}

unsafe extern "C-unwind" fn swap_items(
	_this: *mut ListBase<InputLiveInfo>,
	_first: usize,
	_second: usize,
) {
	unimplemented!("poll_live_info only adds items");
}

static LIST_VTBL: ListBaseVtbl<InputLiveInfo> = ListBaseVtbl {
	get_count,
	get_item_ex,
	filter_mask,
	insert_items,
	reorder_partial,
	sort,
	sort_stable: sort,
	replace_item,
	swap_item_with,
	swap_items,
};
