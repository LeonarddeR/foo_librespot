//! Release notes: a version's section of `CHANGELOG.md`.

/// The text under `## [<version>]` in a Keep a Changelog file, up to the next version's
/// heading, trimmed; `None` when there is no such section or it is empty.
pub fn release_notes(changelog: &str, version: &str) -> Option<String> {
	let heading = format!("## [{version}]");
	let mut lines = changelog.lines().skip_while(|line| !line.starts_with(&heading));
	lines.next()?;
	let section: Vec<&str> = lines.take_while(|line| !line.starts_with("## [")).collect();
	let notes = section.join("\n").trim().to_owned();
	(!notes.is_empty()).then_some(notes)
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
		assert_eq!(release_notes(CHANGELOG, "0.1.0").as_deref(), Some("### Added\n\n- First."));
	}

	#[test]
	fn a_missing_version_has_no_notes() {
		assert_eq!(release_notes(CHANGELOG, "0.2.0"), None);
	}

	#[test]
	fn a_prefix_is_not_a_match() {
		let changelog = "## [0.1.0-beta.1] - 2026-11-01\n\n- Beta.\n";
		assert_eq!(release_notes(changelog, "0.1.0"), None);
	}

	#[test]
	fn an_empty_section_has_no_notes() {
		let changelog = "## [0.1.0] - 2026-11-05\n\n## [0.0.9] - 2026-10-01\n\n- Older.\n";
		assert_eq!(release_notes(changelog, "0.1.0"), None);
	}
}
