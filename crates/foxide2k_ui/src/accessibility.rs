//! Making status text heard: polite live regions and their change events.

use std::mem::ManuallyDrop;

use windows_core::GUID;

use crate::bindings::Windows::Win32::{
	CHILDID_SELF, CLSCTX_INPROC_SERVER, CLSID_AccPropServices, CoCreateInstance,
	EVENT_OBJECT_LIVEREGIONCHANGED, IAccPropServices, NotifyWinEvent, OBJID_CLIENT, Polite,
	VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VARTYPE, VT_I4,
};
use crate::error::{Error, Result};
use crate::host::Dialog;

/// UI Automation's `LiveSetting_Property_GUID` (`UIAutomationClient.h`), which is not in the
/// Win32 metadata.
const LIVE_SETTING_PROPERTY: GUID = GUID::from_u128(0xc12bcd8e_2a8e_4950_8ae7_3625111d58eb);

/// Marks control `id` as a polite live region, so screen readers read its new text after an
/// [`announce`] without moving focus. Needs COM on the calling thread.
pub fn make_live_region(dialog: &Dialog, id: u16) -> Result<()> {
	let fail = |error: windows_core::Error| Error {
		message: format!("can't make a live region: {error}"),
	};
	let services: IAccPropServices =
		unsafe { CoCreateInstance(&CLSID_AccPropServices, None, CLSCTX_INPROC_SERVER) }
			.map_err(fail)?;
	#[allow(clippy::cast_possible_truncation, reason = "VT_I4 is 3")]
	let polite = VARIANT {
		Anonymous: VARIANT_0 {
			Anonymous: ManuallyDrop::new(VARIANT_0_0 {
				vt: VARTYPE(VT_I4 as u16),
				wReserved1: 0,
				wReserved2: 0,
				wReserved3: 0,
				Anonymous: VARIANT_0_0_0 { lVal: Polite },
			}),
		},
	};
	unsafe {
		services.SetHwndProp(
			dialog.control(id),
			OBJID_CLIENT.cast_unsigned(),
			CHILDID_SELF as u32,
			LIVE_SETTING_PROPERTY,
			&polite,
		)
	}
	.ok()
	.map_err(fail)
}

/// Tells screen readers that live region `id` has new text.
pub fn announce(dialog: &Dialog, id: u16) {
	unsafe {
		NotifyWinEvent(
			EVENT_OBJECT_LIVEREGIONCHANGED as u32,
			dialog.control(id),
			OBJID_CLIENT,
			CHILDID_SELF,
		);
	}
}

#[cfg(test)]
mod tests {
	use super::{announce, make_live_region};
	use crate::host::{create_child_dialog, destroy};
	use crate::template::{ControlClass, DialogItem, DialogTemplate, shell_font};
	use crate::testing::hidden_window;

	struct Nothing;

	impl crate::host::DialogHandler for Nothing {
		fn init(&mut self, _dialog: &crate::host::Dialog) -> Option<u16> {
			None
		}

		fn command(&mut self, _dialog: &crate::host::Dialog, _id: u16, _notification: u16) {}

		fn refresh(&mut self, _dialog: &crate::host::Dialog) {}
	}

	#[test]
	fn live_region_can_be_set_on_a_static() {
		let template = DialogTemplate {
			style: crate::bindings::Windows::Win32::WS_CHILD as u32,
			title: String::new(),
			cx: 100,
			cy: 30,
			font: shell_font(),
			items: vec![DialogItem {
				id: 101,
				class: ControlClass::Static,
				text: "Not logged in to Spotify.".to_owned(),
				style: 0,
				x: 7,
				y: 7,
				cx: 86,
				cy: 16,
			}],
		};
		let dialog = create_child_dialog(hidden_window(), &template, Box::new(Nothing)).unwrap();
		make_live_region(&dialog, 101).unwrap();
		announce(&dialog, 101);
		destroy(dialog);
	}
}
