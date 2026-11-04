//! Asking for a Spotify login the first time a link needs one: the import or the play that
//! found the user logged out starts a login and opens the login window. Nothing opens the
//! browser; the window has an "Open browser" button for that.

use std::sync::OnceLock;

use foo_librespot_core::CoreError;
use oxibar::console;
use oxibar::host::host;
use oxibar::main_thread;

use crate::login::controller;
use crate::login_window;

/// Shows the login window. Called from any thread.
pub trait LoginPrompt: Send + Sync {
	fn show(&self);
}

/// Opens the login window on foobar2000's main thread, owned by its main window.
struct WindowPrompt;

impl LoginPrompt for WindowPrompt {
	fn show(&self) {
		let shown = main_thread::run(Box::new(|| {
			if let Err(error) = login_window::show(host().main_window()) {
				console::print(&format!(
					"foo_librespot: can't open the login window: {}",
					error.message
				));
			}
		}));
		if let Err(error) = shown {
			console::print(&format!(
				"foo_librespot: can't open the login window: {}",
				error.message
			));
		}
	}
}

static PROMPT: OnceLock<Box<dyn LoginPrompt>> = OnceLock::new();

/// The prompt in use: the login window, or the one a test installed.
pub fn prompt() -> &'static dyn LoginPrompt {
	PROMPT.get_or_init(|| Box::new(WindowPrompt)).as_ref()
}

/// Makes `prompt()` return `prompt` in this test process.
#[cfg(test)]
pub fn install_prompt(prompt: Box<dyn LoginPrompt>) {
	assert!(PROMPT.set(prompt).is_ok(), "a prompt was already in use");
}

/// Starts a login, unless one is running, and shows the login window.
pub fn prompt_login() {
	controller().start();
	prompt().show();
}

/// Starts a login and waits until the user confirms it, cancels it, or foobar2000 aborts.
/// On the main thread it only shows the window: waiting there would freeze the window it waits
/// for.
pub fn login_on_first_use(is_aborting: &dyn Fn() -> bool) -> Result<(), CoreError> {
	prompt_login();
	if host().is_main_thread() {
		return Err(CoreError::NotLoggedIn);
	}
	controller().wait_for_login(is_aborting).map(drop)
}

/// [`login_on_first_use`] for a play that found the user logged out. Once the user has
/// cancelled a login, playing fails without asking again, so a playlist doesn't reopen the
/// window for each of its tracks; adding a link or pressing Log in asks again.
pub fn login_to_play(is_aborting: &dyn Fn() -> bool) -> Result<(), CoreError> {
	if controller().declined() {
		return Err(CoreError::NotLoggedIn);
	}
	login_on_first_use(is_aborting)
}
