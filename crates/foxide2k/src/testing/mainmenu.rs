//! Driving a main-menu popup or command, and the nodes a dynamic command builds, through their
//! vtables, as foobar2000's menu does.

use super::FakeString;
use crate::abi::input::ByValueServicePtr;
use crate::abi::mainmenu::{COMMANDS_IID, CommandsV2Vtbl, CommandsVtbl, GroupPopupVtbl, NodeVtbl};
use crate::abi::service::ServiceBase;
use crate::cxx::{HostException, try_host};
use crate::guid::Guid;
use crate::service::ServicePtr;

/// `callback` as a `service_ptr_t` temporary, which the callee destroys.
fn by_value(callback: Option<ServicePtr>) -> *mut ServiceBase {
	callback.map_or(std::ptr::null_mut(), ServicePtr::into_raw)
}

/// A `mainmenu_group_popup` or `mainmenu_commands` service, called through its vtable. `guid` and
/// `name` are for popups; the methods with an index are for commands, and `is_dynamic`,
/// `instantiate` and `dynamic_execute` for `mainmenu_commands_v2`.
pub struct MainMenuHandle(ServicePtr);

impl MainMenuHandle {
	pub const fn new(service: ServicePtr) -> Self {
		Self(service)
	}

	fn popup(&self) -> &GroupPopupVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<GroupPopupVtbl>() }
	}

	fn commands(&self) -> &CommandsVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<CommandsVtbl>() }
	}

	fn commands_v2(&self) -> &CommandsV2Vtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<CommandsV2Vtbl>() }
	}

	pub fn answers(&self, iid: &Guid) -> bool {
		self.0.query(iid).is_some()
	}

	pub fn guid(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.popup().get_guid)(self.0.as_raw(), &raw mut guid) };
		guid
	}

	pub fn parent(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		if self.answers(&COMMANDS_IID) {
			unsafe { (self.commands().get_parent)(self.0.as_raw(), &raw mut guid) };
		} else {
			unsafe { (self.popup().get_parent)(self.0.as_raw(), &raw mut guid) };
		}
		guid
	}

	pub fn sort_priority(&self) -> u32 {
		if self.answers(&COMMANDS_IID) {
			unsafe { (self.commands().get_sort_priority)(self.0.as_raw()) }
		} else {
			unsafe { (self.popup().get_sort_priority)(self.0.as_raw()) }
		}
	}

	pub fn name(&self) -> String {
		let mut name = FakeString::new();
		try_host(|| unsafe { (self.popup().get_display_string)(self.0.as_raw(), name.as_raw()) })
			.expect("get_display_string threw");
		name.value()
	}

	pub fn command_count(&self) -> u32 {
		unsafe { (self.commands().get_command_count)(self.0.as_raw()) }
	}

	pub fn command(&self, index: u32) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.commands().get_command)(self.0.as_raw(), &raw mut guid, index) };
		guid
	}

	pub fn command_name(&self, index: u32) -> String {
		let mut name = FakeString::new();
		try_host(|| unsafe { (self.commands().get_name)(self.0.as_raw(), index, name.as_raw()) })
			.expect("get_name threw");
		name.value()
	}

	pub fn description(&self, index: u32) -> Option<String> {
		let mut text = FakeString::new();
		let found = try_host(|| unsafe {
			(self.commands().get_description)(self.0.as_raw(), index, text.as_raw())
		})
		.expect("get_description threw");
		found.then(|| text.value())
	}

	/// The text and flags `get_display` gives, or `None` when the item is hidden.
	pub fn display(&self, index: u32) -> Option<(String, u32)> {
		let mut text = FakeString::new();
		let mut flags = u32::MAX;
		let shown = try_host(|| unsafe {
			(self.commands().get_display)(self.0.as_raw(), index, text.as_raw(), &raw mut flags)
		})
		.expect("get_display threw");
		shown.then(|| (text.value(), flags))
	}

	/// `execute`, passing `callback` by value: the command must release it.
	pub fn execute(&self, index: u32, callback: Option<ServicePtr>) -> Result<(), HostException> {
		let mut callback = by_value(callback);
		try_host(|| unsafe {
			(self.commands().execute)(
				self.0.as_raw(),
				index,
				ByValueServicePtr::for_callee(&raw mut callback),
			);
		})
	}

	pub fn is_dynamic(&self, index: u32) -> bool {
		unsafe { (self.commands_v2().is_command_dynamic)(self.0.as_raw(), index) }
	}

	/// The root node `dynamic_instantiate` builds, as foobar2000 asks for it when the menu opens.
	pub fn instantiate(&self, index: u32) -> MainMenuNodeHandle {
		let mut node: *mut ServiceBase = std::ptr::null_mut();
		try_host(|| unsafe {
			(self.commands_v2().dynamic_instantiate)(self.0.as_raw(), &raw mut node, index);
		})
		.expect("dynamic_instantiate threw");
		MainMenuNodeHandle(unsafe { ServicePtr::from_raw_owned(node) }.expect("no node"))
	}

	/// `dynamic_execute` for the command whose GUID is `sub`, as a shortcut bound to it runs it,
	/// with a null callback. Returns whether a command had that GUID.
	pub fn dynamic_execute(&self, index: u32, sub: &Guid) -> Result<bool, HostException> {
		let mut callback: *mut ServiceBase = std::ptr::null_mut();
		try_host(|| unsafe {
			(self.commands_v2().dynamic_execute)(
				self.0.as_raw(),
				index,
				sub,
				ByValueServicePtr::for_callee(&raw mut callback),
			)
		})
	}
}

/// A `mainmenu_node` service, called through its vtable. Each handle owns one reference.
pub struct MainMenuNodeHandle(ServicePtr);

impl MainMenuNodeHandle {
	fn vtbl(&self) -> &NodeVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<NodeVtbl>() }
	}

	pub fn answers(&self, iid: &Guid) -> bool {
		self.0.query(iid).is_some()
	}

	/// `get_type`: `abi::mainmenu::TYPE_GROUP` or `TYPE_COMMAND`.
	pub fn kind(&self) -> u32 {
		unsafe { (self.vtbl().get_type)(self.0.as_raw()) }
	}

	/// The text and flags `get_display` gives.
	pub fn display(&self) -> (String, u32) {
		let mut text = FakeString::new();
		let mut flags = u32::MAX;
		try_host(|| unsafe {
			(self.vtbl().get_display)(self.0.as_raw(), text.as_raw(), &raw mut flags);
		})
		.expect("get_display threw");
		(text.value(), flags)
	}

	/// `get_child`, which hands over one reference.
	pub fn child(&self, index: usize) -> Result<Self, HostException> {
		let mut child: *mut ServiceBase = std::ptr::null_mut();
		try_host(|| unsafe {
			(self.vtbl().get_child)(self.0.as_raw(), &raw mut child, index);
		})?;
		Ok(Self(unsafe { ServicePtr::from_raw_owned(child) }.expect("get_child gave null")))
	}

	pub fn children(&self) -> Vec<Self> {
		let count = unsafe { (self.vtbl().get_children_count)(self.0.as_raw()) };
		(0..count).map(|index| self.child(index).expect("get_child threw")).collect()
	}

	/// The GUID `get_guid` gives. It starts from a non-null GUID, so a slot that writes nothing
	/// doesn't pass for null.
	pub fn guid(&self) -> Guid {
		let mut guid = Guid::from_u128(1);
		unsafe { (self.vtbl().get_guid)(self.0.as_raw(), &raw mut guid) };
		guid
	}

	/// The text `get_description` gives, or `None` when the node has none.
	pub fn description(&self) -> Option<String> {
		let mut text = FakeString::new();
		let found =
			try_host(|| unsafe { (self.vtbl().get_description)(self.0.as_raw(), text.as_raw()) })
				.expect("get_description threw");
		found.then(|| text.value())
	}

	/// `execute`, passing `callback` by value: the node must release it.
	pub fn execute(&self, callback: Option<ServicePtr>) -> Result<(), HostException> {
		let mut callback = by_value(callback);
		try_host(|| unsafe {
			(self.vtbl().execute)(
				self.0.as_raw(),
				ByValueServicePtr::for_callee(&raw mut callback),
			);
		})
	}
}
