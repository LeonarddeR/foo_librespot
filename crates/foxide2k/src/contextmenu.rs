//! Right-click menu items whose submenus are built each time the menu opens.

use std::ffi::c_void;
use std::marker::PhantomData;

use crate::abi::contextmenu::{
	ENABLED_DEFAULT_ON, FLAG_CHECKED, FLAG_DISABLED, FLAG_GRAYED, GROUP_ROOT, ITEM_IID,
	ITEM_V2_IID, ItemVtbl, Node, NodeVtbl, TYPE_COMMAND, TYPE_GROUP,
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

/// What a command does: runs on the main thread with the selected items' paths.
pub type RunFn = Box<dyn Fn(&[String]) -> Result<()> + Send + Sync>;

/// What a right-click menu item shows, built each time the menu opens.
pub enum MenuNode {
	Command {
		name: String,
		checked: bool,
		/// False shows it greyed out, and it can't be picked.
		enabled: bool,
		/// Runs on the main thread with the selected items' paths.
		run: RunFn,
	},
	Popup {
		name: String,
		children: Vec<Self>,
	},
}

pub trait ContextMenuItem: 'static {
	const GUID: Guid;
	/// Its name in the keyboard shortcut list and in Preferences > Context Menu.
	const NAME: &'static str;
	const DESCRIPTION: &'static str;
	const PARENT: Guid = GROUP_ROOT;
	const SORT_PRIORITY: f64 = 0.0;
	/// The node to show for the selected items' paths; `None` hides the item. Runs on the main
	/// thread while the menu opens, so it must not wait for anything.
	fn build(paths: &[String]) -> Option<MenuNode>;
}

/// The factory that registers item `T` with foobar2000.
pub const fn context_menu_factory<T: ContextMenuItem>() -> Factory {
	Factory::per_instance(&ITEM_IID, context_menu_service::<T>)
}

/// A new `contextmenu_item_v2` service for `T`.
pub fn context_menu_service<T: ContextMenuItem>() -> ServicePtr {
	new_service(Item::<T>(PhantomData))
}

struct Item<T>(PhantomData<fn() -> T>);

unsafe impl<T: ContextMenuItem> ServiceImpl for Item<T> {
	type Vtbl = ItemVtbl;
	const IIDS: &'static [Guid] = &[ITEM_V2_IID, ITEM_IID];
	const VTBL: &'static ItemVtbl = &ItemVtbl {
		base: ServiceBaseVtbl::new::<Self>(),
		get_num_items: item_count,
		instantiate_item: item_instantiate::<T>,
		get_item_guid: item_guid::<T>,
		get_item_name: item_name::<T>,
		get_item_default_path: item_default_path,
		get_item_description: item_description::<T>,
		get_enabled_state: item_enabled_state,
		item_execute_simple,
		get_sort_priority: item_sort_priority::<T>,
		get_parent: item_parent::<T>,
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
			let kind = HandleList::from_raw(data)
				.paths()
				.ok()
				.and_then(|paths| T::build(&paths))
				.map_or(NodeKind::Hidden, NodeKind::from);
			Ok(Box::into_raw(Box::new(NodeObject { vtbl: &raw const NODE_VTBL, kind }))
				.cast::<Node>())
		})
	}
}

unsafe extern "C-unwind" fn item_guid<T: ContextMenuItem>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
	_index: u32,
) -> *mut Guid {
	unsafe { ret.write(T::GUID) };
	ret
}

unsafe extern "C-unwind" fn item_name<T: ContextMenuItem>(
	_this: *mut ServiceBase,
	_index: u32,
	out: *mut StringBase,
) {
	unsafe { guard(|| set_string(out, T::NAME)) }
}

unsafe extern "C-unwind" fn item_default_path(
	_this: *mut ServiceBase,
	_index: u32,
	out: *mut StringBase,
) {
	unsafe { guard(|| set_string(out, "")) }
}

unsafe extern "C-unwind" fn item_description<T: ContextMenuItem>(
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

unsafe extern "C-unwind" fn item_enabled_state(_this: *mut ServiceBase, _index: u32) -> i32 {
	ENABLED_DEFAULT_ON
}

unsafe extern "C-unwind" fn item_execute_simple(
	_this: *mut ServiceBase,
	_index: u32,
	_node: *const Guid,
	_data: *const HandleListRaw,
	_caller: *const Guid,
) {
}

unsafe extern "C-unwind" fn item_sort_priority<T: ContextMenuItem>(_this: *mut ServiceBase) -> f64 {
	T::SORT_PRIORITY
}

unsafe extern "C-unwind" fn item_parent<T: ContextMenuItem>(
	_this: *mut ServiceBase,
	ret: *mut Guid,
) -> *mut Guid {
	unsafe { ret.write(T::PARENT) };
	ret
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
		name: String,
		flags: u32,
		run: RunFn,
	},
	Popup {
		name: String,
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
	data: *const HandleListRaw,
	_caller: *const Guid,
) {
	unsafe {
		guard(|| {
			if let NodeKind::Command { run, .. } = kind(this) {
				run(&HandleList::from_raw(data).paths()?)?;
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
	unsafe { ret.write(Guid::from_u128(0)) };
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

#[cfg(test)]
mod tests {
	use std::cell::{Cell, RefCell};
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, Ordering};

	use super::{ContextMenuItem, MenuNode, context_menu_service};
	use crate::abi::contextmenu::{
		ENABLED_DEFAULT_ON, FLAG_CHECKED, FLAG_DISABLED, FLAG_GRAYED, GROUP_ROOT, ITEM_IID,
		ITEM_V2_IID, TYPE_COMMAND, TYPE_GROUP,
	};
	use crate::cxx::ExceptionKind;
	use crate::error::Error;
	use crate::guid::Guid;
	use crate::testing::{ContextMenuHandle, FakeHandleList};

	thread_local! {
		static BUILT_WITH: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
		static RAN_WITH: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
		static RUNS: Cell<usize> = const { Cell::new(0) };
		static FLAG: RefCell<Arc<AtomicBool>> = RefCell::new(Arc::new(AtomicBool::new(false)));
	}

	fn command(name: &str, checked: bool, enabled: bool) -> MenuNode {
		MenuNode::Command {
			name: name.into(),
			checked,
			enabled,
			run: Box::new(|paths| {
				RUNS.set(RUNS.get() + 1);
				RAN_WITH.with_borrow_mut(|ran| *ran = paths.to_vec());
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

		fn build(paths: &[String]) -> Option<MenuNode> {
			BUILT_WITH.with_borrow_mut(|built| *built = paths.to_vec());
			if paths.is_empty() {
				return None;
			}
			Some(MenuNode::Popup {
				name: "Add to test".into(),
				children: vec![
					command("Checked one", true, true),
					command("Greyed one", false, false),
					command("Plain", false, true),
				],
			})
		}
	}

	struct FailingItem;

	impl ContextMenuItem for FailingItem {
		const GUID: Guid = Guid::from_u128(0x7d2c91a4_3b58_4e06_9f13_c8a5e0b64d73);
		const NAME: &'static str = "Fail";
		const DESCRIPTION: &'static str = "Always fails.";

		fn build(_paths: &[String]) -> Option<MenuNode> {
			Some(MenuNode::Command {
				name: "Fail".into(),
				checked: false,
				enabled: true,
				run: Box::new(|_| Err(Error::new(ExceptionKind::Io, "can't add"))),
			})
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

		fn build(_paths: &[String]) -> Option<MenuNode> {
			let flag = DropFlag(FLAG.with_borrow(Arc::clone));
			Some(MenuNode::Popup {
				name: "Flag".into(),
				children: vec![MenuNode::Command {
					name: "Child".into(),
					checked: false,
					enabled: true,
					run: Box::new(move |_| {
						let _held = &flag;
						Ok(())
					}),
				}],
			})
		}
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
	fn executing_a_command_runs_it_with_the_selection() {
		let list = FakeHandleList::new(&[PATH, "file://C:\\a.mp3"]);
		let root = item().instantiate(&list);
		root.node().child("Plain").unwrap().execute(&list).unwrap();
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
	fn execute_simple_runs_nothing() {
		item().execute_simple(&Guid::from_u128(7));
		assert_eq!(RUNS.get(), 0);
	}
}
