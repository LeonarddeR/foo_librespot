//! Adding popups and commands to foobar2000's main menu.

use std::marker::PhantomData;

use crate::abi::input::ByValueServicePtr;
use crate::abi::mainmenu::{
	COMMANDS_IID, CommandsVtbl, GROUP_IID, GROUP_POPUP_IID, GroupPopupVtbl, SORT_PRIORITY_BASE,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::boundary::guard;
use crate::component::Factory;
use crate::error::Result;
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServicePtr, new_service};
use crate::strings::set_string;

/// A popup submenu, such as File > Spotify.
pub trait MainMenuPopup: 'static {
	const GUID: Guid;
	/// The group it sits in, such as `abi::mainmenu::GROUP_FILE`.
	const PARENT: Guid;
	/// Lower comes first; components use `SORT_PRIORITY_BASE` and up.
	const SORT_PRIORITY: u32;
	const NAME: &'static str;
}

/// One command. foobar2000 calls `execute` on its main thread.
pub trait MainMenuCommand: 'static {
	const GUID: Guid;
	const PARENT: Guid;
	const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE;
	const NAME: &'static str;
	const DESCRIPTION: &'static str;
	fn execute() -> Result<()>;
}

/// The factory that registers popup `T` with foobar2000.
pub const fn mainmenu_popup_factory<T: MainMenuPopup>() -> Factory {
	Factory::per_instance(&GROUP_IID, mainmenu_popup_service::<T>)
}

/// A new `mainmenu_group_popup` service for `T`.
pub fn mainmenu_popup_service<T: MainMenuPopup>() -> ServicePtr {
	new_service(Popup::<T>(PhantomData))
}

/// The factory that registers command `T` with foobar2000.
pub const fn mainmenu_command_factory<T: MainMenuCommand>() -> Factory {
	Factory::per_instance(&COMMANDS_IID, mainmenu_command_service::<T>)
}

/// A new `mainmenu_commands` service holding the single command `T`.
pub fn mainmenu_command_service<T: MainMenuCommand>() -> ServicePtr {
	new_service(Command::<T>(PhantomData))
}

struct Popup<T>(PhantomData<fn() -> T>);

unsafe impl<T: MainMenuPopup> ServiceImpl for Popup<T> {
	type Vtbl = GroupPopupVtbl;
	const IIDS: &'static [Guid] = &[GROUP_POPUP_IID, GROUP_IID];
	const VTBL: &'static GroupPopupVtbl = &GroupPopupVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_guid: popup_guid::<T>,
		get_parent: popup_parent::<T>,
		get_sort_priority: popup_sort_priority::<T>,
		get_display_string: popup_display_string::<T>,
	};
}

unsafe extern "C-unwind" fn popup_guid<T: MainMenuPopup>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::GUID) };
	ret
}

unsafe extern "C-unwind" fn popup_parent<T: MainMenuPopup>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::PARENT) };
	ret
}

unsafe extern "C-unwind" fn popup_sort_priority<T: MainMenuPopup>(_this: *mut ServiceBase) -> u32 {
	T::SORT_PRIORITY
}

unsafe extern "C-unwind" fn popup_display_string<T: MainMenuPopup>(
	_this: *mut ServiceBase,
	out: *mut StringBase,
) {
	unsafe { guard(|| set_string(out, T::NAME)) }
}

struct Command<T>(PhantomData<fn() -> T>);

unsafe impl<T: MainMenuCommand> ServiceImpl for Command<T> {
	type Vtbl = CommandsVtbl;
	const IIDS: &'static [Guid] = &[COMMANDS_IID];
	const VTBL: &'static CommandsVtbl = &CommandsVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_command_count: command_count,
		get_command: command_guid::<T>,
		get_name: command_name::<T>,
		get_description: command_description::<T>,
		get_parent: command_parent::<T>,
		get_sort_priority: command_sort_priority::<T>,
		get_display: command_display::<T>,
		execute: command_execute::<T>,
	};
}

unsafe extern "C-unwind" fn command_count(_this: *mut ServiceBase) -> u32 {
	1
}

unsafe extern "C-unwind" fn command_guid<T: MainMenuCommand>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
	_index: u32,
) -> *mut Guid {
	unsafe { ret.write(T::GUID) };
	ret
}

unsafe extern "C-unwind" fn command_name<T: MainMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	out: *mut StringBase,
) {
	unsafe { guard(|| set_string(out, T::NAME)) }
}

unsafe extern "C-unwind" fn command_description<T: MainMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	out: *mut StringBase,
) -> bool {
	unsafe {
		guard(|| {
			set_string(out, T::DESCRIPTION)?;
			Ok(true)
		})
	}
}

unsafe extern "C-unwind" fn command_parent<T: MainMenuCommand>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::PARENT) };
	ret
}

unsafe extern "C-unwind" fn command_sort_priority<T: MainMenuCommand>(
	_this: *mut ServiceBase,
) -> u32 {
	T::SORT_PRIORITY
}

unsafe extern "C-unwind" fn command_display<T: MainMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	text: *mut StringBase,
	flags: *mut u32,
) -> bool {
	unsafe {
		guard(|| {
			set_string(text, T::NAME)?;
			flags.write(0);
			Ok(true)
		})
	}
}

unsafe extern "C-unwind" fn command_execute<T: MainMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	callback: ByValueServicePtr,
) {
	drop(callback);
	unsafe { guard(T::execute) }
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicUsize, Ordering};

	use super::{MainMenuCommand, MainMenuPopup, mainmenu_command_service, mainmenu_popup_service};
	use crate::abi::mainmenu::{
		COMMANDS_IID, GROUP_FILE, GROUP_IID, GROUP_POPUP_IID, SORT_PRIORITY_BASE,
	};
	use crate::cxx::ExceptionKind;
	use crate::error::{Error, Result};
	use crate::guid::Guid;
	use crate::testing::{CountedService, MainMenuHandle};

	static EXECUTED: AtomicUsize = AtomicUsize::new(0);

	struct TestPopup;

	impl MainMenuPopup for TestPopup {
		const GUID: Guid = Guid::from_u128(0x3c9a5e18_72bd_4f06_a1d4_8b2e6f0c7d35);
		const PARENT: Guid = GROUP_FILE;
		const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE;
		const NAME: &'static str = "Spotify";
	}

	struct TestCommand;

	impl MainMenuCommand for TestCommand {
		const GUID: Guid = Guid::from_u128(0x5b17d2c4_9e83_4a60_b2f1_0d4c8a6e3917);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Open playlist…";
		const DESCRIPTION: &'static str = "Opens one of your Spotify playlists.";

		fn execute() -> Result<()> {
			EXECUTED.fetch_add(1, Ordering::SeqCst);
			Ok(())
		}
	}

	struct FailingCommand;

	impl MainMenuCommand for FailingCommand {
		const GUID: Guid = Guid::from_u128(0x5b17d2c4_9e83_4a60_b2f1_0d4c8a6e3918);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Fail";
		const DESCRIPTION: &'static str = "Always fails.";

		fn execute() -> Result<()> {
			Err(Error::new(ExceptionKind::Io, "can't open"))
		}
	}

	fn popup() -> MainMenuHandle {
		MainMenuHandle::new(mainmenu_popup_service::<TestPopup>())
	}

	fn command() -> MainMenuHandle {
		MainMenuHandle::new(mainmenu_command_service::<TestCommand>())
	}

	#[test]
	fn a_popup_answers_as_popup_and_group() {
		let popup = popup();
		assert!(popup.answers(&GROUP_POPUP_IID));
		assert!(popup.answers(&GROUP_IID));
		assert!(!popup.answers(&COMMANDS_IID));
	}

	#[test]
	fn a_popup_takes_guid_parent_priority_and_name_from_the_trait() {
		let popup = popup();
		assert_eq!(popup.guid(), TestPopup::GUID);
		assert_eq!(popup.parent(), GROUP_FILE);
		assert_eq!(popup.sort_priority(), 0x10000);
		assert_eq!(popup.name(), "Spotify");
	}

	#[test]
	fn a_command_answers_as_mainmenu_commands_only() {
		let command = command();
		assert!(command.answers(&COMMANDS_IID));
		assert!(!command.answers(&GROUP_IID));
		assert!(!command.answers(&GROUP_POPUP_IID));
	}

	#[test]
	fn a_command_has_one_entry_with_the_traits_values() {
		let command = command();
		assert_eq!(command.command_count(), 1);
		assert_eq!(command.command(0), TestCommand::GUID);
		assert_eq!(command.command_name(0), "Open playlist…");
		assert_eq!(command.description(0).as_deref(), Some("Opens one of your Spotify playlists."));
		assert_eq!(command.parent(), TestPopup::GUID);
		assert_eq!(command.sort_priority(), 0x10000);
		assert_eq!(command.display(0), Some(("Open playlist…".into(), 0)));
	}

	#[test]
	fn executing_the_command_runs_the_trait() {
		let before = EXECUTED.load(Ordering::SeqCst);
		command().execute(0, None).unwrap();
		assert_eq!(EXECUTED.load(Ordering::SeqCst), before + 1);
	}

	#[test]
	fn a_command_that_fails_throws() {
		let command = MainMenuHandle::new(mainmenu_command_service::<FailingCommand>());
		let error = command.execute(0, None).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(error.message, "can't open");
	}

	#[test]
	fn execute_releases_the_callback_it_is_given() {
		let (callback, counter) = CountedService::new();
		command().execute(0, Some(callback)).unwrap();
		assert_eq!(counter.get(), 1);
	}
}
