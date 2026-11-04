//! Release notes: a version's section of `CHANGELOG.md`.

/// The text under `## [<version>]` in a Keep a Changelog file, trimmed. It ends at the next
/// second-level heading or at the link definitions Keep a Changelog puts at the bottom. The error
/// says whether the section is missing or empty.
pub fn release_notes(changelog: &str, version: &str) -> Result<String, String> {
	let heading = format!("## [{version}]");
	let mut lines = changelog.lines().skip_while(|line| !line.starts_with(&heading));
	if lines.next().is_none() {
		return Err(format!("CHANGELOG.md has no section for version {version}"));
	}
	let section: Vec<&str> = lines.take_while(|line| !ends_section(line)).collect();
	let notes = section.join("\n").trim().to_owned();
	if notes.is_empty() {
		return Err(format!("CHANGELOG.md's section for version {version} is empty"));
	}
	Ok(notes)
}

/// A line that ends a section: the next `## ` heading, or a link definition such as
/// `[0.1.0]: https://…`.
fn ends_section(line: &str) -> bool {
	line.starts_with("## ")
		|| (line.starts_with('[')
			&& line.split_once("]: ").is_some_and(|(label, _)| !label.contains(' ')))
}

#[cfg(test)]
mod tests {
	use super::release_notes;

	const CHANGELOG: &str = "# Changelog

## [Unreleased]

- Not yet.

## [0.1.0] - 2026-11-05

### Added

- First.

## [0.0.9] - 2026-10-01

- Older.
";

	#[test]
	fn finds_the_section_for_a_version() {
		assert_eq!(release_notes(CHANGELOG, "0.1.0").as_deref(), Ok("### Added\n\n- First."));
	}

	#[test]
	fn a_missing_version_says_there_is_no_section() {
		assert_eq!(
			release_notes(CHANGELOG, "0.2.0"),
			Err("CHANGELOG.md has no section for version 0.2.0".to_owned())
		);
	}

	#[test]
	fn a_prefix_is_not_a_match() {
		let changelog = "## [0.1.0-beta.1] - 2026-11-01\n\n- Beta.\n";
		assert!(release_notes(changelog, "0.1.0").is_err());
	}

	#[test]
	fn an_empty_section_says_it_is_empty() {
		let changelog = "## [0.1.0] - 2026-11-05\n\n## [0.0.9] - 2026-10-01\n\n- Older.\n";
		assert_eq!(
			release_notes(changelog, "0.1.0"),
			Err("CHANGELOG.md's section for version 0.1.0 is empty".to_owned())
		);
	}

	#[test]
	fn the_oldest_section_stops_at_the_link_definitions_and_other_headings() {
		let changelog = "## [0.1.0] - 2026-11-05\n\n- First.\n\n[unreleased]: https://github.com/x/y/compare/v0.1.0...HEAD\n[0.1.0]: https://github.com/x/y/releases/tag/v0.1.0\n";
		assert_eq!(release_notes(changelog, "0.1.0").as_deref(), Ok("- First."));
		let changelog = "## [0.1.0] - 2026-11-05\n\n- First.\n\n## Notes\n\nAbout this file.\n";
		assert_eq!(release_notes(changelog, "0.1.0").as_deref(), Ok("- First."));
	}
}
