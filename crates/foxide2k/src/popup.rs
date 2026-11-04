//! foobar2000's non-modal message window.

use crate::abi::popup::{ICON_ERROR, ICON_INFORMATION, IID, PopupMessageVtbl};
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

#[cfg(test)]
mod tests {
	use super::{PopupIcon, show};
	use crate::testing::FakeApi;

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
