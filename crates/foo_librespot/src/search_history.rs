//! The last Spotify searches, newest first, kept in foobar2000's settings store so the search
//! window's field offers them across restarts.

use foxide2k::config_store;

/// The searches, newest first, one per line.
pub const HISTORY_KEY: &str = "foo_librespot.search_history";

/// How many searches the history keeps.
pub const HISTORY_LIMIT: usize = 20;

/// `text` with every run of whitespace (spaces, tabs, line breaks) as one space and the ends
/// trimmed; nothing is percent-decoded. `None` when nothing is left.
pub fn normalize(text: &str) -> Option<String> {
	let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
	Some(words).filter(|words| !words.is_empty())
}

/// `history` with `words` first and any older entry that differs from it only in capitals left
/// out, cut to [`HISTORY_LIMIT`].
pub fn remember(history: &[String], words: &str) -> Vec<String> {
	let key = words.to_lowercase();
	std::iter::once(words.to_owned())
		.chain(history.iter().filter(|entry| entry.to_lowercase() != key).cloned())
		.take(HISTORY_LIMIT)
		.collect()
}

/// The stored searches, newest first, at most [`HISTORY_LIMIT`]; empty when none are stored.
pub fn load() -> Vec<String> {
	config_store::get_string(HISTORY_KEY).map_or_else(Vec::new, |text| {
		text.lines()
			.filter(|line| !line.is_empty())
			.take(HISTORY_LIMIT)
			.map(str::to_owned)
			.collect()
	})
}

/// Stores `history`, one search per line.
pub fn save(history: &[String]) -> foxide2k::error::Result<()> {
	config_store::set_string(HISTORY_KEY, &history.join("\n"))
}

#[cfg(test)]
mod tests {
	use super::{HISTORY_KEY, HISTORY_LIMIT, load, normalize, remember, save};
	use crate::testing::installed_api;

	fn strings(items: &[&str]) -> Vec<String> {
		items.iter().map(|&item| item.to_owned()).collect()
	}

	#[test]
	fn normalize_makes_every_run_of_whitespace_one_space() {
		assert_eq!(normalize("  in\tchrist \n alone "), Some("in christ alone".to_owned()));
		assert_eq!(normalize(" \t "), None);
		assert_eq!(normalize("100% Händel"), Some("100% Händel".to_owned()));
	}

	#[test]
	fn remember_puts_new_words_first() {
		assert_eq!(remember(&strings(&["bach"]), "getty"), strings(&["getty", "bach"]));
	}

	#[test]
	fn remember_moves_a_repeat_to_the_top_with_the_new_spelling() {
		let history = strings(&["bach", "In Christ Alone", "getty"]);
		assert_eq!(
			remember(&history, "in christ alone"),
			strings(&["in christ alone", "bach", "getty"])
		);
	}

	#[test]
	fn remember_keeps_the_last_twenty() {
		let history: Vec<String> = (0..HISTORY_LIMIT).map(|n| format!("search {n}")).collect();
		let remembered = remember(&history, "getty");
		assert_eq!(remembered.len(), HISTORY_LIMIT);
		assert_eq!(remembered[0], "getty");
		assert_eq!(remembered[1..], history[..HISTORY_LIMIT - 1]);
		assert!(!remembered.contains(&format!("search {}", HISTORY_LIMIT - 1)));
	}

	#[test]
	fn the_history_round_trips_through_the_settings_store() {
		let api = installed_api();
		assert!(load().is_empty());
		save(&strings(&["a b", "c"])).unwrap();
		assert_eq!(load(), strings(&["a b", "c"]));
		assert_eq!(api.config_string(HISTORY_KEY), Some("a b\nc".to_owned()));
	}

	#[test]
	fn load_skips_empty_lines_and_keeps_at_most_twenty() {
		installed_api();
		let lines: Vec<String> = (0..HISTORY_LIMIT + 5).map(|n| format!("search {n}")).collect();
		let stored = format!("\n{}\n\n", lines.join("\n\n"));
		foxide2k::config_store::set_string(HISTORY_KEY, &stored).unwrap();
		assert_eq!(load(), lines[..HISTORY_LIMIT]);
	}
}
