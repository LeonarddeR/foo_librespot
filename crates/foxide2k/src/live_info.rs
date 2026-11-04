//! Technical info that changes while a track plays, such as the bitrate, handed to foobar2000
//! 2.25 and later through `input_decoder_v5::poll_live_info`. Each change is a
//! `file_info_filter` that foobar2000 applies to the info it shows.

use crate::abi::file::FileStats;
use crate::abi::file_info::FileInfo as RawFileInfo;
use crate::abi::input::ByValueServicePtr;
use crate::abi::live_info::{FILE_INFO_FILTER_IID, FileInfoFilterVtbl, InputLiveInfo, SCOPE_INFO};
use crate::abi::pfc_list::{ListBase, ListBaseConst, ListBaseConstVtbl};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::boundary::guard;
use crate::cxx::try_host;
use crate::error::Result;
use crate::file_info::FileInfo;
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServiceObject, new_service};

#[cfg(test)]
static FILTERS_ALIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many filters exist, for tests of their reference counting.
#[cfg(test)]
pub fn filters_alive() -> usize {
	FILTERS_ALIVE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Writes technical info fields into the info foobar2000 shows.
struct InfoFilter {
	fields: Vec<(String, String)>,
}

impl InfoFilter {
	fn new(fields: Vec<(String, String)>) -> Self {
		#[cfg(test)]
		FILTERS_ALIVE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
		Self { fields }
	}
}

#[cfg(test)]
impl Drop for InfoFilter {
	fn drop(&mut self) {
		FILTERS_ALIVE.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
	}
}

unsafe impl ServiceImpl for InfoFilter {
	type Vtbl = FileInfoFilterVtbl;
	const IIDS: &'static [Guid] = &[FILE_INFO_FILTER_IID];
	const VTBL: &'static FileInfoFilterVtbl =
		&FileInfoFilterVtbl { base: ServiceBaseVtbl::new::<Self>(), apply_filter };
}

unsafe extern "C-unwind" fn apply_filter(
	this: *mut ServiceBase,
	location: ByValueServicePtr,
	_stats: *const FileStats,
	info: *mut RawFileInfo,
) -> bool {
	unsafe {
		// Released first, so a failure below can't leak the caller's track.
		drop(location);
		guard(|| {
			let mut info = FileInfo::from_raw(info);
			for (name, value) in &ServiceObject::<InfoFilter>::value(this).fields {
				info.info_set(name, value)?;
			}
			Ok(true)
		})
	}
}

/// One item, offered to `list_base_t::insert_items` as a `list_base_const_t`.
#[repr(C)]
struct OneItem {
	vtbl: *const ListBaseConstVtbl<InputLiveInfo>,
	item: InputLiveInfo,
}

static ONE_ITEM_VTBL: ListBaseConstVtbl<InputLiveInfo> =
	ListBaseConstVtbl { get_count: one_count, get_item_ex: one_item };

unsafe extern "C-unwind" fn one_count(_this: *const ListBaseConst<InputLiveInfo>) -> usize {
	1
}

/// Copy assignment, as `input_live_info_t`'s is: `out` takes a reference to the payload and
/// releases the one it held.
unsafe extern "C-unwind" fn one_item(
	this: *const ListBaseConst<InputLiveInfo>,
	out: *mut InputLiveInfo,
	_n: usize,
) {
	unsafe {
		guard(|| {
			(*out).assign_from(&(*this.cast::<OneItem>()).item);
			Ok(())
		});
	}
}

/// Adds a `scope_info` change to `list` that writes `fields`, shown from the start of the last
/// chunk.
///
/// # Safety
/// `list` must be the live `list_base_t<input_live_info_t>` foobar2000 passed to
/// `poll_live_info`.
pub(crate) unsafe fn add_info_change(
	list: *mut ListBase<InputLiveInfo>,
	fields: Vec<(String, String)>,
) -> Result<()> {
	let filter = new_service(InfoFilter::new(fields));
	let one = OneItem {
		vtbl: &raw const ONE_ITEM_VTBL,
		item: InputLiveInfo { payload: filter.as_raw(), scope: SCOPE_INFO, delta_time: 0.0 },
	};
	let source = std::ptr::from_ref(&one).cast::<ListBaseConst<InputLiveInfo>>();
	// The list keeps its own reference; `filter` drops ours.
	try_host(|| unsafe { ((*(*list).vtbl).insert_items)(list, source, usize::MAX) })?;
	Ok(())
}
