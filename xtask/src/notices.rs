//! Checks on the generated third-party notices.

/// The comment `about.hbs` writes before a licence text that cargo-about took from its own
/// generic copy because the crates ship no licence file it can read, followed by the licence id
/// and the crates' names.
const GENERIC_MARKER: &str = "<!-- generic licence text: ";

/// Licences whose text carries the holder's copyright line. Their generic text has none, or a
/// placeholder in its place, so a notice made from it is incomplete. The generic texts of the
/// others, such as Apache-2.0 and MPL-2.0, are the whole licence.
const NEEDS_COPYRIGHT: [&str; 6] =
	["MIT", "BSD-2-Clause", "BSD-3-Clause", "BSD-Source-Code", "ISC", "Zlib"];

/// Why the notices in `html` can't ship, if they can't: a licence that needs a copyright line
/// came from the generic text. Such crates need a `clarify` entry in `about.toml`.
pub fn missing_copyright(html: &str) -> Option<String> {
	let mut missing = html.split(GENERIC_MARKER).skip(1).filter_map(|rest| {
		let (marker, _) = rest.split_once(" -->")?;
		let (license, crates) = marker.split_once(" for ")?;
		NEEDS_COPYRIGHT.contains(&license).then(|| format!("{license} for {}", crates.trim()))
	});
	let first = missing.next()?;
	let all: Vec<String> = std::iter::once(first).chain(missing).collect();
	Some(format!(
		"licenses.html has the generic licence text, without a copyright line, of {}. Give these crates a clarify entry in about.toml.",
		all.join("; ")
	))
}

#[cfg(test)]
mod tests {
	use super::{GENERIC_MARKER, missing_copyright};

	#[test]
	fn a_generic_mit_text_names_its_crates() {
		let html = "<h3>a</h3>\n<!-- generic licence text: MIT for a b -->\n<pre>MIT License</pre>";
		assert_eq!(
			missing_copyright(html).as_deref(),
			Some(
				"licenses.html has the generic licence text, without a copyright line, of MIT for a b. Give these crates a clarify entry in about.toml."
			)
		);
	}

	#[test]
	fn a_generic_zlib_text_is_caught_although_it_has_no_placeholder() {
		let html = "<!-- generic licence text: Zlib for c -->\n<pre>zlib License\n\nThis software is provided 'as-is'</pre>";
		assert!(missing_copyright(html).is_some_and(|message| message.contains("Zlib for c")));
	}

	#[test]
	fn generic_apache_and_mpl_texts_are_complete() {
		let html = "<!-- generic licence text: Apache-2.0 for d -->\n<!-- generic licence text: MPL-2.0 for e -->";
		assert_eq!(missing_copyright(html), None);
	}

	#[test]
	fn texts_from_the_crates_own_files_pass() {
		let html = "<pre>Copyright (c) 2015 Paul Lietar\n\nPermission is hereby granted</pre>";
		assert_eq!(missing_copyright(html), None);
	}

	#[test]
	fn the_template_writes_the_marker_for_generic_texts() {
		let template = include_str!("../../about.hbs");
		assert!(
			template
				.contains(&format!("{{{{#unless source_path}}}}{GENERIC_MARKER}{{{{id}}}} for ")),
			"about.hbs must mark generic texts the way notices.rs reads them"
		);
	}
}
