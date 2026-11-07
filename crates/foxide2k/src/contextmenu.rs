//! Right-click menu items: items whose submenus are built each time the menu opens, commands
//! that can be bound to keys, and the submenus they sit in.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::abi::contextmenu::{
	ENABLED_DEFAULT_ON, ENABLED_FORCE_OFF, FLAG_CHECKED, FLAG_DISABLED, FLAG_GRAYED, GROUP_IID,
	GROUP_POPUP_IID, GROUP_ROOT, GroupPopupVtbl, ITEM_IID, ITEM_V2_IID, ItemVtbl, Node, NodeVtbl,
	TYPE_COMMAND, TYPE_GROUP,
};
use crate::abi::metadb_handle::HandleListRaw;
use crate::abi::service::{ServiceBase, ServiceBaseVtbl};
use crate::abi::string_base::StringBase;
use crate::boundary::guard;
use crate::component::Factory;
use crate::error::Result;
use crate::guid::Guid;
use crate::metadb::HandleList;
use crate::service::{ServiceImpl, ServicePtr, free_from_release, new_service};
use crate::strings::set_string;

/// What a command does: runs on the main thread when the command is picked.
pub type RunFn = Box<dyn Fn() -> Result<()> + Send + Sync>;

/// What a right-click menu item shows, built each time the menu opens. Each name is an `Arc<str>`
/// that the command behind it can share; `&str` and `String` convert with `.into()`.
pub enum MenuNode {
	Command {
		name: Arc<str>,
		checked: bool,
		/// False shows it greyed out, and it can't be picked.
		enabled: bool,
		run: RunFn,
	},
	Popup {
		name: Arc<str>,
		children: Vec<Self>,
	},
}

impl MenuNode {
	/// An unchecked command named `name` that runs `run`; greyed out unless `enabled`.
	pub fn command(name: impl Into<Arc<str>>, enabled: bool, run: RunFn) -> Self {
		Self::Command { name: name.into(), checked: false, enabled, run }
	}
}

pub trait ContextMenuItem: 'static {
	const GUID: Guid;
	/// Its name in the keyboard shortcut list and in Preferences > Context Menu.
	const NAME: &'static str;
	const DESCRIPTION: &'static str;
	const PARENT: Guid = GROUP_ROOT;
	const SORT_PRIORITY: f64 = 0.0;
	/// The node to show for `selection`, the selected items; `None` hides the item. Runs on the
	/// main thread while the menu opens, and must not wait for anything. A command that needs the
	/// selection keeps what it needs from it.
	fn build(selection: &HandleList<'_>) -> Option<MenuNode>;
}

/// A submenu other items sit in, such as right-click > Spotify. Items and groups inside it name
/// its `GUID` as their `PARENT`.
pub trait ContextMenuGroup: 'static {
	const GUID: Guid;
	const PARENT: Guid = GROUP_ROOT;
	const SORT_PRIORITY: f64 = 0.0;
	const NAME: &'static str;
}

/// One command whose name never changes, so it can be bound to a key.
pub trait ContextMenuCommand: 'static {
	const GUID: Guid;
	/// Its name in the menu, in the keyboard shortcut list and in Preferences > Context Menu.
	const NAME: &'static str;
	const DESCRIPTION: &'static str;
	const PARENT: Guid = GROUP_ROOT;
	const SORT_PRIORITY: f64 = 0.0;
	/// Shown only in the keyboard shortcut list (`FORCE_OFF`).
	const SHORTCUT_ONLY: bool = false;
	/// How it shows for `selection`, the selected items. Runs on the main thread while the menu
	/// opens, and never waits.
	fn shown(selection: &HandleList<'_>) -> Shown;
	/// Runs on the main thread, from the menu or a shortcut, which ignores `shown`. A shortcut
	/// passes the selection or the playing track.
	fn execute(selection: &HandleList<'_>) -> Result<()>;
}

/// How a command shows in a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
	/// Left out of the menu.
	Hidden,
	Enabled,
	/// Greyed out, and it can't be picked.
	Greyed,
}

/// The factory that registers item `T` with foobar2000.
pub const fn context_menu_factory<T: ContextMenuItem>() -> Factory {
	Factory::per_instance(&ITEM_IID, context_menu_service::<T>)
}

/// A new `contextmenu_item_v2` service for `T`.
pub fn context_menu_service<T: ContextMenuItem>() -> ServicePtr {
	new_service(Item::<T>(PhantomData))
}

/// The factory that registers group `T` with foobar2000.
pub const fn context_menu_group_factory<T: ContextMenuGroup>() -> Factory {
	Factory::per_instance(&GROUP_IID, context_menu_group_service::<T>)
}

/// A new `contextmenu_group_popup` service for `T`.
pub fn context_menu_group_service<T: ContextMenuGroup>() -> ServicePtr {
	new_service(Group::<T>(PhantomData))
}

/// The factory that registers command `T` with foobar2000.
pub const fn context_menu_command_factory<T: ContextMenuCommand>() -> Factory {
	Factory::per_instance(&ITEM_IID, context_menu_command_service::<T>)
}

/// A new `contextmenu_item_v2` service holding the single command `T`.
pub fn context_menu_command_service<T: ContextMenuCommand>() -> ServicePtr {
	new_service(Command::<T>(PhantomData))
}

/// What a `contextmenu_item_v2` service with one item says about it, the same for any selection.
trait Entry: 'static {
	const GUID: Guid;
	const NAME: &'static str;
	const DESCRIPTION: &'static str;
	const PARENT: Guid;
	const SORT_PRIORITY: f64;
}

struct Item<T>(PhantomData<fn() -> T>);

impl<T: ContextMenuItem> Entry for Item<T> {
	const GUID: Guid = T::GUID;
	const NAME: &'static str = T::NAME;
	const DESCRIPTION: &'static str = T::DESCRIPTION;
	const PARENT: Guid = T::PARENT;
	const SORT_PRIORITY: f64 = T::SORT_PRIORITY;
}

unsafe impl<T: ContextMenuItem> ServiceImpl for Item<T> {
	type Vtbl = ItemVtbl;
	const IIDS: &'static [Guid] = &[ITEM_V2_IID, ITEM_IID];
	const VTBL: &'static ItemVtbl = &item_vtbl::<Self>(item_instantiate::<T>);
}

/// The `contextmenu_item_v2` slots of service `E`, whose one item's node `instantiate_item`
/// makes. The item is on by default and a shortcut runs nothing; a command replaces those two
/// slots.
const fn item_vtbl<E: Entry + ServiceImpl>(
	instantiate_item: unsafe extern "C-unwind" fn(
		*mut ServiceBase,
		u32,
		*const HandleListRaw,
		*const Guid,
	) -> *mut Node,
) -> ItemVtbl {
	ItemVtbl {
		base: ServiceBaseVtbl::new::<E>(),
		get_num_items: item_count,
		instantiate_item,
		get_item_guid: item_guid::<E>,
		get_item_name: item_name::<E>,
		get_item_default_path: item_default_path,
		get_item_description: item_description::<E>,
		get_enabled_state: item_enabled_state,
		item_execute_simple,
		get_sort_priority: item_sort_priority::<E>,
		get_parent: item_parent::<E>,
	}
}

struct Command<T>(PhantomData<fn() -> T>);

impl<T: ContextMenuCommand> Entry for Command<T> {
	const GUID: Guid = T::GUID;
	const NAME: &'static str = T::NAME;
	const DESCRIPTION: &'static str = T::DESCRIPTION;
	const PARENT: Guid = T::PARENT;
	const SORT_PRIORITY: f64 = T::SORT_PRIORITY;
}

unsafe impl<T: ContextMenuCommand> ServiceImpl for Command<T> {
	type Vtbl = ItemVtbl;
	const IIDS: &'static [Guid] = &[ITEM_V2_IID, ITEM_IID];
	const VTBL: &'static ItemVtbl = &ItemVtbl {
		get_enabled_state: command_enabled_state::<T>,
		item_execute_simple: command_execute_simple::<T>,
		..item_vtbl::<Self>(command_instantiate::<T>)
	};
}

unsafe extern "C-unwind" fn item_count(_this: *mut ServiceBase) -> u32 {
	1
}

unsafe extern "C-unwind" fn item_instantiate<T: ContextMenuItem>(
	_this: *mut ServiceBase,
	_index: u32,
	data: *const HandleListRaw,
	_caller: *const Guid,
) -> *mut Node {
	unsafe {
		guard(|| {
			let kind =
				T::build(&HandleList::from_raw(data)).map_or(NodeKind::Hidden, NodeKind::from);
			Ok(Box::into_raw(Box::new(NodeObject { vtbl: &raw const NODE_VTBL, kind }))
				.cast::<Node>())
		})
	}
}

/// The command's node, which answers from the tracks each of its calls passes.
unsafe extern "C-unwind" fn command_instantiate<T: ContextMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	_data: *const HandleListRaw,
	_caller: *const Guid,
) -> *mut Node {
	unsafe {
		guard(|| {
			let node = CommandNode::<T> { vtbl: CommandNode::<T>::VTBL, _command: PhantomData };
			Ok(Box::into_raw(Box::new(node)).cast::<Node>())
		})
	}
}

unsafe extern "C-unwind" fn item_guid<E: Entry>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
	_index: u32,
) -> *mut Guid {
	unsafe { ret.write(E::GUID) };
	ret
}

unsafe extern "C-unwind" fn item_name<E: Entry>(
	_this: *mut ServiceBase,
	_index: u32,
	out: *mut StringBase,
) {
	unsafe { guard(|| set_string(out, E::NAME)) }
}

unsafe extern "C-unwind" fn item_default_path(
	_this: *mut ServiceBase,
	_index: u32,
	out: *mut StringBase,
) {
	unsafe { guard(|| set_string(out, "")) }
}

unsafe extern "C-unwind" fn item_description<E: Entry>(
	_this: *mut ServiceBase,
	_index: u32,
	out: *mut StringBase,
) -> bool {
	unsafe {
		guard(|| {
			set_string(out, E::DESCRIPTION)?;
			Ok(true)
		})
	}
}

unsafe extern "C-unwind" fn item_enabled_state(_this: *mut ServiceBase, _index: u32) -> i32 {
	ENABLED_DEFAULT_ON
}

unsafe extern "C-unwind" fn command_enabled_state<T: ContextMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
) -> i32 {
	if T::SHORTCUT_ONLY { ENABLED_FORCE_OFF } else { ENABLED_DEFAULT_ON }
}

unsafe extern "C-unwind" fn item_execute_simple(
	_this: *mut ServiceBase,
	_index: u32,
	_node: *const Guid,
	_data: *const HandleListRaw,
	_caller: *const Guid,
) {
}

/// Runs the command when `node` is null, the GUID its node answers, as `contextmenu_item_simple`
/// does.
unsafe extern "C-unwind" fn command_execute_simple<T: ContextMenuCommand>(
	_this: *mut ServiceBase,
	_index: u32,
	node: *const Guid,
	data: *const HandleListRaw,
	_caller: *const Guid,
) {
	unsafe {
		guard(|| {
			if *node == Guid::NULL {
				T::execute(&HandleList::from_raw(data))?;
			}
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn item_sort_priority<E: Entry>(_this: *mut ServiceBase) -> f64 {
	E::SORT_PRIORITY
}

unsafe extern "C-unwind" fn item_parent<E: Entry>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(E::PARENT) };
	ret
}

struct Group<T>(PhantomData<fn() -> T>);

unsafe impl<T: ContextMenuGroup> ServiceImpl for Group<T> {
	type Vtbl = GroupPopupVtbl;
	const IIDS: &'static [Guid] = &[GROUP_POPUP_IID, GROUP_IID];
	const VTBL: &'static GroupPopupVtbl = &GroupPopupVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_guid: group_guid::<T>,
		get_parent: group_parent::<T>,
		get_sort_priority: group_sort_priority::<T>,
		get_display_string: group_display_string::<T>,
	};
}

unsafe extern "C-unwind" fn group_guid<T: ContextMenuGroup>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::GUID) };
	ret
}

unsafe extern "C-unwind" fn group_parent<T: ContextMenuGroup>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::PARENT) };
	ret
}

unsafe extern "C-unwind" fn group_sort_priority<T: ContextMenuGroup>(
	_this: *mut ServiceBase,
) -> f64 {
	T::SORT_PRIORITY
}

unsafe extern "C-unwind" fn group_display_string<T: ContextMenuGroup>(
	_this: *mut ServiceBase,
	out: *mut StringBase,
) {
	unsafe { guard(|| set_string(out, T::NAME)) }
}

/// A node as foobar2000 sees it: the vtable pointer first, so a pointer to it is a `Node`.
#[repr(C)]
struct NodeObject {
	vtbl: *const NodeVtbl,
	kind: NodeKind,
}

enum NodeKind {
	/// An item with nothing to show.
	Hidden,
	Command {
		name: Arc<str>,
		flags: u32,
		run: RunFn,
	},
	Popup {
		name: Arc<str>,
		children: Vec<NodeObject>,
	},
}

impl From<MenuNode> for NodeKind {
	fn from(node: MenuNode) -> Self {
		match node {
			MenuNode::Command { name, checked, enabled, run } => {
				let mut flags = 0;
				if checked {
					flags |= FLAG_CHECKED;
				}
				if !enabled {
					flags |= FLAG_DISABLED | FLAG_GRAYED;
				}
				Self::Command { name, flags, run }
			}
			MenuNode::Popup { name, children } => Self::Popup {
				name,
				children: children
					.into_iter()
					.map(|child| NodeObject { vtbl: &raw const NODE_VTBL, kind: child.into() })
					.collect(),
			},
		}
	}
}

static NODE_VTBL: NodeVtbl = NodeVtbl {
	get_display_data: node_display_data,
	get_type: node_type,
	execute: node_execute,
	get_glyph: node_glyph,
	get_children_count: node_children_count,
	get_child: node_child,
	get_description: node_description,
	get_guid: node_guid,
	is_mappable_shortcut: node_is_mappable_shortcut,
	delete: node_delete,
};

unsafe fn kind<'a>(this: *mut Node) -> &'a NodeKind {
	unsafe { &(*this.cast::<NodeObject>()).kind }
}

unsafe extern "C-unwind" fn node_display_data(
	this: *mut Node,
	out: *mut StringBase,
	flags: *mut u32,
	_data: *const HandleListRaw,
	_caller: *const Guid,
) -> bool {
	unsafe {
		guard(|| match kind(this) {
			NodeKind::Hidden => Ok(false),
			NodeKind::Command { name, flags: shown, .. } => {
				set_string(out, name)?;
				flags.write(*shown);
				Ok(true)
			}
			NodeKind::Popup { name, .. } => {
				set_string(out, name)?;
				flags.write(0);
				Ok(true)
			}
		})
	}
}

unsafe extern "C-unwind" fn node_type(this: *mut Node) -> i32 {
	match unsafe { kind(this) } {
		NodeKind::Command { .. } => TYPE_COMMAND,
		NodeKind::Hidden | NodeKind::Popup { .. } => TYPE_GROUP,
	}
}

unsafe extern "C-unwind" fn node_execute(
	this: *mut Node,
	_data: *const HandleListRaw,
	_caller: *const Guid,
) {
	unsafe {
		guard(|| {
			if let NodeKind::Command { run, .. } = kind(this) {
				run()?;
			}
			Ok(())
		});
	}
}

unsafe extern "C-unwind" fn node_glyph(
	_this: *mut Node,
	_data: *const HandleListRaw,
	_caller: *const Guid,
) -> *mut c_void {
	std::ptr::null_mut()
}

unsafe extern "C-unwind" fn node_children_count(this: *mut Node) -> usize {
	match unsafe { kind(this) } {
		NodeKind::Popup { children, .. } => children.len(),
		NodeKind::Hidden | NodeKind::Command { .. } => 0,
	}
}

unsafe extern "C-unwind" fn node_child(this: *mut Node, index: usize) -> *mut Node {
	match unsafe { kind(this) } {
		NodeKind::Popup { children, .. } => {
			children.get(index).map_or(std::ptr::null_mut(), |child| {
				std::ptr::from_ref::<NodeObject>(child).cast_mut().cast()
			})
		}
		NodeKind::Hidden | NodeKind::Command { .. } => std::ptr::null_mut(),
	}
}

unsafe extern "C-unwind" fn node_description(_this: *mut Node, _out: *mut StringBase) -> bool {
	false
}

unsafe extern "C-unwind" fn node_guid(_this: *mut Node, ret: *mut Guid) -> *mut Guid {
	unsafe { ret.write(Guid::NULL) };
	ret
}

unsafe extern "C-unwind" fn node_is_mappable_shortcut(_this: *mut Node) -> bool {
	false
}

/// The root's scalar deleting destructor: frees the whole tree when `flags & 1`.
unsafe extern "C-unwind" fn node_delete(this: *mut Node, flags: u32) -> *mut c_void {
	if flags & 1 != 0 {
		unsafe { free_from_release(this.cast::<NodeObject>()) };
	}
	this.cast()
}

/// A command's node, the root and only node, as `contextmenu_item_simple` makes: its GUID is
/// null and it can be bound to a key.
#[repr(C)]
struct CommandNode<T> {
	vtbl: *const NodeVtbl,
	_command: PhantomData<fn() -> T>,
}

impl<T: ContextMenuCommand> CommandNode<T> {
	const VTBL: &'static NodeVtbl = &NodeVtbl {
		get_display_data: command_display_data::<T>,
		get_type: command_type,
		execute: command_execute::<T>,
		get_glyph: node_glyph,
		get_children_count: command_children_count,
		get_child: command_child,
		get_description: command_description::<T>,
		get_guid: node_guid,
		is_mappable_shortcut: command_is_mappable_shortcut,
		delete: command_delete::<T>,
	};
}

unsafe extern "C-unwind" fn command_display_data<T: ContextMenuCommand>(
	_this: *mut Node,
	out: *mut StringBase,
	flags: *mut u32,
	data: *const HandleListRaw,
	_caller: *const Guid,
) -> bool {
	unsafe {
		guard(|| {
			let shown = match T::shown(&HandleList::from_raw(data)) {
				Shown::Hidden => return Ok(false),
				Shown::Enabled => 0,
				Shown::Greyed => FLAG_DISABLED | FLAG_GRAYED,
			};
			set_string(out, T::NAME)?;
			flags.write(shown);
			Ok(true)
		})
	}
}

unsafe extern "C-unwind" fn command_type(_this: *mut Node) -> i32 {
	TYPE_COMMAND
}

unsafe extern "C-unwind" fn command_execute<T: ContextMenuCommand>(
	_this: *mut Node,
	data: *const HandleListRaw,
	_caller: *const Guid,
) {
	unsafe { guard(|| T::execute(&HandleList::from_raw(data))) }
}

unsafe extern "C-unwind" fn command_children_count(_this: *mut Node) -> usize {
	0
}

unsafe extern "C-unwind" fn command_child(_this: *mut Node, _index: usize) -> *mut Node {
	std::ptr::null_mut()
}

unsafe extern "C-unwind" fn command_description<T: ContextMenuCommand>(
	_this: *mut Node,
	out: *mut StringBase,
) -> bool {
	unsafe {
		guard(|| {
			set_string(out, T::DESCRIPTION)?;
			Ok(true)
		})
	}
}

unsafe extern "C-unwind" fn command_is_mappable_shortcut(_this: *mut Node) -> bool {
	true
}

/// The node's scalar deleting destructor: frees it when `flags & 1`.
unsafe extern "C-unwind" fn command_delete<T: ContextMenuCommand>(
	this: *mut Node,
	flags: u32,
) -> *mut c_void {
	if flags & 1 != 0 {
		unsafe { free_from_release(this.cast::<CommandNode<T>>()) };
	}
	this.cast()
}

#[cfg(test)]
mod tests {
	use std::cell::{Cell, RefCell};
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, Ordering};

	use super::{
		ContextMenuCommand, ContextMenuGroup, ContextMenuItem, MenuNode, Shown,
		context_menu_command_service, context_menu_group_service, context_menu_service,
	};
	use crate::abi::contextmenu::{
		ENABLED_DEFAULT_ON, ENABLED_FORCE_OFF, FLAG_CHECKED, FLAG_DISABLED, FLAG_GRAYED, GROUP_IID,
		GROUP_POPUP_IID, GROUP_ROOT, ITEM_IID, ITEM_V2_IID, TYPE_COMMAND, TYPE_GROUP,
	};
	use crate::cxx::ExceptionKind;
	use crate::error::{Error, Result};
	use crate::guid::Guid;
	use crate::metadb::HandleList;
	use crate::testing::{ContextMenuGroupHandle, ContextMenuHandle, FakeHandleList};

	thread_local! {
		static BUILT_WITH: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
		static RAN_WITH: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
		static RUNS: Cell<usize> = const { Cell::new(0) };
		static FLAG: RefCell<Arc<AtomicBool>> = RefCell::new(Arc::new(AtomicBool::new(false)));
		static SHARED: RefCell<Arc<str>> = RefCell::new(Arc::from(""));
	}

	/// A command that keeps `paths` to record when it runs.
	fn command(name: &str, checked: bool, enabled: bool, paths: &[String]) -> MenuNode {
		let paths = paths.to_vec();
		MenuNode::Command {
			name: name.into(),
			checked,
			enabled,
			run: Box::new(move || {
				RUNS.set(RUNS.get() + 1);
				RAN_WITH.with_borrow_mut(|ran| ran.clone_from(&paths));
				Ok(())
			}),
		}
	}

	struct TestItem;

	impl ContextMenuItem for TestItem {
		const GUID: Guid = Guid::from_u128(0x7d2c91a4_3b58_4e06_9f13_c8a5e0b64d72);
		const NAME: &'static str = "Add to test";
		const DESCRIPTION: &'static str = "Adds the tracks to a test list.";
		const SORT_PRIORITY: f64 = 2.5;

		fn build(selection: &HandleList<'_>) -> Option<MenuNode> {
			let paths = selection.paths().ok()?;
			BUILT_WITH.with_borrow_mut(|built| built.clone_from(&paths));
			if paths.is_empty() {
				return None;
			}
			Some(MenuNode::Popup {
				name: "Add to test".into(),
				children: vec![
					command("Checked one", true, true, &paths),
					command("Greyed one", false, false, &paths),
					command("Plain", false, true, &paths),
				],
			})
		}
	}

	struct FailingItem;

	impl ContextMenuItem for FailingItem {
		const GUID: Guid = Guid::from_u128(0x7d2c91a4_3b58_4e06_9f13_c8a5e0b64d73);
		const NAME: &'static str = "Fail";
		const DESCRIPTION: &'static str = "Always fails.";

		fn build(_selection: &HandleList<'_>) -> Option<MenuNode> {
			Some(MenuNode::command(
				"Fail",
				true,
				Box::new(|| Err(Error::new(ExceptionKind::Io, "can't add"))),
			))
		}
	}

	struct DropFlag(Arc<AtomicBool>);

	impl Drop for DropFlag {
		fn drop(&mut self) {
			self.0.store(true, Ordering::SeqCst);
		}
	}

	struct FlagItem;

	impl ContextMenuItem for FlagItem {
		const GUID: Guid = Guid::from_u128(0x7d2c91a4_3b58_4e06_9f13_c8a5e0b64d74);
		const NAME: &'static str = "Flag";
		const DESCRIPTION: &'static str = "Holds a drop flag.";

		fn build(_selection: &HandleList<'_>) -> Option<MenuNode> {
			let flag = DropFlag(FLAG.with_borrow(Arc::clone));
			Some(MenuNode::Popup {
				name: "Flag".into(),
				children: vec![MenuNode::command(
					"Child",
					true,
					Box::new(move || {
						let _held = &flag;
						Ok(())
					}),
				)],
			})
		}
	}

	struct SharedItem;

	impl ContextMenuItem for SharedItem {
		const GUID: Guid = Guid::from_u128(0x7d2c91a4_3b58_4e06_9f13_c8a5e0b64d75);
		const NAME: &'static str = "Shared";
		const DESCRIPTION: &'static str = "Shows the name in `SHARED`.";

		fn build(_selection: &HandleList<'_>) -> Option<MenuNode> {
			Some(MenuNode::command(SHARED.with_borrow(Arc::clone), true, Box::new(|| Ok(()))))
		}
	}

	#[test]
	fn command_makes_an_unchecked_command_node_with_its_values() {
		let node = MenuNode::command(
			"Plain",
			false,
			Box::new(|| {
				RUNS.set(RUNS.get() + 1);
				Ok(())
			}),
		);
		let MenuNode::Command { name, checked, enabled, run } = node else {
			panic!("not a command");
		};
		assert_eq!((&*name, checked, enabled), ("Plain", false, false));
		run().unwrap();
		assert_eq!(RUNS.get(), 1);
	}

	fn item() -> ContextMenuHandle {
		ContextMenuHandle::new(context_menu_service::<TestItem>())
	}

	const PATH: &str = "sptf://spotify:track:2hQxLiI93xdjqENJuKeXjO";

	#[test]
	fn an_item_answers_as_contextmenu_item_v2_and_contextmenu_item() {
		let item = item();
		assert!(item.answers(&ITEM_V2_IID));
		assert!(item.answers(&ITEM_IID));
		assert!(!item.answers(&Guid::from_u128(1)));
	}

	#[test]
	fn an_item_takes_name_guid_description_parent_and_priority_from_the_trait() {
		let item = item();
		assert_eq!(item.item_count(), 1);
		assert_eq!(item.guid(), TestItem::GUID);
		assert_eq!(item.name(), "Add to test");
		assert_eq!(item.default_path(), "");
		assert_eq!(item.description().as_deref(), Some("Adds the tracks to a test list."));
		assert_eq!(item.parent(), GROUP_ROOT);
		assert!((item.sort_priority() - 2.5).abs() < f64::EPSILON);
		assert_eq!(item.enabled_state(), ENABLED_DEFAULT_ON);
	}

	#[test]
	fn a_popup_shows_its_children_with_their_flags() {
		let list = FakeHandleList::new(&[PATH]);
		let root = item().instantiate(&list);
		let node = root.node();
		assert_eq!(node.display(&list), Some(("Add to test".into(), 0)));
		assert_eq!(node.kind(), TYPE_GROUP);
		let children = node.children();
		let shown: Vec<_> = children.iter().map(|child| child.display(&list)).collect();
		assert_eq!(
			shown,
			[
				Some(("Checked one".into(), FLAG_CHECKED)),
				Some(("Greyed one".into(), FLAG_DISABLED | FLAG_GRAYED)),
				Some(("Plain".into(), 0)),
			]
		);
		assert!(children.iter().all(|child| child.kind() == TYPE_COMMAND));
		assert_eq!(node.child("Plain").map(|child| child.kind()), Some(TYPE_COMMAND));
		assert!(node.child("Missing").is_none());
	}

	#[test]
	fn build_sees_the_selected_paths_in_order() {
		let list = FakeHandleList::new(&[PATH, "file://C:\\a.mp3"]);
		let _root = item().instantiate(&list);
		BUILT_WITH.with_borrow(|built| assert_eq!(built, &[PATH, "file://C:\\a.mp3"]));
	}

	#[test]
	fn an_item_that_builds_nothing_is_hidden() {
		let list = FakeHandleList::new(&[]);
		let root = item().instantiate(&list);
		assert_eq!(root.node().display(&list), None);
	}

	#[test]
	fn executing_a_command_runs_what_build_made_of_the_selection() {
		let list = FakeHandleList::new(&[PATH, "file://C:\\a.mp3"]);
		let root = item().instantiate(&list);
		root.node().child("Plain").unwrap().execute(&FakeHandleList::new(&[])).unwrap();
		assert_eq!(RUNS.get(), 1);
		RAN_WITH.with_borrow(|ran| assert_eq!(ran, &[PATH, "file://C:\\a.mp3"]));
	}

	#[test]
	fn executing_a_popup_runs_nothing() {
		let list = FakeHandleList::new(&[PATH]);
		let root = item().instantiate(&list);
		root.node().execute(&list).unwrap();
		assert_eq!(RUNS.get(), 0);
	}

	#[test]
	fn a_command_that_fails_throws_to_foobar2000() {
		let list = FakeHandleList::new(&[PATH]);
		let root = ContextMenuHandle::new(context_menu_service::<FailingItem>()).instantiate(&list);
		let error = root.node().execute(&list).unwrap_err();
		assert_eq!(error.kind, ExceptionKind::Io);
		assert_eq!(error.message, "can't add");
	}

	#[test]
	fn deleting_the_root_frees_every_node() {
		let dropped = Arc::new(AtomicBool::new(false));
		FLAG.with_borrow_mut(|flag| *flag = Arc::clone(&dropped));
		let list = FakeHandleList::new(&[PATH]);
		let root = ContextMenuHandle::new(context_menu_service::<FlagItem>()).instantiate(&list);
		assert!(!dropped.load(Ordering::SeqCst));
		drop(root);
		assert!(dropped.load(Ordering::SeqCst));
	}

	#[test]
	fn a_menu_shares_its_names_instead_of_copying_them() {
		let name: Arc<str> = Arc::from("Shared");
		SHARED.with_borrow_mut(|shared| *shared = Arc::clone(&name));
		let list = FakeHandleList::new(&[PATH]);
		let root = ContextMenuHandle::new(context_menu_service::<SharedItem>()).instantiate(&list);
		assert_eq!(root.node().display(&list), Some(("Shared".into(), 0)));
		assert_eq!(Arc::strong_count(&name), 3); // `name`, SHARED and the menu
		drop(root);
		assert_eq!(Arc::strong_count(&name), 2);
	}

	#[test]
	fn execute_simple_runs_nothing() {
		item().execute_simple(&Guid::from_u128(7));
		assert_eq!(RUNS.get(), 0);
	}

	struct TestGroup;

	impl ContextMenuGroup for TestGroup {
		const GUID: Guid = Guid::from_u128(0xdeda7cba_d69f_460e_8cd8_283c8adbe1a2);
		const SORT_PRIORITY: f64 = 1.5;
		const NAME: &'static str = "Spotify";
	}

	struct NestedGroup;

	impl ContextMenuGroup for NestedGroup {
		const GUID: Guid = Guid::from_u128(0xdeda7cba_d69f_460e_8cd8_283c8adbe1a3);
		const PARENT: Guid = TestGroup::GUID;
		const NAME: &'static str = "More";
	}

	fn group() -> ContextMenuGroupHandle {
		ContextMenuGroupHandle::new(context_menu_group_service::<TestGroup>())
	}

	#[test]
	fn a_group_answers_as_contextmenu_group_popup_and_contextmenu_group() {
		let group = group();
		assert!(group.answers(&GROUP_POPUP_IID));
		assert!(group.answers(&GROUP_IID));
		assert!(!group.answers(&ITEM_IID));
	}

	#[test]
	fn a_group_takes_guid_parent_priority_and_name_from_the_trait() {
		let group = group();
		assert_eq!(group.guid(), TestGroup::GUID);
		assert_eq!(group.parent(), GROUP_ROOT);
		assert!((group.sort_priority() - 1.5).abs() < f64::EPSILON);
		assert_eq!(group.name(), "Spotify");
		let nested = ContextMenuGroupHandle::new(context_menu_group_service::<NestedGroup>());
		assert_eq!(nested.guid(), NestedGroup::GUID);
		assert_eq!(nested.parent(), TestGroup::GUID);
		assert!(nested.sort_priority().abs() < f64::EPSILON);
		assert_eq!(nested.name(), "More");
	}

	thread_local! {
		static COMMAND_RAN_WITH: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
	}

	/// Shows for Spotify tracks only: hidden with none selected, greyed with any other track.
	struct TestCommand;

	impl ContextMenuCommand for TestCommand {
		const GUID: Guid = Guid::from_u128(0x738b30b4_1f87_4f1a_b9b2_fba251509b50);
		const NAME: &'static str = "Open album";
		const DESCRIPTION: &'static str = "Opens the track's album in a tab.";
		const PARENT: Guid = TestGroup::GUID;
		const SORT_PRIORITY: f64 = 3.0;

		fn shown(selection: &HandleList<'_>) -> Shown {
			match selection.paths() {
				Ok(paths) if paths.is_empty() => Shown::Hidden,
				Ok(paths) if paths.iter().all(|path| path.starts_with("sptf://")) => Shown::Enabled,
				_ => Shown::Greyed,
			}
		}

		fn execute(selection: &HandleList<'_>) -> Result<()> {
			let paths = selection.paths()?;
			COMMAND_RAN_WITH.with_borrow_mut(|ran| ran.push(paths));
			Ok(())
		}
	}

	struct ShortcutOnlyCommand;

	impl ContextMenuCommand for ShortcutOnlyCommand {
		const GUID: Guid = Guid::from_u128(0x738b30b4_1f87_4f1a_b9b2_fba251509b51);
		const NAME: &'static str = "Open artist…";
		const DESCRIPTION: &'static str = "Asks which artist to open.";
		const SHORTCUT_ONLY: bool = true;

		fn shown(_selection: &HandleList<'_>) -> Shown {
			Shown::Enabled
		}

		fn execute(_selection: &HandleList<'_>) -> Result<()> {
			Ok(())
		}
	}

	struct FailingCommand;

	impl ContextMenuCommand for FailingCommand {
		const GUID: Guid = Guid::from_u128(0x738b30b4_1f87_4f1a_b9b2_fba251509b52);
		const NAME: &'static str = "Fail";
		const DESCRIPTION: &'static str = "Always fails.";

		fn shown(_selection: &HandleList<'_>) -> Shown {
			Shown::Enabled
		}

		fn execute(_selection: &HandleList<'_>) -> Result<()> {
			Err(Error::new(ExceptionKind::Io, "can't open"))
		}
	}

	fn open_album() -> ContextMenuHandle {
		ContextMenuHandle::new(context_menu_command_service::<TestCommand>())
	}

	#[test]
	fn a_command_answers_as_contextmenu_item_v2_and_contextmenu_item() {
		let command = open_album();
		assert!(command.answers(&ITEM_V2_IID));
		assert!(command.answers(&ITEM_IID));
		assert!(!command.answers(&GROUP_IID));
	}

	#[test]
	fn a_command_takes_name_guid_description_parent_and_priority_from_the_trait() {
		let command = open_album();
		assert_eq!(command.item_count(), 1);
		assert_eq!(command.guid(), TestCommand::GUID);
		assert_eq!(command.name(), "Open album");
		assert_eq!(command.default_path(), "");
		assert_eq!(command.description().as_deref(), Some("Opens the track's album in a tab."));
		assert_eq!(command.parent(), TestGroup::GUID);
		assert!((command.sort_priority() - 3.0).abs() < f64::EPSILON);
		assert_eq!(command.enabled_state(), ENABLED_DEFAULT_ON);
		let defaults = ContextMenuHandle::new(context_menu_command_service::<FailingCommand>());
		assert_eq!(defaults.parent(), GROUP_ROOT);
		assert!(defaults.sort_priority().abs() < f64::EPSILON);
	}

	#[test]
	fn a_command_shows_as_its_trait_says_for_each_selection() {
		let tracks = FakeHandleList::new(&[PATH]);
		let root = open_album().instantiate(&tracks);
		let node = root.node();
		assert_eq!(node.display(&FakeHandleList::new(&[])), None);
		assert_eq!(
			node.display(&FakeHandleList::new(&[PATH, "file://C:\\a.mp3"])),
			Some(("Open album".into(), FLAG_DISABLED | FLAG_GRAYED))
		);
		assert_eq!(node.display(&tracks), Some(("Open album".into(), 0)));
	}

	#[test]
	fn a_command_is_a_mappable_shortcut() {
		let list = FakeHandleList::new(&[PATH]);
		let root = open_album().instantiate(&list);
		let node = root.node();
		assert!(node.is_mappable_shortcut());
		assert_eq!(node.kind(), TYPE_COMMAND);
		assert_eq!(node.guid(), Guid::NULL);
		assert!(node.children().is_empty());
		assert_eq!(node.description().as_deref(), Some("Opens the track's album in a tab."));
	}

	#[test]
	fn picking_a_command_runs_it_on_the_tracks_it_passes() {
		let root = open_album().instantiate(&FakeHandleList::new(&[PATH]));
		root.node().execute(&FakeHandleList::new(&[PATH, "file://C:\\a.mp3"])).unwrap();
		assert_eq!(COMMAND_RAN_WITH.take(), [[PATH, "file://C:\\a.mp3"]]);
	}

	#[test]
	fn a_shortcut_runs_the_command_on_the_tracks_it_passes() {
		let list = FakeHandleList::new(&[PATH, "file://C:\\a.mp3"]);
		open_album().execute_simple_on(&Guid::NULL, &list).unwrap();
		assert_eq!(COMMAND_RAN_WITH.take(), [[PATH, "file://C:\\a.mp3"]]);
	}

	#[test]
	fn a_shortcut_for_another_node_runs_nothing() {
		open_album().execute_simple_on(&Guid::from_u128(7), &FakeHandleList::new(&[PATH])).unwrap();
		assert!(COMMAND_RAN_WITH.take().is_empty());
	}

	#[test]
	fn a_shortcut_only_command_is_forced_off() {
		let command = ContextMenuHandle::new(context_menu_command_service::<ShortcutOnlyCommand>());
		assert_eq!(command.enabled_state(), ENABLED_FORCE_OFF);
		assert_eq!(command.name(), "Open artist…");
	}

	#[test]
	fn a_failing_command_throws_to_foobar2000() {
		let list = FakeHandleList::new(&[PATH]);
		let command = ContextMenuHandle::new(context_menu_command_service::<FailingCommand>());
		let root = command.instantiate(&list);
		let picked = root.node().execute(&list).unwrap_err();
		assert_eq!(picked.kind, ExceptionKind::Io);
		assert_eq!(picked.message, "can't open");
		let shortcut = command.execute_simple_on(&Guid::NULL, &list).unwrap_err();
		assert_eq!(shortcut.kind, ExceptionKind::Io);
		assert_eq!(shortcut.message, "can't open");
	}
}
