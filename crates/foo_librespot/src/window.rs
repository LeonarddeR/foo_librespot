//! The windows the component opens on its own: at most one of each kind, on foobar2000's main
//! thread.

use std::any::TypeId;
use std::cell::RefCell;
use std::marker::PhantomData;

use foxide2k::error::Result;
use foxide2k::host::host;
use foxide2k::main_thread::{add_modeless, remove_modeless};
use foxide2k_ui::host::{Dialog, DialogHandler, create_modeless_dialog, refresh_poster};
use foxide2k_ui::template::DialogTemplate;

use crate::errors::{from_ui, report_failure};

thread_local! {
	/// The open window of each kind, by the type of its handler. Only foobar2000's main thread
	/// sets and reads it.
	static OPEN: RefCell<Vec<(TypeId, Dialog)>> = const { RefCell::new(Vec::new()) };
}

/// A window of which at most one is open, made from `template` with a new `H` as its handler.
pub struct SingleWindow<H> {
	template: fn() -> DialogTemplate,
	handler: PhantomData<fn() -> H>,
}

impl<H: DialogHandler + Default> SingleWindow<H> {
	pub const fn new(template: fn() -> DialogTemplate) -> Self {
		Self { template, handler: PhantomData }
	}

	/// The open window, if any.
	#[expect(clippy::unused_self, reason = "called on the window's static, as open and show are")]
	pub fn current(&self) -> Option<Dialog> {
		let key = TypeId::of::<H>();
		OPEN.with_borrow(|open| {
			open.iter().find(|(open, _)| *open == key).map(|(_, dialog)| *dialog)
		})
		.filter(Dialog::is_alive)
	}

	/// Creates the window, hidden, unless it is open already. Main thread only.
	pub fn open(&self, owner: isize) -> Result<Dialog> {
		if let Some(dialog) = self.current() {
			return Ok(dialog);
		}
		let key = TypeId::of::<H>();
		let handler = Box::new(Single { handler: H::default(), key, dialog: None });
		let dialog = create_modeless_dialog(owner, &(self.template)(), handler).map_err(from_ui)?;
		OPEN.with_borrow_mut(|open| {
			open.retain(|(open, _)| *open != key);
			open.push((key, dialog));
		});
		add_modeless(dialog.hwnd());
		Ok(dialog)
	}

	/// Shows the window, or brings it to the front when it is open. Main thread only.
	pub fn show(&self, owner: isize) -> Result<()> {
		bring_up(self.open(owner)?);
		Ok(())
	}

	/// Brings the open window up to date with a refresh, or opens it, owned by foobar2000's main
	/// window, and brings it up. When it can't open, says in the console "`failed`: why", and
	/// shows the window's title and why in an error popup titled `title`. Main thread only.
	pub fn show_fresh(&self, failed: &str, title: &str) {
		if let Some(dialog) = self.current() {
			refresh_poster(&dialog).post();
		}
		if let Err(error) = self.show(host().main_window()) {
			report_failure(failed, title, &(self.template)().title, &error.message);
		}
	}

	/// What `look` finds in the open window's handler, run on it as the window's own messages
	/// are; `None` while the window is closed.
	#[cfg(test)]
	pub fn inspect<R>(&self, look: impl FnOnce(&H, Dialog) -> R) -> Option<R> {
		let dialog = self.current()?;
		foxide2k_ui::testing::with_handler(&dialog, |handler, dialog| {
			let handler: &dyn std::any::Any = handler;
			handler.downcast_ref::<Single<H>>().map(|single| look(&single.handler, *dialog))
		})
		.flatten()
	}
}

/// Makes `dialog` visible and brings it to the front.
#[cfg(not(test))]
fn bring_up(dialog: Dialog) {
	dialog.show();
	dialog.activate();
}

#[cfg(test)]
thread_local! {
	/// How often a window was brought up on this thread.
	static BROUGHT_UP: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Only counts, so that no test shows a window or takes the keyboard focus.
#[cfg(test)]
fn bring_up(_dialog: Dialog) {
	BROUGHT_UP.set(BROUGHT_UP.get() + 1);
}

/// How often [`SingleWindow::show`] brought a window up on this thread.
#[cfg(test)]
pub fn brought_up() -> usize {
	BROUGHT_UP.get()
}

/// The window's own handler, and the window to forget once it is destroyed.
struct Single<H> {
	handler: H,
	/// The key of the window's kind in [`OPEN`].
	key: TypeId,
	dialog: Option<Dialog>,
}

impl<H: DialogHandler> DialogHandler for Single<H> {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		self.dialog = Some(*dialog);
		self.handler.init(dialog)
	}

	fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
		self.handler.command(dialog, id, notification);
	}

	fn refresh(&mut self, dialog: &Dialog) {
		self.handler.refresh(dialog);
	}

	fn notify(&mut self, dialog: &Dialog, id: u16, code: i32) {
		self.handler.notify(dialog, id, code);
	}
}

impl<H> Drop for Single<H> {
	/// Runs when the window is destroyed, also when foobar2000 quits with it open.
	fn drop(&mut self) {
		if let Some(dialog) = self.dialog {
			remove_modeless(dialog.hwnd());
			let entry = (self.key, dialog);
			OPEN.with_borrow_mut(|open| open.retain(|open| *open != entry));
		}
	}
}

#[cfg(test)]
mod tests {
	use foxide2k::popup::PopupIcon;
	use foxide2k_ui::host::{Dialog, DialogHandler, destroy};
	use foxide2k_ui::notify::NM_DBLCLK;
	use foxide2k_ui::style::{WS_CAPTION, WS_POPUP};
	use foxide2k_ui::template::{DialogTemplate, shell_font};
	use foxide2k_ui::testing::{hidden_window, pump_messages, send_close, send_notify};

	use super::{SingleWindow, brought_up};
	use crate::testing::installed_api;

	fn template() -> DialogTemplate {
		DialogTemplate {
			style: WS_POPUP | WS_CAPTION,
			title: "A window".to_owned(),
			cx: 100,
			cy: 50,
			font: shell_font(),
			items: Vec::new(),
		}
	}

	/// Closes its window on Escape and the close box, counts its refreshes and keeps its
	/// notifications.
	#[derive(Default)]
	struct Closing {
		refreshes: usize,
		notifications: Vec<(u16, i32)>,
	}

	impl DialogHandler for Closing {
		fn init(&mut self, _dialog: &Dialog) -> Option<u16> {
			None
		}

		fn command(&mut self, dialog: &Dialog, id: u16, _notification: u16) {
			if id == foxide2k_ui::notify::IDCANCEL {
				destroy(*dialog);
			}
		}

		fn refresh(&mut self, _dialog: &Dialog) {
			self.refreshes += 1;
		}

		fn notify(&mut self, _dialog: &Dialog, id: u16, code: i32) {
			self.notifications.push((id, code));
		}
	}

	#[derive(Default)]
	struct Other(Closing);

	impl DialogHandler for Other {
		fn init(&mut self, dialog: &Dialog) -> Option<u16> {
			self.0.init(dialog)
		}

		fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
			self.0.command(dialog, id, notification);
		}

		fn refresh(&mut self, dialog: &Dialog) {
			self.0.refresh(dialog);
		}
	}

	static ONE: SingleWindow<Closing> = SingleWindow::new(template);
	static OTHER: SingleWindow<Other> = SingleWindow::new(template);

	#[test]
	fn opening_twice_keeps_one_window_of_each_kind() {
		let owner = hidden_window();
		let first = ONE.open(owner).unwrap();
		assert_eq!(ONE.open(owner).unwrap(), first);
		assert_eq!(ONE.current(), Some(first));
		let other = OTHER.open(owner).unwrap();
		assert_ne!(other, first);
		assert_eq!(OTHER.current(), Some(other));
		destroy(first);
		assert!(ONE.current().is_none());
		assert_eq!(OTHER.current(), Some(other));
		destroy(other);
	}

	#[test]
	fn showing_opens_the_window_once_and_brings_it_up_each_time() {
		let owner = hidden_window();
		ONE.show(owner).unwrap();
		let shown = ONE.current().expect("showing opens the window");
		ONE.show(owner).unwrap();
		assert_eq!(ONE.current(), Some(shown));
		assert_eq!(brought_up(), 2);
		destroy(shown);
	}

	#[test]
	fn showing_fresh_refreshes_the_open_window_and_brings_it_up() {
		installed_api().set_main_window(hidden_window());
		ONE.show_fresh("can't open the window", "Can't open it");
		let shown = ONE.current().expect("showing opens the window");
		assert_eq!(ONE.inspect(|one, _| one.refreshes), Some(0));
		ONE.show_fresh("can't open the window", "Can't open it");
		pump_messages();
		assert_eq!(ONE.current(), Some(shown));
		assert_eq!(ONE.inspect(|one, _| one.refreshes), Some(1));
		assert_eq!(brought_up(), 2);
		destroy(shown);
	}

	#[test]
	fn a_window_that_cant_open_says_why_in_the_console_and_an_error_popup() {
		let api = installed_api();
		let gone = hidden_window();
		destroy(Dialog::from_hwnd(gone));
		api.set_main_window(gone);
		ONE.show_fresh("can't open the window", "Can't open it");
		assert!(ONE.current().is_none());
		assert_eq!(brought_up(), 0);
		let popups = api.popups();
		assert_eq!(popups.len(), 1, "{popups:?}");
		let (title, message, icon) = &popups[0];
		assert_eq!(title, "Can't open it");
		assert!(message.starts_with("A window\n\ncan't create a dialog: "), "{message}");
		assert_eq!(*icon, PopupIcon::Error);
		let console = api.console_lines();
		assert!(
			console.iter().any(|line| {
				line.starts_with("foo_librespot: can't open the window: can't create a dialog: ")
			}),
			"{console:?}"
		);
	}

	#[test]
	fn inspecting_reaches_the_open_windows_handler() {
		let owner = hidden_window();
		assert!(ONE.inspect(|_, _| ()).is_none());
		let dialog = ONE.open(owner).unwrap();
		assert_eq!(ONE.inspect(|_, seen| seen), Some(dialog));
		assert!(dialog.is_alive());
		destroy(dialog);
	}

	#[test]
	fn notifications_reach_the_windows_own_handler() {
		let dialog = ONE.open(hidden_window()).unwrap();
		send_notify(&dialog, 7, NM_DBLCLK);
		assert_eq!(ONE.inspect(|one, _| one.notifications.clone()), Some(vec![(7, NM_DBLCLK)]));
		destroy(dialog);
	}

	#[test]
	fn a_window_closed_by_the_user_is_forgotten_and_opens_anew() {
		let owner = hidden_window();
		let first = ONE.open(owner).unwrap();
		send_close(&first);
		pump_messages();
		assert!(!first.is_alive());
		assert!(ONE.current().is_none());
		let again = ONE.open(owner).unwrap();
		assert!(again.is_alive());
		destroy(again);
	}
}
