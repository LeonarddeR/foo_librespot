//! Checks on the generated third-party notices.

/// The first placeholder of a generic licence template left in `html`, such as the
/// `<copyright holders>` of the MIT text. cargo-about falls back to the generic text when a
/// crate ships no licence file it can read, and then the notice lacks the copyright line the
/// licence asks for; such a crate needs a `clarify` entry in `about.toml`.
pub fn placeholder(html: &str) -> Option<&'static str> {
	PLACEHOLDERS.into_iter().find(|placeholder| html.contains(placeholder))
}

/// The placeholders of the MIT, BSD, ISC and zlib templates, as the HTML escapes them.
const PLACEHOLDERS: [&str; 3] = ["&lt;year&gt;", "&lt;copyright holders&gt;", "&lt;owner&gt;"];

#[cfg(test)]
mod tests {
	use super::placeholder;

	#[test]
	fn the_generic_mit_text_is_a_placeholder() {
		let html = "<pre>Copyright (c) &lt;year&gt; &lt;copyright holders&gt;\n\nPermission is hereby granted</pre>";
		assert_eq!(placeholder(html), Some("&lt;year&gt;"));
	}

	#[test]
	fn the_generic_bsd_text_is_a_placeholder() {
		let html = "<pre>Copyright (c) 2019 &lt;owner&gt;. All rights reserved.</pre>";
		assert_eq!(placeholder(html), Some("&lt;owner&gt;"));
	}

	#[test]
	fn a_real_copyright_line_is_not() {
		let html = "<pre>Copyright (c) 2015 Paul Lietar\n\nPermission is hereby granted</pre>";
		assert_eq!(placeholder(html), None);
	}
}
