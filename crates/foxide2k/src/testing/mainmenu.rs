//! Driving a main-menu popup or command through its vtable, as foobar2000's menu does.

use super::FakeString;
use crate::abi::input::ByValueServicePtr;
use crate::abi::mainmenu::{COMMANDS_IID, CommandsVtbl, GroupPopupVtbl};
use crate::cxx::{HostException, try_host};
use crate::guid::Guid;
use crate::service::ServicePtr;

/// A `mainmenu_group_popup` or `mainmenu_commands` service, called through its vtable. `guid` and
/// `name` are for popups; the methods with an index are for commands.
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
		let mut callback = callback.map_or(std::ptr::null_mut(), ServicePtr::into_raw);
		try_host(|| unsafe {
			(self.commands().execute)(
				self.0.as_raw(),
				index,
				ByValueServicePtr::for_callee(&raw mut callback),
			);
		})
	}
}
