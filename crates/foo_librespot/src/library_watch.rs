//! What the windows that list the Spotify library share: each change of the library or of the
//! login refreshes them, and one that finds the account logged out asks for a login, once.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use foo_librespot_core::library::LibraryState;
use foxide2k::dialog::host::{Dialog, refresh_poster};

use crate::backend::backend;
use crate::first_use::ask_for_login;
use crate::login::controller;

/// A window's subscriptions to the library and the login, made by [`start`](Self::start) and
/// dropped with it.
#[derive(Default)]
pub struct LibraryWatch {
	login_subscription: Option<u64>,
	library_subscription: Option<u64>,
	/// Set while the window hasn't learnt whether the account is logged in and the login hasn't
	/// changed since it opened: a logged-out answer then asks for a login.
	ask_when_logged_out: Arc<AtomicBool>,
}

impl LibraryWatch {
	/// Makes each change of the library or the login refresh `dialog`, and answers what the
	/// window shows first. Logged out, it asks for a login. A read that runs or failed shows as
	/// `Loading`, as asking starts another; when that one finds the account logged out, the
	/// refresh asks for a login, unless the login changed meanwhile.
	pub fn start(&mut self, dialog: Dialog) -> LibraryState {
		let (poster, ask) = (refresh_poster(&dialog), Arc::clone(&self.ask_when_logged_out));
		self.login_subscription = Some(controller().subscribe(Box::new(move || {
			ask.store(false, Ordering::SeqCst);
			poster.post();
		})));
		let poster = refresh_poster(&dialog);
		self.library_subscription =
			Some(backend().subscribe_library(Box::new(move || poster.post())));
		match backend().library() {
			LibraryState::LoggedOut => {
				ask_for_login();
				LibraryState::LoggedOut
			}
			LibraryState::Loading | LibraryState::Failed(_) => {
				self.ask_when_logged_out.store(true, Ordering::SeqCst);
				LibraryState::Loading
			}
			state @ LibraryState::Ready(_) => state,
		}
	}

	/// What the window shows on a refresh: the library held, or what a new read answers while
	/// none is. Asks for a login when [`start`](Self::start) left that to a refresh and the
	/// account is logged out.
	pub fn refreshed(&self) -> LibraryState {
		let state = match backend().held_library() {
			LibraryState::Loading => backend().library(),
			state => state,
		};
		match state {
			LibraryState::Loading => {}
			LibraryState::LoggedOut if self.ask_when_logged_out.swap(false, Ordering::SeqCst) => {
				ask_for_login();
			}
			_ => self.ask_when_logged_out.store(false, Ordering::SeqCst),
		}
		state
	}
}

/// What the status line of a window that lists the library says for `state`: nothing once the
/// library is ready, "Loading…" while it loads, why it couldn't load, or that the user should
/// log in.
pub fn status(state: &LibraryState) -> String {
	match state {
		LibraryState::Ready(_) => String::new(),
		LibraryState::Loading => "Loading…".to_owned(),
		LibraryState::Failed(reason) => format!("Couldn't load your playlists: {reason}."),
		LibraryState::LoggedOut => "Log in to Spotify to see your playlists.".to_owned(),
	}
}

impl Drop for LibraryWatch {
	/// Runs when the window is destroyed, also when foobar2000 quits with it open.
	fn drop(&mut self) {
		if let Some(subscription) = self.login_subscription {
			controller().unsubscribe(subscription);
		}
		if let Some(subscription) = self.library_subscription {
			backend().unsubscribe_library(subscription);
		}
	}
}
