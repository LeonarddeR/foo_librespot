//! The foo_librespot page under Preferences > Tools: the Spotify login and the bitrate.

use std::ffi::CStr;

use foo_librespot_core::audio::format::Bitrate;
use foxide2k::abi::preferences::{GUID_TOOLS, STATE_CHANGED, STATE_RESETTABLE};
use foxide2k::console;
use foxide2k::dialog::host::{Dialog, DialogHandler, create_child_dialog};
use foxide2k::dialog::notify::CBN_SELCHANGE;
use foxide2k::dialog::style::{
	BS_GROUPBOX, CBS_DROPDOWNLIST, DS_CONTROL, WS_CHILD, WS_TABSTOP, WS_VSCROLL,
};
use foxide2k::dialog::template::{ControlClass, DialogTemplate};
use foxide2k::error::Result;
use foxide2k::guid::Guid;
use foxide2k::preferences::{PageChanged, PageInstance, PreferencesPage};

use crate::account::{self, AccountPanel, LoginControls, item};
use crate::settings::{self, bitrate_at, bitrate_index};

const ACCOUNT_GROUP: u16 = 100;
const PLAYBACK_GROUP: u16 = 105;
const BITRATE_LABEL: u16 = 106;
const BITRATE: u16 = 107;

const BITRATES: [&str; 3] = ["96 kbit/s", "160 kbit/s", "320 kbit/s"];

/// The page, in dialog units. Creation order is tab order, and each label comes right before
/// its control.
pub fn page_template() -> DialogTemplate {
	let mut items =
		vec![item(ACCOUNT_GROUP, ControlClass::Button, "Account", BS_GROUPBOX, (7, 7, 318, 108))];
	items.extend(account::items(14, 21, 304));
	items.extend([
		item(PLAYBACK_GROUP, ControlClass::Button, "Playback", BS_GROUPBOX, (7, 120, 318, 40)),
		item(BITRATE_LABEL, ControlClass::Static, "&Preferred bitrate:", 0, (14, 136, 86, 8)),
		item(
			BITRATE,
			ControlClass::ComboBox,
			"",
			CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP,
			(104, 134, 90, 60),
		),
	]);
	DialogTemplate {
		style: WS_CHILD | DS_CONTROL,
		title: String::new(),
		cx: 332,
		cy: 170,
		font: (8, "MS Shell Dlg".to_owned()),
		items,
	}
}

/// The page under Preferences > Tools.
pub struct SpotifyPage;

impl PreferencesPage for SpotifyPage {
	const GUID: Guid = Guid::from_u128(0xcbd58b0a_b483_4deb_a487_38dfdb3e106e);
	const PARENT: Guid = GUID_TOOLS;
	const NAME: &'static CStr = c"foo_librespot";
	const HELP_URL: &'static str = "https://github.com/LeonarddeR/foo_librespot";

	fn create(parent: isize, changed: PageChanged) -> Result<Box<dyn PageInstance>> {
		let handler = PageHandler { panel: None, changed: changed.clone() };
		let dialog = create_child_dialog(parent, &page_template(), Box::new(handler))?;
		Ok(Box::new(OpenPage { dialog, changed }))
	}
}

struct PageHandler {
	panel: Option<AccountPanel>,
	changed: PageChanged,
}

impl DialogHandler for PageHandler {
	fn init(&mut self, dialog: &Dialog) -> Option<u16> {
		self.panel = Some(AccountPanel::init(*dialog, LoginControls::WhileConfirming));
		for text in BITRATES {
			dialog.combo_add(BITRATE, text);
		}
		dialog.combo_select(BITRATE, bitrate_index(settings::bitrate()));
		None
	}

	fn command(&mut self, dialog: &Dialog, id: u16, notification: u16) {
		if AccountPanel::command(*dialog, id, notification) {
			return;
		}
		if id == BITRATE && notification == CBN_SELCHANGE {
			self.changed.on_state_changed();
		}
	}

	fn refresh(&mut self, dialog: &Dialog) {
		if let Some(panel) = &mut self.panel {
			panel.refresh(*dialog);
		}
	}
}

/// The page while it is open in Preferences.
struct OpenPage {
	dialog: Dialog,
	changed: PageChanged,
}

impl OpenPage {
	fn chosen(&self) -> Bitrate {
		self.dialog.combo_selection(BITRATE).map_or_else(Bitrate::default, bitrate_at)
	}
}

impl PageInstance for OpenPage {
	fn hwnd(&self) -> isize {
		self.dialog.hwnd()
	}

	fn state(&self) -> u32 {
		if self.chosen() == settings::bitrate() {
			STATE_RESETTABLE
		} else {
			STATE_RESETTABLE | STATE_CHANGED
		}
	}

	fn apply(&self) {
		if let Err(error) = settings::set_bitrate(self.chosen()) {
			console::print(&format!("foo_librespot: can't save the bitrate: {}", error.message));
		}
		self.changed.on_state_changed();
	}

	fn reset(&self) {
		self.dialog.combo_select(BITRATE, bitrate_index(Bitrate::default()));
		self.changed.on_state_changed();
	}
}

#[cfg(test)]
mod tests {
	use std::sync::{Arc, Mutex, PoisonError};

	use foo_librespot_core::login::DeviceCode;
	use foxide2k::abi::preferences::{GUID_TOOLS, STATE_CHANGED, STATE_RESETTABLE};
	use foxide2k::dialog::host::Dialog;
	use foxide2k::dialog::notify::{BN_CLICKED, CBN_SELCHANGE};
	use foxide2k::preferences::preferences_page_service;
	use foxide2k::testing::{
		FakeApi, FakePageCallback, PageInstanceHandle, PreferencesHandle, hidden_window,
		pump_messages, send_command, wants_all_keys,
	};

	use super::{SpotifyPage, page_template};
	use crate::account::{ACCOUNT, BROWSER, CODE, CODE_LABEL, PAGE, PAGE_LABEL, STATUS};
	use crate::backend;
	use crate::login::{LoginController, install_controller};
	use crate::settings::BITRATE_KEY;
	use crate::testing::FakeBackend;

	const BITRATE: u16 = super::BITRATE;
	const BITRATE_LABEL: u16 = super::BITRATE_LABEL;

	struct Setup {
		api: &'static FakeApi,
		fake: FakeBackend,
		opened: Arc<Mutex<Vec<String>>>,
		callback: FakePageCallback,
		instance: PageInstanceHandle,
		dialog: Dialog,
	}

	fn open_page() -> Setup {
		let api = FakeApi::new();
		api.install();
		let fake = FakeBackend::default();
		backend::install(Box::new(fake.clone()));
		let opened = Arc::new(Mutex::new(Vec::new()));
		let record = Arc::clone(&opened);
		install_controller(LoginController::new(Box::new(move |url: &str| {
			record.lock().unwrap_or_else(PoisonError::into_inner).push(url.to_owned());
		})));
		let (callback, _destroyed) = FakePageCallback::new();
		let instance = PreferencesHandle::new(preferences_page_service::<SpotifyPage>())
			.instantiate(hidden_window(), &callback);
		let dialog = Dialog::from_hwnd(instance.hwnd());
		Setup { api, fake, opened, callback, instance, dialog }
	}

	fn code() -> DeviceCode {
		DeviceCode {
			user_code: "KKGT-2024".to_owned(),
			verification_uri: "https://spotify.com/pair".to_owned(),
			url: "https://spotify.com/pair?code=KKGT-2024".to_owned(),
		}
	}

	/// Presses the account button and lets Spotify send a code.
	fn confirming(setup: &Setup) {
		send_command(&setup.dialog, ACCOUNT, BN_CLICKED);
		setup.fake.send_code(code());
		pump_messages();
	}

	/// The controls that only matter while a code waits for the user.
	const LOGIN_CONTROLS: [u16; 5] = [CODE_LABEL, CODE, PAGE_LABEL, PAGE, BROWSER];

	fn shown(setup: &Setup) -> Vec<u16> {
		LOGIN_CONTROLS.into_iter().filter(|&id| setup.dialog.is_visible(id)).collect()
	}

	#[test]
	fn page_shows_the_logged_out_state() {
		let setup = open_page();
		assert_eq!(setup.dialog.text(STATUS), "Not logged in to Spotify.");
		assert_eq!(setup.dialog.text(ACCOUNT), "&Log in");
		assert_eq!(setup.dialog.text(CODE_LABEL), "Co&de:");
		assert_eq!(setup.dialog.text(PAGE_LABEL), "Pa&ge:");
		assert_eq!(setup.dialog.text(BITRATE_LABEL), "&Preferred bitrate:");
		assert_eq!(setup.dialog.combo_selection(BITRATE), Some(2));
	}

	#[test]
	fn code_and_page_appear_only_while_a_code_waits() {
		let setup = open_page();
		assert_eq!(shown(&setup), Vec::<u16>::new());
		send_command(&setup.dialog, ACCOUNT, BN_CLICKED);
		pump_messages();
		assert_eq!(shown(&setup), Vec::<u16>::new());
		setup.fake.send_code(code());
		pump_messages();
		assert_eq!(shown(&setup), LOGIN_CONTROLS);
		setup.fake.finish_login(Ok("Keith Getty".to_owned()));
		pump_messages();
		assert_eq!(shown(&setup), Vec::<u16>::new());
	}

	#[test]
	fn labels_precede_their_controls_in_tab_order() {
		let setup = open_page();
		assert_eq!(setup.dialog.tab_order(), [ACCOUNT, BITRATE]);
		confirming(&setup);
		assert_eq!(setup.dialog.tab_order(), [ACCOUNT, CODE, PAGE, BROWSER, BITRATE]);
		let ids: Vec<u16> = page_template().items.iter().map(|item| item.id).collect();
		let at = |id| ids.iter().position(|&each| each == id).unwrap();
		assert_eq!(at(CODE_LABEL) + 1, at(CODE));
		assert_eq!(at(PAGE_LABEL) + 1, at(PAGE));
		assert_eq!(at(BITRATE_LABEL) + 1, at(BITRATE));
	}

	#[test]
	fn escape_and_enter_in_the_code_fields_reach_preferences() {
		let setup = open_page();
		confirming(&setup);
		assert!(!wants_all_keys(&setup.dialog, CODE));
		assert!(!wants_all_keys(&setup.dialog, PAGE));
	}

	#[test]
	fn open_browser_shows_only_while_confirming_and_opens_on_press() {
		let setup = open_page();
		assert!(!setup.dialog.is_visible(BROWSER));
		send_command(&setup.dialog, ACCOUNT, BN_CLICKED);
		pump_messages();
		assert!(!setup.dialog.is_visible(BROWSER));
		setup.fake.send_code(code());
		pump_messages();
		assert!(setup.dialog.is_visible(BROWSER));
		assert!(setup.opened.lock().unwrap().is_empty(), "the browser opened by itself");
		send_command(&setup.dialog, BROWSER, BN_CLICKED);
		assert_eq!(*setup.opened.lock().unwrap(), ["https://spotify.com/pair?code=KKGT-2024"]);
	}

	#[test]
	fn pressing_the_button_starts_a_login_and_the_page_follows() {
		let setup = open_page();
		send_command(&setup.dialog, ACCOUNT, BN_CLICKED);
		pump_messages();
		assert_eq!(setup.dialog.text(STATUS), "Asking Spotify for a login code…");
		assert_eq!(setup.dialog.text(ACCOUNT), "&Cancel login");
		setup.fake.send_code(code());
		pump_messages();
		assert_eq!(setup.dialog.text(CODE), "KKGT-2024");
		assert_eq!(setup.dialog.text(PAGE), "https://spotify.com/pair?code=KKGT-2024");
		setup.fake.finish_login(Ok("Keith Getty".to_owned()));
		pump_messages();
		assert_eq!(setup.dialog.text(STATUS), "Logged in to Spotify as Keith Getty.");
		assert_eq!(setup.dialog.text(ACCOUNT), "Log &out");
	}

	#[test]
	fn focus_leaves_open_browser_for_the_account_button_when_the_login_ends() {
		let setup = open_page();
		confirming(&setup);
		setup.dialog.focus(BROWSER);
		setup.fake.finish_login(Ok("Keith Getty".to_owned()));
		pump_messages();
		assert!(!setup.dialog.is_visible(BROWSER));
		assert_eq!(setup.dialog.focused(), Some(ACCOUNT));
	}

	/// Ends a login while foobar2000 is in the background, as it is while the user confirms in
	/// the browser, then gives focus back to `id`, as Windows does when the Preferences window
	/// is activated again with `id` as its saved focus.
	fn focus_returns_after_the_login_to(id: u16) -> Option<u16> {
		let setup = open_page();
		confirming(&setup);
		setup.fake.finish_login(Ok("Keith Getty".to_owned()));
		pump_messages();
		setup.dialog.focus(id);
		pump_messages();
		setup.dialog.focused()
	}

	#[test]
	fn focus_that_returns_to_a_hidden_code_field_moves_to_the_account_button() {
		assert_eq!(focus_returns_after_the_login_to(PAGE), Some(ACCOUNT));
	}

	#[test]
	fn focus_that_returns_to_a_hidden_open_browser_moves_to_the_account_button() {
		assert_eq!(focus_returns_after_the_login_to(BROWSER), Some(ACCOUNT));
	}

	#[test]
	fn focus_leaves_a_code_field_for_the_account_button_when_the_login_is_cancelled() {
		let setup = open_page();
		confirming(&setup);
		setup.dialog.focus(PAGE);
		send_command(&setup.dialog, ACCOUNT, BN_CLICKED);
		pump_messages();
		assert!(!setup.dialog.is_visible(PAGE));
		assert_eq!(setup.dialog.focused(), Some(ACCOUNT));
	}

	#[test]
	fn bitrate_change_marks_the_page_changed_and_apply_stores_it() {
		let setup = open_page();
		assert_eq!(setup.instance.state(), STATE_RESETTABLE);
		setup.dialog.combo_select(BITRATE, 0);
		send_command(&setup.dialog, BITRATE, CBN_SELCHANGE);
		assert_eq!(setup.callback.calls(), 1);
		assert_eq!(setup.instance.state(), STATE_RESETTABLE | STATE_CHANGED);
		setup.instance.apply();
		assert_eq!(setup.api.config_int(BITRATE_KEY), Some(96));
		assert_eq!(setup.instance.state(), STATE_RESETTABLE);
	}

	#[test]
	fn reset_selects_320() {
		let setup = open_page();
		setup.dialog.combo_select(BITRATE, 0);
		setup.instance.reset();
		assert_eq!(setup.dialog.combo_selection(BITRATE), Some(2));
		assert_eq!(setup.instance.state(), STATE_RESETTABLE);
	}

	#[test]
	fn page_registers_under_tools() {
		let page = PreferencesHandle::new(preferences_page_service::<SpotifyPage>());
		assert_eq!(page.name(), "foo_librespot");
		assert_eq!(page.parent_guid(), GUID_TOOLS);
		assert_eq!(
			page.guid(),
			foxide2k::guid::Guid::from_u128(0xcbd58b0a_b483_4deb_a487_38dfdb3e106e)
		);
		assert_eq!(page.help_url().as_deref(), Some("https://github.com/LeonarddeR/foo_librespot"));
	}
}
