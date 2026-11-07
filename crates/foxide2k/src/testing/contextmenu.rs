//! Driving a right-click menu item and its nodes through their vtables, as foobar2000's menu
//! does, and a list of tracks to pass them.

use std::ffi::CString;
use std::marker::PhantomData;

use super::FakeString;
use crate::abi::contextmenu::{CALLER_KEYBOARD_SHORTCUT_LIST, ItemVtbl, Node};
use crate::abi::metadb_handle::{HandleListRaw, MetadbHandleVtbl};
use crate::abi::pfc_list::{ListBaseConst, ListBaseConstVtbl};
use crate::abi::playlist_loader::PlayableLocation;
use crate::abi::service::ServiceBase;
use crate::cxx::{HostException, try_host};
use crate::guid::Guid;
use crate::playlist_loader::LOCATION_VTBL;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};

static CALLER: Guid = CALLER_KEYBOARD_SHORTCUT_LIST;

/// A `metadb_handle` that knows only its location.
struct LocationHandle {
	location: PlayableLocation,
	_path: CString,
}

// The location points into `_path`, which the handle owns and never changes.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for LocationHandle {}
unsafe impl Sync for LocationHandle {}

unsafe impl ServiceImpl for LocationHandle {
	type Vtbl = MetadbHandleVtbl;
	const IIDS: &'static [Guid] = &[];
	const VTBL: &'static MetadbHandleVtbl = &MetadbHandleVtbl {
		base: crate::abi::service::ServiceBaseVtbl::new::<Self>(),
		get_location: handle_location,
	};
}

unsafe extern "C-unwind" fn handle_location(this: *mut ServiceBase) -> *const PlayableLocation {
	&raw const unsafe { ServiceObject::<LocationHandle>::value(this) }.location
}

/// A `metadb_handle_list_cref` of fake handles.
#[repr(C)]
pub struct FakeHandleList {
	vtbl: *const ListBaseConstVtbl<*mut ServiceBase>,
	items: Vec<ServicePtr>,
}

impl FakeHandleList {
	#[allow(clippy::new_ret_no_self)]
	pub fn new(paths: &[&str]) -> Box<Self> {
		let items = paths
			.iter()
			.map(|path| {
				let path = CString::new(*path).expect("no NUL in a path");
				let location = PlayableLocation {
					vtbl: &raw const LOCATION_VTBL,
					path: path.as_ptr(),
					subsong: 0,
				};
				new_service(LocationHandle { location, _path: path })
			})
			.collect();
		Box::new(Self { vtbl: &raw const LIST_VTBL, items })
	}

	pub fn as_raw(&self) -> *const HandleListRaw {
		std::ptr::from_ref(self).cast()
	}
}

static LIST_VTBL: ListBaseConstVtbl<*mut ServiceBase> =
	ListBaseConstVtbl { get_count: list_count, get_item_ex: list_item };

unsafe extern "C-unwind" fn list_count(this: *const ListBaseConst<*mut ServiceBase>) -> usize {
	unsafe { &*this.cast::<FakeHandleList>() }.items.len()
}

/// Copy assignment, as `service_ptr_t`'s is: `out` takes a reference and releases the one it held.
unsafe extern "C-unwind" fn list_item(
	this: *const ListBaseConst<*mut ServiceBase>,
	out: *mut *mut ServiceBase,
	n: usize,
) {
	unsafe {
		let item = (&*this.cast::<FakeHandleList>()).items[n].clone().into_raw();
		drop(ServicePtr::from_raw_owned(*out));
		*out = item;
	}
}

/// A `contextmenu_item_v2` service, called through its vtable.
pub struct ContextMenuHandle(ServicePtr);

impl ContextMenuHandle {
	pub const fn new(service: ServicePtr) -> Self {
		Self(service)
	}

	fn vtbl(&self) -> &ItemVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<ItemVtbl>() }
	}

	pub fn answers(&self, iid: &Guid) -> bool {
		self.0.query(iid).is_some()
	}

	pub fn item_count(&self) -> u32 {
		unsafe { (self.vtbl().get_num_items)(self.0.as_raw()) }
	}

	pub fn guid(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.vtbl().get_item_guid)(self.0.as_raw(), &raw mut guid, 0) };
		guid
	}

	pub fn name(&self) -> String {
		let mut name = FakeString::new();
		try_host(|| unsafe { (self.vtbl().get_item_name)(self.0.as_raw(), 0, name.as_raw()) })
			.expect("get_item_name threw");
		name.value()
	}

	pub fn default_path(&self) -> String {
		let mut path = FakeString::new();
		try_host(|| unsafe {
			(self.vtbl().get_item_default_path)(self.0.as_raw(), 0, path.as_raw());
		})
		.expect("get_item_default_path threw");
		path.value()
	}

	pub fn description(&self) -> Option<String> {
		let mut text = FakeString::new();
		let found = try_host(|| unsafe {
			(self.vtbl().get_item_description)(self.0.as_raw(), 0, text.as_raw())
		})
		.expect("get_item_description threw");
		found.then(|| text.value())
	}

	pub fn parent(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.vtbl().get_parent)(self.0.as_raw(), &raw mut guid) };
		guid
	}

	pub fn sort_priority(&self) -> f64 {
		unsafe { (self.vtbl().get_sort_priority)(self.0.as_raw()) }
	}

	pub fn enabled_state(&self) -> i32 {
		unsafe { (self.vtbl().get_enabled_state)(self.0.as_raw(), 0) }
	}

	/// `instantiate_item` for the keyboard shortcut list's caller.
	pub fn instantiate(&self, data: &FakeHandleList) -> NodeHandle {
		let root = try_host(|| unsafe {
			(self.vtbl().instantiate_item)(self.0.as_raw(), 0, data.as_raw(), &raw const CALLER)
		})
		.expect("instantiate_item threw");
		assert!(!root.is_null(), "instantiate_item returned no node");
		NodeHandle { root }
	}

	/// `item_execute_simple` for `node`, with no tracks.
	pub fn execute_simple(&self, node: &Guid) {
		let data = FakeHandleList::new(&[]);
		try_host(|| unsafe {
			(self.vtbl().item_execute_simple)(
				self.0.as_raw(),
				0,
				node,
				data.as_raw(),
				&raw const CALLER,
			);
		})
		.expect("item_execute_simple threw");
	}
}

/// The root node `instantiate_item` returned. Dropping it calls its destructor, as foobar2000
/// does when the menu closes.
pub struct NodeHandle {
	root: *mut Node,
}

impl NodeHandle {
	pub const fn node(&self) -> NodeRef<'_> {
		NodeRef { raw: self.root, _borrow: PhantomData }
	}
}

impl Drop for NodeHandle {
	fn drop(&mut self) {
		let root = self.root;
		try_host(|| unsafe { ((*(*root).vtbl).delete)(root, 1) }).expect("delete threw");
	}
}

/// A node of a tree a [`NodeHandle`] owns.
#[derive(Clone, Copy)]
pub struct NodeRef<'a> {
	raw: *mut Node,
	_borrow: PhantomData<&'a Node>,
}

impl NodeRef<'_> {
	/// The text and flags `get_display_data` gives, or `None` when the node is hidden.
	pub fn display(&self, data: &FakeHandleList) -> Option<(String, u32)> {
		let raw = self.raw;
		let mut text = FakeString::new();
		let mut flags = u32::MAX;
		let shown = try_host(|| unsafe {
			((*(*raw).vtbl).get_display_data)(
				raw,
				text.as_raw(),
				&raw mut flags,
				data.as_raw(),
				&raw const CALLER,
			)
		})
		.expect("get_display_data threw");
		shown.then(|| (text.value(), flags))
	}

	pub fn kind(&self) -> i32 {
		let raw = self.raw;
		unsafe { ((*(*raw).vtbl).get_type)(raw) }
	}

	pub fn children(&self) -> Vec<Self> {
		let raw = self.raw;
		let count = unsafe { ((*(*raw).vtbl).get_children_count)(raw) };
		(0..count)
			.map(|index| NodeRef {
				raw: unsafe { ((*(*raw).vtbl).get_child)(raw, index) },
				_borrow: PhantomData,
			})
			.collect()
	}

	/// The child whose display text is `name`.
	pub fn child(&self, name: &str) -> Option<Self> {
		let data = FakeHandleList::new(&[]);
		self.children()
			.into_iter()
			.find(|child| child.display(&data).is_some_and(|(text, _)| text == name))
	}

	pub fn execute(&self, data: &FakeHandleList) -> Result<(), HostException> {
		let raw = self.raw;
		try_host(|| unsafe {
			((*(*raw).vtbl).execute)(raw, data.as_raw(), &raw const CALLER);
		})
	}
}
