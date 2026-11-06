//! Adding branches and checkboxes to foobar2000's Advanced preferences.

use std::marker::PhantomData;

use crate::abi::advconfig::{
	BRANCH_IID, CHECKBOX_IID, CHECKBOX_V2_IID, CheckboxVtbl, ENTRY_IID, EntryVtbl,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::boundary::guard;
use crate::component::Factory;
use crate::error::Result;
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServicePtr, new_service};
use crate::strings::set_string;

/// A branch in Advanced preferences, holding other entries.
pub trait AdvconfigBranch: 'static {
	const GUID: Guid;
	/// The branch it sits in, such as `abi::advconfig::GUID_BRANCH_TOOLS`.
	const PARENT: Guid;
	const NAME: &'static str;
	/// Where it sorts among its parent's entries: lower comes first.
	const SORT_PRIORITY: f64 = 0.0;
}

/// A checkbox in Advanced preferences. foobar2000 calls `set_state` when the user applies a change.
pub trait AdvconfigCheckbox: 'static {
	const GUID: Guid;
	/// The branch it sits in.
	const PARENT: Guid;
	const NAME: &'static str;
	/// Where it sorts among its parent's entries: lower comes first.
	const SORT_PRIORITY: f64 = 0.0;
	/// The state Reset puts back.
	const DEFAULT: bool;
	fn state() -> bool;
	fn set_state(state: bool) -> Result<()>;
}

/// The factory that registers branch `T` with foobar2000.
pub const fn advconfig_branch_factory<T: AdvconfigBranch>() -> Factory {
	Factory::per_instance(&ENTRY_IID, advconfig_branch_service::<T>)
}

/// A new `advconfig_branch` service for `T`.
pub fn advconfig_branch_service<T: AdvconfigBranch>() -> ServicePtr {
	new_service(Branch::<T>(PhantomData))
}

/// The factory that registers checkbox `T` with foobar2000.
pub const fn advconfig_checkbox_factory<T: AdvconfigCheckbox>() -> Factory {
	Factory::per_instance(&ENTRY_IID, advconfig_checkbox_service::<T>)
}

/// A new `advconfig_entry_checkbox_v2` service for `T`.
pub fn advconfig_checkbox_service<T: AdvconfigCheckbox>() -> ServicePtr {
	new_service(Checkbox::<T>(PhantomData))
}

struct Branch<T>(PhantomData<fn() -> T>);

unsafe impl<T: AdvconfigBranch> ServiceImpl for Branch<T> {
	type Vtbl = EntryVtbl;
	const IIDS: &'static [Guid] = &[BRANCH_IID, ENTRY_IID];
	const VTBL: &'static EntryVtbl = &EntryVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_name: get_name::<BranchEntry<T>>,
		get_guid: guid::<BranchEntry<T>>,
		get_parent: parent::<BranchEntry<T>>,
		reset: branch_reset,
		get_sort_priority: sort_priority::<BranchEntry<T>>,
	};
}

/// The constants every entry has.
trait Entry {
	const GUID: Guid;
	const PARENT: Guid;
	const NAME: &'static str;
	const SORT_PRIORITY: f64;
}

struct BranchEntry<T>(PhantomData<fn() -> T>);

impl<T: AdvconfigBranch> Entry for BranchEntry<T> {
	const GUID: Guid = T::GUID;
	const PARENT: Guid = T::PARENT;
	const NAME: &'static str = T::NAME;
	const SORT_PRIORITY: f64 = T::SORT_PRIORITY;
}

struct CheckboxEntry<T>(PhantomData<fn() -> T>);

impl<T: AdvconfigCheckbox> Entry for CheckboxEntry<T> {
	const GUID: Guid = T::GUID;
	const PARENT: Guid = T::PARENT;
	const NAME: &'static str = T::NAME;
	const SORT_PRIORITY: f64 = T::SORT_PRIORITY;
}

struct Checkbox<T>(PhantomData<fn() -> T>);

unsafe impl<T: AdvconfigCheckbox> ServiceImpl for Checkbox<T> {
	type Vtbl = CheckboxVtbl;
	const IIDS: &'static [Guid] = &[CHECKBOX_V2_IID, CHECKBOX_IID, ENTRY_IID];
	const VTBL: &'static CheckboxVtbl = &CheckboxVtbl {
		entry: EntryVtbl {
			base: ServiceBaseVtbl::new::<Self>(),
			get_name: get_name::<CheckboxEntry<T>>,
			get_guid: guid::<CheckboxEntry<T>>,
			get_parent: parent::<CheckboxEntry<T>>,
			reset: checkbox_reset::<T>,
			get_sort_priority: sort_priority::<CheckboxEntry<T>>,
		},
		get_state: get_state::<T>,
		set_state: set_state::<T>,
		is_radio,
		get_default_state: default_state::<T>,
		get_preferences_flags: preferences_flags,
	};
}

unsafe extern "C-unwind" fn get_name<T: Entry>(_this: *mut ServiceBase, out: *mut StringBase) {
	unsafe { guard(|| set_string(out, T::NAME)) }
}

unsafe extern "C-unwind" fn guid<T: Entry>(_this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid {
	unsafe { ret.write(T::GUID) };
	ret
}

unsafe extern "C-unwind" fn parent<T: Entry>(_this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid {
	unsafe { ret.write(T::PARENT) };
	ret
}

unsafe extern "C-unwind" fn sort_priority<T: Entry>(_this: *mut ServiceBase) -> f64 {
	T::SORT_PRIORITY
}

unsafe extern "C-unwind" fn branch_reset(_this: *mut ServiceBase) {}

unsafe extern "C-unwind" fn checkbox_reset<T: AdvconfigCheckbox>(_this: *mut ServiceBase) {
	unsafe { guard(|| T::set_state(T::DEFAULT)) }
}

unsafe extern "C-unwind" fn get_state<T: AdvconfigCheckbox>(_this: *mut ServiceBase) -> bool {
	unsafe { guard(|| Ok(T::state())) }
}

unsafe extern "C-unwind" fn set_state<T: AdvconfigCheckbox>(_this: *mut ServiceBase, state: bool) {
	unsafe { guard(|| T::set_state(state)) }
}

unsafe extern "C-unwind" fn is_radio(_this: *mut ServiceBase) -> bool {
	false
}

unsafe extern "C-unwind" fn default_state<T: AdvconfigCheckbox>(_this: *mut ServiceBase) -> bool {
	T::DEFAULT
}

unsafe extern "C-unwind" fn preferences_flags(_this: *mut ServiceBase) -> u32 {
	0
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicBool, Ordering};

	use super::{
		AdvconfigBranch, AdvconfigCheckbox, advconfig_branch_service, advconfig_checkbox_service,
	};
	use crate::abi::advconfig::{
		BRANCH_IID, CHECKBOX_IID, CHECKBOX_V2_IID, ENTRY_IID, GUID_BRANCH_TOOLS,
	};
	use crate::cxx::ExceptionKind;
	use crate::error::{Error, Result};
	use crate::guid::Guid;
	use crate::testing::AdvconfigHandle;

	static STATE: AtomicBool = AtomicBool::new(false);

	struct TestBranch;

	impl AdvconfigBranch for TestBranch {
		const GUID: Guid = Guid::from_u128(0x6f1c2a77_3b8e_4d0a_9c51_2e7f04b8d913);
		const PARENT: Guid = GUID_BRANCH_TOOLS;
		const NAME: &'static str = "Spotify";
		const SORT_PRIORITY: f64 = 2.5;
	}

	struct TestCheckbox;

	impl AdvconfigCheckbox for TestCheckbox {
		const GUID: Guid = Guid::from_u128(0x0d4e8b62_95a1_4f3c_b7e0_6c2d19f4a758);
		const PARENT: Guid = TestBranch::GUID;
		const NAME: &'static str = "Log details";
		const DEFAULT: bool = false;

		fn state() -> bool {
			STATE.load(Ordering::SeqCst)
		}

		fn set_state(state: bool) -> Result<()> {
			STATE.store(state, Ordering::SeqCst);
			Ok(())
		}
	}

	struct FailingCheckbox;

	impl AdvconfigCheckbox for FailingCheckbox {
		const GUID: Guid = Guid::from_u128(0x0d4e8b62_95a1_4f3c_b7e0_6c2d19f4a759);
		const PARENT: Guid = TestBranch::GUID;
		const NAME: &'static str = "Can't store";
		const DEFAULT: bool = false;

		fn state() -> bool {
			false
		}

		fn set_state(_state: bool) -> Result<()> {
			Err(Error::new(ExceptionKind::Io, "can't store"))
		}
	}

	fn branch() -> AdvconfigHandle {
		AdvconfigHandle::new(advconfig_branch_service::<TestBranch>())
	}

	fn checkbox() -> AdvconfigHandle {
		AdvconfigHandle::new(advconfig_checkbox_service::<TestCheckbox>())
	}

	#[test]
	fn a_branch_answers_as_branch_and_entry_only() {
		let branch = branch();
		assert!(branch.answers(&BRANCH_IID));
		assert!(branch.answers(&ENTRY_IID));
		assert!(!branch.answers(&CHECKBOX_IID));
		assert!(!branch.answers(&CHECKBOX_V2_IID));
	}

	#[test]
	fn a_checkbox_answers_both_checkbox_levels_and_entry_but_not_branch() {
		let checkbox = checkbox();
		for iid in [CHECKBOX_V2_IID, CHECKBOX_IID, ENTRY_IID] {
			assert!(checkbox.answers(&iid), "{iid:?}");
		}
		assert!(!checkbox.answers(&BRANCH_IID));
	}

	#[test]
	fn a_branch_takes_name_guid_parent_and_priority_from_the_trait() {
		let branch = branch();
		assert_eq!(branch.name(), "Spotify");
		assert_eq!(branch.guid(), TestBranch::GUID);
		assert_eq!(branch.parent(), GUID_BRANCH_TOOLS);
		assert!((branch.sort_priority() - 2.5).abs() < f64::EPSILON);
	}

	#[test]
	fn a_checkbox_takes_name_guid_parent_and_default_from_the_trait() {
		let checkbox = checkbox();
		assert_eq!(checkbox.name(), "Log details");
		assert_eq!(checkbox.guid(), TestCheckbox::GUID);
		assert_eq!(checkbox.parent(), TestBranch::GUID);
		assert!(checkbox.sort_priority().abs() < f64::EPSILON);
		assert!(!checkbox.default_state());
		assert!(!checkbox.is_radio());
		assert_eq!(checkbox.preferences_flags(), 0);
	}

	#[test]
	fn a_checkbox_reads_and_writes_its_state_through_the_trait() {
		let checkbox = checkbox();
		assert!(checkbox.set_state(true).is_ok());
		assert!(checkbox.state());
		assert!(STATE.load(Ordering::SeqCst));
	}

	#[test]
	fn resetting_a_checkbox_restores_its_default() {
		let checkbox = checkbox();
		checkbox.set_state(true).unwrap();
		checkbox.reset();
		assert!(!checkbox.state());
	}

	#[test]
	fn a_checkbox_that_cannot_store_its_state_throws() {
		let checkbox = AdvconfigHandle::new(advconfig_checkbox_service::<FailingCheckbox>());
		let error = checkbox.set_state(true).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(error.message, "can't store");
	}
}
