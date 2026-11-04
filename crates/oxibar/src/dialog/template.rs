//! Dialog templates built in Rust, in the `DLGTEMPLATEEX` layout `CreateDialogIndirectParamW`
//! reads, so no resource compiler is needed.

use crate::bindings::Windows::Win32::{DS_SETFONT, WS_CHILD, WS_VISIBLE};

/// The predefined window classes a template item can name by atom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlClass {
	Button,
	Edit,
	Static,
	ComboBox,
}

impl ControlClass {
	const fn atom(self) -> u16 {
		match self {
			Self::Button => 0x0080,
			Self::Edit => 0x0081,
			Self::Static => 0x0082,
			Self::ComboBox => 0x0085,
		}
	}
}

/// One control. Positions and sizes are in dialog units. `WS_CHILD | WS_VISIBLE` are always
/// added to `style`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogItem {
	pub id: u16,
	pub class: ControlClass,
	pub text: String,
	pub style: u32,
	pub x: i16,
	pub y: i16,
	pub cx: i16,
	pub cy: i16,
}

/// A whole dialog. `DS_SETFONT` is always added to `style`; `font` is the point size and face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogTemplate {
	pub style: u32,
	pub title: String,
	pub cx: i16,
	pub cy: i16,
	pub font: (u16, String),
	pub items: Vec<DialogItem>,
}

fn push_dword(words: &mut Vec<u16>, value: u32) {
	words.extend([(value & 0xFFFF) as u16, (value >> 16) as u16]);
}

fn push_string(words: &mut Vec<u16>, text: &str) {
	words.extend(text.encode_utf16().chain([0]));
}

#[allow(clippy::cast_sign_loss, reason = "coordinates are stored as their 16-bit pattern")]
const fn word(value: i16) -> u16 {
	value as u16
}

impl DialogTemplate {
	/// The template in `DLGTEMPLATEEX` layout. The words must be copied to a DWORD-aligned
	/// buffer before Windows reads them.
	pub fn to_bytes(&self) -> Vec<u16> {
		let mut words = vec![1, 0xFFFF];
		push_dword(&mut words, 0); // helpID
		push_dword(&mut words, 0); // exStyle
		push_dword(&mut words, self.style | DS_SETFONT as u32);
		let count = u16::try_from(self.items.len()).expect("a dialog has fewer than 65536 items");
		words.extend([count, 0, 0, word(self.cx), word(self.cy)]);
		words.extend([0, 0]); // no menu, the default dialog class
		push_string(&mut words, &self.title);
		// Point size, weight (normal), then italic (0) in the low byte and charset (1, the
		// default) in the high byte.
		words.extend([self.font.0, 400, 0x0100]);
		push_string(&mut words, &self.font.1);
		for item in &self.items {
			if words.len() % 2 == 1 {
				words.push(0);
			}
			push_dword(&mut words, 0); // helpID
			push_dword(&mut words, 0); // exStyle
			push_dword(&mut words, item.style | (WS_CHILD | WS_VISIBLE) as u32);
			words.extend([word(item.x), word(item.y), word(item.cx), word(item.cy)]);
			push_dword(&mut words, u32::from(item.id));
			words.extend([0xFFFF, item.class.atom()]);
			push_string(&mut words, &item.text);
			words.push(0); // no creation data
		}
		words
	}
}

#[cfg(test)]
mod tests {
	use super::{ControlClass, DialogItem, DialogTemplate};
	use crate::bindings::Windows::Win32::{DS_SETFONT, WS_CHILD, WS_TABSTOP, WS_VISIBLE};

	fn item(id: u16, class: ControlClass, text: &str) -> DialogItem {
		DialogItem { id, class, text: text.to_owned(), style: 0, x: 1, y: 2, cx: 30, cy: 14 }
	}

	fn template(items: Vec<DialogItem>) -> DialogTemplate {
		DialogTemplate {
			style: 0,
			title: "Log in to Spotify".to_owned(),
			cx: 260,
			cy: 130,
			font: (8, "MS Shell Dlg".to_owned()),
			items,
		}
	}

	fn utf16(text: &str) -> Vec<u16> {
		text.encode_utf16().chain([0]).collect()
	}

	fn dword(words: &[u16], at: usize) -> u32 {
		u32::from(words[at]) | (u32::from(words[at + 1]) << 16)
	}

	/// Skips a NUL-terminated string starting at `at`; returns the index after the NUL.
	fn skip_string(words: &[u16], at: usize) -> usize {
		at + words[at..].iter().position(|&word| word == 0).unwrap() + 1
	}

	/// Where each item starts, found by walking the template the way Windows does.
	fn item_starts(words: &[u16]) -> Vec<usize> {
		let count = usize::from(words[8]);
		// Header, then the menu and class (both 0), then the title.
		let mut at = skip_string(words, 15);
		// Point size, weight, italic and charset, then the face name.
		at = skip_string(words, at + 3);
		let mut starts = Vec::new();
		for _ in 0..count {
			at += at % 2;
			starts.push(at);
			// Fixed part (12 words), then 0xFFFF and the class atom, the title, the extra count.
			at = skip_string(words, at + 14) + 1;
		}
		starts
	}

	#[test]
	fn header_has_the_dlgtemplateex_signature_and_item_count() {
		let words = template(vec![
			item(101, ControlClass::Static, "Status"),
			item(102, ControlClass::Button, "&Log in"),
		])
		.to_bytes();
		assert_eq!(words[..2], [1, 0xFFFF]);
		assert_eq!(dword(&words, 6) & DS_SETFONT as u32, DS_SETFONT as u32);
		assert_eq!(words[8], 2);
		assert_eq!(words[9..13], [0, 0, 260, 130]);
	}

	#[test]
	fn items_start_on_dword_boundaries() {
		let words = template(vec![
			item(101, ControlClass::Static, "Odd"),
			item(102, ControlClass::Button, "Even"),
		])
		.to_bytes();
		let starts = item_starts(&words);
		assert_eq!(starts.len(), 2);
		assert!(starts.iter().all(|start| start % 2 == 0), "{starts:?}");
	}

	#[test]
	fn item_encodes_class_atom_text_and_id() {
		let mut button = item(102, ControlClass::Button, "&Log in");
		button.style = WS_TABSTOP as u32;
		let words = template(vec![button]).to_bytes();
		let at = item_starts(&words)[0];
		let style = dword(&words, at + 4);
		assert_eq!(style, (WS_CHILD | WS_VISIBLE | WS_TABSTOP) as u32);
		assert_eq!(words[at + 6..at + 10], [1, 2, 30, 14]);
		assert_eq!(dword(&words, at + 10), 102);
		assert_eq!(words[at + 12..at + 14], [0xFFFF, 0x0080]);
		assert_eq!(words[at + 14..at + 14 + 8], utf16("&Log in")[..]);
		assert_eq!(words[at + 22], 0, "extra count");
		let classes = [
			(ControlClass::Edit, 0x0081),
			(ControlClass::Static, 0x0082),
			(ControlClass::ComboBox, 0x0085),
		];
		for (class, atom) in classes {
			let words = template(vec![item(1, class, "")]).to_bytes();
			assert_eq!(words[item_starts(&words)[0] + 13], atom);
		}
	}

	#[test]
	fn font_follows_the_title() {
		let words = template(Vec::new()).to_bytes();
		assert_eq!(words[13..15], [0, 0], "no menu, default class");
		let title = utf16("Log in to Spotify");
		assert_eq!(words[15..15 + title.len()], title[..]);
		let font = 15 + title.len();
		assert_eq!(words[font..font + 3], [8, 400, 0x0100]);
		assert_eq!(words[font + 3..], utf16("MS Shell Dlg")[..]);
	}
}
