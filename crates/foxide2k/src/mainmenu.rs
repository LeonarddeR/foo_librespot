//! Adding popups and commands to foobar2000's main menu, including commands whose entry is built
//! each time the menu opens.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::abi::input::ByValueServicePtr;
use crate::abi::mainmenu::{
	COMMANDS_IID, COMMANDS_V2_IID, CommandsV2Vtbl, CommandsVtbl, FLAG_DISABLED, GROUP_IID,
	GROUP_POPUP_IID, GroupPopupVtbl, NODE_IID, NodeVtbl, SORT_PRIORITY_BASE, TYPE_COMMAND,
	TYPE_GROUP,
};
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::boundary::guard;
use crate::component::Factory;
use crate::contextmenu::{RunFn, Shown};
use crate::cxx::ExceptionKind;
use crate::error::{Error, Result};
use crate::guid::Guid;
use crate::service::{ServiceImpl, ServiceObject, ServicePtr, new_service};
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
	/// How it shows each time the menu opens. Runs on the main thread, and never waits. A
	/// shortcut bound to the command ignores it.
	fn shown() -> Shown {
		Shown::Enabled
	}
	/// What picking the command does. An `Err` reaches foobar2000, whose menu helpers wrap
	/// commands in `crashOnException`, so it can crash foobar2000: a command reports its own
	/// failures and returns `Ok`.
	fn execute() -> Result<()>;
}

/// A command whose menu entry is built each time the menu opens, such as a submenu that lists
/// what is playing. Its [`MainMenuCommand`] values name it in the keyboard shortcut list, its
/// `execute` is what a shortcut bound to `NAME` does, and a shortcut bound to a command in its
/// entry ignores its `shown` too.
pub trait MainMenuDynamicCommand: MainMenuCommand {
	/// The entry for the menu. Runs on the main thread while the menu opens, and also when a
	/// shortcut runs one of its entries, to find that entry's command. It never waits.
	fn build() -> MainMenuNode;
}

/// A main-menu entry built as the menu opens. Each name is an `Arc<str>` that the command behind
/// it can share; `&str` and `String` convert with `.into()`.
pub enum MainMenuNode {
	Command {
		name: Arc<str>,
		/// Tells this command apart from the others in the same entry; a shortcut bound to it
		/// names this GUID. It only has to be unique within the entry.
		guid: Guid,
		/// False shows it greyed out, and it can't be picked from the menu. A shortcut bound to
		/// it still runs it: `dynamic_execute` runs a greyed command too.
		enabled: bool,
		/// What picking it does. An `Err` reaches foobar2000, whose menu helpers wrap commands in
		/// `crashOnException`, so it can crash foobar2000: a command reports its own failures and
		/// returns `Ok`.
		run: RunFn,
	},
	Group {
		name: Arc<str>,
		children: Vec<Self>,
	},
}

impl MainMenuNode {
	/// A command named `name`, told apart by `guid`, that runs `run`; greyed out unless
	/// `enabled`.
	pub fn command(name: impl Into<Arc<str>>, guid: Guid, enabled: bool, run: RunFn) -> Self {
		Self::Command { name: name.into(), guid, enabled, run }
	}
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

/// The factory that registers dynamic command `T` with foobar2000.
pub const fn mainmenu_dynamic_factory<T: MainMenuDynamicCommand>() -> Factory {
	Factory::per_instance(&COMMANDS_IID, mainmenu_dynamic_service::<T>)
}

/// A new `mainmenu_commands_v2` service holding the single dynamic command `T`.
pub fn mainmenu_dynamic_service<T: MainMenuDynamicCommand>() -> ServicePtr {
	new_service(Dynamic::<T>(PhantomData))
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

/// The `mainmenu_commands` slots of service `S`, whose one command is `T`.
const fn commands_vtbl<S: ServiceImpl, T: MainMenuCommand>() -> CommandsVtbl {
	CommandsVtbl {
		base: ServiceBaseVtbl::new::<S>(),
		get_command_count: command_count,
		get_command: command_guid::<T>,
		get_name: command_name::<T>,
		get_description: command_description::<T>,
		get_parent: command_parent::<T>,
		get_sort_priority: command_sort_priority::<T>,
		get_display: command_display::<T>,
		execute: command_execute::<T>,
	}
}

struct Command<T>(PhantomData<fn() -> T>);

unsafe impl<T: MainMenuCommand> ServiceImpl for Command<T> {
	type Vtbl = CommandsVtbl;
	const IIDS: &'static [Guid] = &[COMMANDS_IID];
	const VTBL: &'static CommandsVtbl = &commands_vtbl::<Self, T>();
}

struct Dynamic<T>(PhantomData<fn() -> T>);

unsafe impl<T: MainMenuDynamicCommand> ServiceImpl for Dynamic<T> {
	type Vtbl = CommandsV2Vtbl;
	const IIDS: &'static [Guid] = &[COMMANDS_V2_IID, COMMANDS_IID];
	const VTBL: &'static CommandsV2Vtbl = &CommandsV2Vtbl {
		base: commands_vtbl::<Self, T>(),
		is_command_dynamic: dynamic_is_dynamic,
		dynamic_instantiate: dynamic_instantiate::<T>,
		dynamic_execute: dynamic_execute::<T>,
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

/// Returns false for `Shown::Hidden`, which leaves the command out of the menu.
unsafe extern "C-unwind" fn command_display<T: MainMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	text: *mut StringBase,
	flags: *mut u32,
) -> bool {
	unsafe {
		guard(|| {
			let shown = match T::shown() {
				Shown::Hidden => return Ok(false),
				Shown::Enabled => 0,
				Shown::Greyed => FLAG_DISABLED,
			};
			set_string(text, T::NAME)?;
			flags.write(shown);
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

unsafe extern "C-unwind" fn dynamic_is_dynamic(_this: *mut ServiceBase, _index: u32) -> bool {
	true
}

/// Builds the entry and writes one reference to its root node into `ret`.
unsafe extern "C-unwind" fn dynamic_instantiate<T: MainMenuDynamicCommand>(
	_this: *mut ServiceBase,
	ret: *mut *mut ServiceBase,
	_index: u32,
) -> *mut *mut ServiceBase {
	unsafe {
		guard(|| {
			ret.write(node_service(T::build()).into_raw());
			Ok(ret)
		})
	}
}

/// Builds the entry again and runs its command whose GUID is `sub`, greyed or not, as the SDK's
/// default does. Returns false when it has none.
unsafe extern "C-unwind" fn dynamic_execute<T: MainMenuDynamicCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	sub: *const Guid,
	callback: ByValueServicePtr,
) -> bool {
	drop(callback);
	unsafe {
		guard(|| {
			let entry = T::build();
			let Some(run) = find_command(&entry, &*sub) else {
				return Ok(false);
			};
			run()?;
			Ok(true)
		})
	}
}

/// What the command in `node` whose GUID is `sub` runs.
fn find_command<'a>(node: &'a MainMenuNode, sub: &Guid) -> Option<&'a RunFn> {
	match node {
		MainMenuNode::Command { guid, run, .. } => (guid == sub).then_some(run),
		MainMenuNode::Group { children, .. } => {
			children.iter().find_map(|child| find_command(child, sub))
		}
	}
}

/// A new `mainmenu_node` service for `node`, holding a service for each of its children.
fn node_service(node: MainMenuNode) -> ServicePtr {
	new_service(match node {
		MainMenuNode::Command { name, guid, enabled, run } => {
			let flags = if enabled { 0 } else { FLAG_DISABLED };
			Node::Command { name, guid, flags, run }
		}
		MainMenuNode::Group { name, children } => {
			Node::Group { name, children: children.into_iter().map(node_service).collect() }
		}
	})
}

/// A `mainmenu_node`: a command, or a group that holds a reference to each child's node.
enum Node {
	Command { name: Arc<str>, guid: Guid, flags: u32, run: RunFn },
	Group { name: Arc<str>, children: Vec<ServicePtr> },
}

unsafe impl ServiceImpl for Node {
	type Vtbl = NodeVtbl;
	const IIDS: &'static [Guid] = &[NODE_IID];
	const VTBL: &'static NodeVtbl = &NodeVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_type: node_type,
		get_display: node_display,
		get_children_count: node_children_count,
		get_child: node_child,
		execute: node_execute,
		get_guid: node_guid,
		get_description: node_description,
	};
}

unsafe fn node<'a>(this: *mut ServiceBase) -> &'a Node {
	unsafe { ServiceObject::<Node>::value(this) }
}

unsafe extern "C-unwind" fn node_type(this: *mut ServiceBase) -> u32 {
	match unsafe { node(this) } {
		Node::Command { .. } => TYPE_COMMAND,
		Node::Group { .. } => TYPE_GROUP,
	}
}

unsafe extern "C-unwind" fn node_display(
	this: *mut ServiceBase,
	text: *mut StringBase,
	flags: *mut u32,
) {
	unsafe {
		guard(|| {
			let (name, shown) = match node(this) {
				Node::Command { name, flags, .. } => (name, *flags),
				Node::Group { name, .. } => (name, 0),
			};
			set_string(text, name)?;
			flags.write(shown);
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn node_children_count(this: *mut ServiceBase) -> usize {
	match unsafe { node(this) } {
		Node::Group { children, .. } => children.len(),
		Node::Command { .. } => 0,
	}
}

/// Writes one reference to the child into `ret`. Throws `pfc::exception_invalid_params` past
/// the last child, as the SDK's `mainmenu_node_command` does for any index.
unsafe extern "C-unwind" fn node_child(
	this: *mut ServiceBase,
	ret: *mut *mut ServiceBase,
	index: usize,
) -> *mut *mut ServiceBase {
	unsafe {
		guard(|| {
			let child = match node(this) {
				Node::Group { children, .. } => children.get(index),
				Node::Command { .. } => None,
			};
			let child = child.ok_or_else(|| {
				Error::new(ExceptionKind::InvalidParams, "no menu entry at that index")
			})?;
			ret.write(child.clone().into_raw());
			Ok(ret)
		})
	}
}

/// Runs a command; a group does nothing, as the SDK's `mainmenu_node_group` does.
unsafe extern "C-unwind" fn node_execute(this: *mut ServiceBase, callback: ByValueServicePtr) {
	drop(callback);
	unsafe {
		guard(|| match node(this) {
			Node::Command { run, .. } => run(),
			Node::Group { .. } => Ok(()),
		});
	}
}

/// A group's GUID is null, as the SDK's `mainmenu_node_group` gives.
unsafe extern "C-unwind" fn node_guid(this: *mut ServiceBase, ret: *mut Guid) -> *mut Guid {
	let guid = match unsafe { node(this) } {
		Node::Command { guid, .. } => *guid,
		Node::Group { .. } => Guid::NULL,
	};
	unsafe { ret.write(guid) };
	ret
}

unsafe extern "C-unwind" fn node_description(
	_this: *mut ServiceBase,
	_out: *mut StringBase,
) -> bool {
	false
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

	use super::{
		MainMenuCommand, MainMenuDynamicCommand, MainMenuNode, MainMenuPopup,
		mainmenu_command_service, mainmenu_dynamic_service, mainmenu_popup_service,
	};
	use crate::abi::mainmenu::{
		COMMANDS_IID, COMMANDS_V2_IID, FLAG_DISABLED, GROUP_FILE, GROUP_IID, GROUP_POPUP_IID,
		NODE_IID, SORT_PRIORITY_BASE, TYPE_COMMAND, TYPE_GROUP,
	};
	use crate::contextmenu::Shown;
	use crate::cxx::ExceptionKind;
	use crate::error::{Error, Result};
	use crate::guid::Guid;
	use crate::testing::{CountedService, MainMenuHandle, MainMenuNodeHandle};

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

	struct HiddenCommand;

	impl MainMenuCommand for HiddenCommand {
		const GUID: Guid = Guid::from_u128(0xd97cbfbc_97c4_4832_a05d_491e62c83cc8);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Hidden";
		const DESCRIPTION: &'static str = "Never shows.";

		fn shown() -> Shown {
			Shown::Hidden
		}

		fn execute() -> Result<()> {
			Ok(())
		}
	}

	struct GreyedCommand;

	impl MainMenuCommand for GreyedCommand {
		const GUID: Guid = Guid::from_u128(0x6bdf1b5b_c823_44a3_a50c_468d7d1350e3);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Open album";
		const DESCRIPTION: &'static str = "Opens the playing track's album.";

		fn shown() -> Shown {
			Shown::Greyed
		}

		fn execute() -> Result<()> {
			Ok(())
		}
	}

	#[test]
	fn a_hidden_command_isnt_displayed() {
		let command = MainMenuHandle::new(mainmenu_command_service::<HiddenCommand>());
		assert_eq!(command.display(0), None);
	}

	#[test]
	fn a_greyed_command_is_displayed_disabled() {
		let command = MainMenuHandle::new(mainmenu_command_service::<GreyedCommand>());
		assert_eq!(command.display(0), Some(("Open album".into(), FLAG_DISABLED)));
	}

	thread_local! {
		static RAN: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
		static FLAG: RefCell<Arc<AtomicBool>> = RefCell::new(Arc::new(AtomicBool::new(false)));
	}

	const FIRST: Guid = Guid::from_u128(0x23ed6d80_6032_43f4_aeb2_31287ed48334);
	const SECOND: Guid = Guid::from_u128(0x87c28712_d970_4574_9e9f_e39b8fa3b4f7);

	/// A command that records `name` in `RAN` when it runs.
	fn records(name: &'static str, guid: Guid, enabled: bool) -> MainMenuNode {
		MainMenuNode::command(
			name,
			guid,
			enabled,
			Box::new(move || {
				RAN.with_borrow_mut(|ran| ran.push(name));
				Ok(())
			}),
		)
	}

	#[test]
	fn command_makes_a_command_node_with_its_values() {
		let MainMenuNode::Command { name, guid, enabled, run } = records("First", FIRST, false)
		else {
			panic!("not a command");
		};
		assert_eq!((&*name, guid, enabled), ("First", FIRST, false));
		run().unwrap();
		assert_eq!(RAN.take(), ["First"]);
	}

	struct TestDynamic;

	impl MainMenuCommand for TestDynamic {
		const GUID: Guid = Guid::from_u128(0x080a91b9_2c65_4ad7_834a_95f37bcb08e7);
		const PARENT: Guid = TestPopup::GUID;
		const SORT_PRIORITY: u32 = SORT_PRIORITY_BASE + 1;
		const NAME: &'static str = "Open artist";
		const DESCRIPTION: &'static str = "Opens one of the playing track's artists.";

		fn execute() -> Result<()> {
			RAN.with_borrow_mut(|ran| ran.push("shortcut"));
			Ok(())
		}
	}

	impl MainMenuDynamicCommand for TestDynamic {
		fn build() -> MainMenuNode {
			MainMenuNode::Group {
				name: "Open artist".into(),
				children: vec![records("First", FIRST, true), records("Second", SECOND, false)],
			}
		}
	}

	struct FailingDynamic;

	impl MainMenuCommand for FailingDynamic {
		const GUID: Guid = Guid::from_u128(0xdf1269fa_22ba_458b_b3a4_eff8b6717345);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Fail";
		const DESCRIPTION: &'static str = "Always fails.";

		fn execute() -> Result<()> {
			Err(Error::new(ExceptionKind::Io, "can't open"))
		}
	}

	impl MainMenuDynamicCommand for FailingDynamic {
		fn build() -> MainMenuNode {
			MainMenuNode::command(
				"Fail",
				FIRST,
				true,
				Box::new(|| Err(Error::new(ExceptionKind::Io, "can't open"))),
			)
		}
	}

	struct DropFlag(Arc<AtomicBool>);

	impl Drop for DropFlag {
		fn drop(&mut self) {
			self.0.store(true, Ordering::SeqCst);
		}
	}

	/// A group whose only child holds a `DropFlag` from `FLAG`.
	struct FlagDynamic;

	impl MainMenuCommand for FlagDynamic {
		const GUID: Guid = Guid::from_u128(0xdf1269fa_22ba_458b_b3a4_eff8b6717346);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Flag";
		const DESCRIPTION: &'static str = "Holds a drop flag.";

		fn execute() -> Result<()> {
			Ok(())
		}
	}

	impl MainMenuDynamicCommand for FlagDynamic {
		fn build() -> MainMenuNode {
			let flag = DropFlag(FLAG.with_borrow(Arc::clone));
			MainMenuNode::Group {
				name: "Flag".into(),
				children: vec![MainMenuNode::command(
					"Child",
					FIRST,
					true,
					Box::new(move || {
						let _held = &flag;
						Ok(())
					}),
				)],
			}
		}
	}

	fn dynamic() -> MainMenuHandle {
		MainMenuHandle::new(mainmenu_dynamic_service::<TestDynamic>())
	}

	struct HiddenDynamic;

	impl MainMenuCommand for HiddenDynamic {
		const GUID: Guid = Guid::from_u128(0x119610c7_c4f3_4825_8987_f23287117925);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Open artist";
		const DESCRIPTION: &'static str = "Shows only for a track.";

		fn shown() -> Shown {
			Shown::Hidden
		}

		fn execute() -> Result<()> {
			RAN.with_borrow_mut(|ran| ran.push("hidden shortcut"));
			Ok(())
		}
	}

	impl MainMenuDynamicCommand for HiddenDynamic {
		fn build() -> MainMenuNode {
			records("Hidden", FIRST, true)
		}
	}

	struct GreyedDynamic;

	impl MainMenuCommand for GreyedDynamic {
		const GUID: Guid = Guid::from_u128(0x88026a85_8e5c_4f5b_b1f0_1932fe59ae7a);
		const PARENT: Guid = TestPopup::GUID;
		const NAME: &'static str = "Open artist";
		const DESCRIPTION: &'static str = "Greyed out while nothing plays.";

		fn shown() -> Shown {
			Shown::Greyed
		}

		fn execute() -> Result<()> {
			Ok(())
		}
	}

	impl MainMenuDynamicCommand for GreyedDynamic {
		fn build() -> MainMenuNode {
			records("Greyed", FIRST, false)
		}
	}

	#[test]
	fn a_hidden_dynamic_command_isnt_displayed_but_its_shortcuts_still_run() {
		let command = MainMenuHandle::new(mainmenu_dynamic_service::<HiddenDynamic>());
		assert_eq!(command.display(0), None);
		command.execute(0, None).unwrap();
		assert!(command.dynamic_execute(0, &FIRST).unwrap());
		assert_eq!(RAN.take(), ["hidden shortcut", "Hidden"]);
	}

	#[test]
	fn a_greyed_dynamic_command_is_displayed_disabled() {
		let command = MainMenuHandle::new(mainmenu_dynamic_service::<GreyedDynamic>());
		assert_eq!(command.display(0), Some(("Open artist".into(), FLAG_DISABLED)));
	}

	#[test]
	fn a_dynamic_command_answers_as_mainmenu_commands_v2() {
		let command = dynamic();
		assert!(command.answers(&COMMANDS_V2_IID));
		assert!(command.answers(&COMMANDS_IID));
		assert!(!command.answers(&GROUP_IID));
		assert!(command.is_dynamic(0));
	}

	#[test]
	fn a_dynamic_command_has_one_entry_with_the_traits_values() {
		let command = dynamic();
		assert_eq!(command.command_count(), 1);
		assert_eq!(command.command(0), TestDynamic::GUID);
		assert_eq!(command.command_name(0), "Open artist");
		assert_eq!(
			command.description(0).as_deref(),
			Some("Opens one of the playing track's artists.")
		);
		assert_eq!(command.parent(), TestPopup::GUID);
		assert_eq!(command.sort_priority(), 0x10001);
		assert_eq!(command.display(0), Some(("Open artist".into(), 0)));
		let defaults = MainMenuHandle::new(mainmenu_dynamic_service::<FailingDynamic>());
		assert_eq!(defaults.sort_priority(), 0x10000);
	}

	#[test]
	fn a_dynamic_group_shows_its_children() {
		let node = dynamic().instantiate(0);
		assert!(node.answers(&NODE_IID));
		assert_eq!(node.kind(), TYPE_GROUP);
		assert_eq!(node.display(), ("Open artist".into(), 0));
		assert_eq!(node.guid(), Guid::NULL);
		let children = node.children();
		let shown: Vec<_> = children.iter().map(MainMenuNodeHandle::display).collect();
		assert_eq!(shown, [("First".into(), 0), ("Second".into(), FLAG_DISABLED)]);
		let guids: Vec<_> = children.iter().map(MainMenuNodeHandle::guid).collect();
		assert_eq!(guids, [FIRST, SECOND]);
		assert!(children.iter().all(|child| child.kind() == TYPE_COMMAND));
		assert!(children.iter().all(|child| child.children().is_empty()));
		assert!(children.iter().all(|child| child.description().is_none()));
	}

	#[test]
	fn a_child_past_the_end_throws() {
		let node = dynamic().instantiate(0);
		let error = node.child(2).err().expect("get_child past the end");
		assert_eq!(error.kind, ExceptionKind::InvalidParams);
		let command = node.child(0).unwrap();
		assert_eq!(
			command.child(0).err().map(|error| error.kind),
			Some(ExceptionKind::InvalidParams)
		);
	}

	#[test]
	fn executing_a_dynamic_child_runs_it() {
		let command = dynamic();
		command.instantiate(0).children()[0].execute(None).unwrap();
		assert_eq!(RAN.take(), ["First"]);
		assert!(command.dynamic_execute(0, &SECOND).unwrap());
		assert_eq!(RAN.take(), ["Second"]);
	}

	#[test]
	fn dynamic_execute_with_an_unknown_guid_runs_nothing_and_says_so() {
		let command = dynamic();
		assert!(!command.dynamic_execute(0, &Guid::from_u128(7)).unwrap());
		assert!(!command.dynamic_execute(0, &Guid::NULL).unwrap());
		assert!(RAN.take().is_empty());
	}

	#[test]
	fn executing_a_dynamic_group_runs_nothing() {
		dynamic().instantiate(0).execute(None).unwrap();
		assert!(RAN.take().is_empty());
	}

	#[test]
	fn executing_a_dynamic_command_by_its_shortcut_runs_execute() {
		dynamic().execute(0, None).unwrap();
		assert_eq!(RAN.take(), ["shortcut"]);
	}

	#[test]
	fn a_dynamic_command_that_fails_throws() {
		let command = MainMenuHandle::new(mainmenu_dynamic_service::<FailingDynamic>());
		for error in [
			command.instantiate(0).execute(None).unwrap_err(),
			command.dynamic_execute(0, &FIRST).unwrap_err(),
			command.execute(0, None).unwrap_err(),
		] {
			assert_eq!(error.kind, ExceptionKind::Io);
			assert_eq!(error.message, "can't open");
		}
	}

	#[test]
	fn a_node_releases_the_callback_it_is_given() {
		let (callback, counter) = CountedService::new();
		dynamic().instantiate(0).children()[0].execute(Some(callback)).unwrap();
		assert_eq!(counter.get(), 1);
	}

	#[test]
	fn the_nodes_are_freed_with_their_last_reference() {
		let dropped = Arc::new(AtomicBool::new(false));
		FLAG.with_borrow_mut(|flag| *flag = Arc::clone(&dropped));
		let root = MainMenuHandle::new(mainmenu_dynamic_service::<FlagDynamic>()).instantiate(0);
		let child = root.child(0).unwrap();
		drop(root);
		assert!(!dropped.load(Ordering::SeqCst));
		child.execute(None).unwrap();
		drop(child);
		assert!(dropped.load(Ordering::SeqCst));
	}
}
