//! Driving an Advanced preferences entry through its vtables, as foobar2000's Advanced page does.

use super::FakeString;
use crate::abi::advconfig::{CheckboxVtbl, EntryVtbl};
use crate::cxx::{HostException, try_host};
use crate::guid::Guid;
use crate::service::ServicePtr;

/// An `advconfig_entry` service, called through its vtable. The checkbox methods are for
/// `advconfig_entry_checkbox_v2` services only.
pub struct AdvconfigHandle(ServicePtr);

impl AdvconfigHandle {
	pub const fn new(service: ServicePtr) -> Self {
		Self(service)
	}

	fn entry(&self) -> &EntryVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<EntryVtbl>() }
	}

	fn checkbox(&self) -> &CheckboxVtbl {
		unsafe { &*(*self.0.as_raw()).vtbl.cast::<CheckboxVtbl>() }
	}

	pub fn answers(&self, iid: &Guid) -> bool {
		self.0.query(iid).is_some()
	}

	pub fn name(&self) -> String {
		let mut name = FakeString::new();
		try_host(|| unsafe { (self.entry().get_name)(self.0.as_raw(), name.as_raw()) })
			.expect("get_name threw");
		name.value()
	}

	pub fn guid(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.entry().get_guid)(self.0.as_raw(), &raw mut guid) };
		guid
	}

	pub fn parent(&self) -> Guid {
		let mut guid = Guid::from_u128(0);
		unsafe { (self.entry().get_parent)(self.0.as_raw(), &raw mut guid) };
		guid
	}

	pub fn sort_priority(&self) -> f64 {
		unsafe { (self.entry().get_sort_priority)(self.0.as_raw()) }
	}

	pub fn reset(&self) {
		try_host(|| unsafe { (self.entry().reset)(self.0.as_raw()) }).expect("reset threw");
	}

	pub fn state(&self) -> bool {
		try_host(|| unsafe { (self.checkbox().get_state)(self.0.as_raw()) })
			.expect("get_state threw")
	}

	pub fn set_state(&self, state: bool) -> Result<(), HostException> {
		try_host(|| unsafe { (self.checkbox().set_state)(self.0.as_raw(), state) })
	}

	pub fn is_radio(&self) -> bool {
		unsafe { (self.checkbox().is_radio)(self.0.as_raw()) }
	}

	pub fn default_state(&self) -> bool {
		unsafe { (self.checkbox().get_default_state)(self.0.as_raw()) }
	}

	pub fn preferences_flags(&self) -> u32 {
		unsafe { (self.checkbox().get_preferences_flags)(self.0.as_raw()) }
	}
}
