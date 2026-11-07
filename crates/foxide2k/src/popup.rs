//! foobar2000's non-modal message window.

use std::ffi::{CString, c_void};

use crate::abi::popup::{
	BUTTON_NO, BUTTON_YES, ICON_ERROR, ICON_INFORMATION, ICON_QUESTION, IID, PopupMessageVtbl,
	PopupV3Vtbl, Query, V3_IID,
};
use crate::cxx::{ExceptionKind, try_host};
use crate::error::{Error, Result};
use crate::host::host;

/// The icon a popup shows next to its message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupIcon {
	Information,
	Error,
}

impl PopupIcon {
	pub(crate) const fn raw(self) -> u32 {
		match self {
			Self::Information => ICON_INFORMATION,
			Self::Error => ICON_ERROR,
		}
	}

	#[cfg(any(test, feature = "testing"))]
	pub(crate) const fn from_raw(raw: u32) -> Self {
		if raw == ICON_ERROR { Self::Error } else { Self::Information }
	}
}

/// Shows `message` in a popup titled `title`. Safe from any thread: foobar2000 shows it on the
/// main thread.
pub fn show(title: &str, message: &str, icon: PopupIcon) -> Result<()> {
	let popup = host()
		.core_service(&IID)
		.ok_or_else(|| Error::new(ExceptionKind::BugCheck, "foobar2000 has no popup_message"))?;
	let raw = popup.as_raw();
	try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<PopupMessageVtbl>();
		(vtbl.show_ex)(
			raw,
			message.as_ptr().cast(),
			message.len(),
			title.as_ptr().cast(),
			title.len(),
			icon.raw(),
		);
	})?;
	Ok(())
}

/// Asks a yes/no question in foobar2000's message window and waits for the answer. Returns true
/// for Yes; No is the default button. Main thread only.
pub fn ask(title: &str, message: &str) -> Result<bool> {
	let popup = host()
		.core_service(&V3_IID)
		.ok_or_else(|| Error::new(ExceptionKind::BugCheck, "foobar2000 has no popup_message_v3"))?;
	let nul = |_| Error::new(ExceptionKind::InvalidParams, "a question contains a NUL");
	let title = CString::new(title).map_err(nul)?;
	let message = CString::new(message).map_err(nul)?;
	let query = Query {
		title: title.as_ptr(),
		message: message.as_ptr(),
		buttons: BUTTON_YES | BUTTON_NO,
		default_button: BUTTON_NO,
		icon: ICON_QUESTION,
		reply: std::ptr::null_mut(),
		parent: host().main_window() as *mut c_void,
		do_not_ask_again: std::ptr::null(),
	};
	let raw = popup.as_raw();
	let button = try_host(|| unsafe {
		let vtbl = &*(*raw).vtbl.cast::<PopupV3Vtbl>();
		(vtbl.show_query_modal)(raw, &raw const query)
	})?;
	Ok(button == BUTTON_YES)
}

#[cfg(test)]
mod tests {
	use super::{PopupIcon, ask, show};
	use crate::abi::popup::BUTTON_NO;
	use crate::testing::FakeApi;

	#[test]
	fn a_question_shows_yes_and_no_with_no_first() {
		let api = FakeApi::new();
		api.install();
		api.answer_questions(true);
		let answer =
			ask("Remove from Spotify playlist", "Remove the selected track from Sunday set?");
		assert!(answer.unwrap());
		assert_eq!(
			api.questions(),
			[(
				"Remove from Spotify playlist".to_owned(),
				"Remove the selected track from Sunday set?".to_owned(),
				BUTTON_NO
			)]
		);
	}

	#[test]
	fn a_question_answered_no_is_false() {
		let api = FakeApi::new();
		api.install();
		assert!(!ask("Remove from Spotify playlist", "Remove it?").unwrap());
		api.answer_questions(false);
		assert!(!ask("Remove from Spotify playlist", "Remove it?").unwrap());
	}

	#[test]
	fn popup_reaches_the_host_once_with_title_message_and_icon() {
		let api = FakeApi::new();
		api.install();
		show("foo_librespot", "Added 12 tracks from Living Hope.", PopupIcon::Error).unwrap();
		assert_eq!(
			api.popups(),
			[(
				"foo_librespot".to_owned(),
				"Added 12 tracks from Living Hope.".to_owned(),
				PopupIcon::Error
			)]
		);
	}
}
