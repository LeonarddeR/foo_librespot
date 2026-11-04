//! Opening web pages in the user's browser.

use windows_core::PCWSTR;

use crate::bindings::Windows::Win32::{SW_SHOWNORMAL, ShellExecuteW};
use crate::cxx::ExceptionKind;
use crate::error::{Error, Result};
use crate::strings::wide;

/// Opens `url` in the default browser. Only `https://` URLs are opened, so a page address
/// from the network can never start a program or open a file.
pub fn open_url(url: &str) -> Result<()> {
	let https = url.len() > "https://".len()
		&& url
			.get(.."https://".len())
			.is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
		&& !url.contains(['"', '\0', ' ']);
	if !https {
		return Err(Error::new(
			ExceptionKind::InvalidParams,
			format!("not an https:// URL: {url}"),
		));
	}
	let url = wide(url);
	let result = unsafe {
		ShellExecuteW(
			None,
			windows_core::w!("open"),
			PCWSTR(url.as_ptr()),
			PCWSTR::null(),
			PCWSTR::null(),
			SW_SHOWNORMAL,
		)
	};
	// ShellExecute reports success as a value above 32.
	if result.0 as usize > 32 {
		Ok(())
	} else {
		Err(Error::new(
			ExceptionKind::Std,
			format!("can't open the browser (error {})", result.0 as usize),
		))
	}
}

#[cfg(test)]
mod tests {
	use super::open_url;

	#[test]
	fn open_url_rejects_anything_but_https() {
		// Never launches a browser: each of these is refused before ShellExecute.
		for url in ["file:///C:/x", "http://spotify.com/pair", "calc.exe", "", "javascript:x"] {
			assert!(open_url(url).is_err(), "{url}");
		}
	}
}
