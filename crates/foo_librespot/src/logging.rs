//! librespot's and our own log messages, in the foobar2000 console.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::settings;

/// Whether detailed logging is on.
static DETAILED: AtomicBool = AtomicBool::new(false);

/// Ours at info level and up, and everyone else's from warnings up. With `detailed`, ours and
/// librespot's from debug level up. Trace level never shows: librespot logs access tokens there.
pub fn wanted(target: &str, level: Level, detailed: bool) -> bool {
	let ours = target.starts_with("foo_librespot");
	if detailed && (ours || target.starts_with("librespot")) {
		level <= Level::Debug
	} else if ours {
		level <= Level::Info
	} else {
		level <= Level::Warn
	}
}

/// librespot messages that dump whole raw server messages, by target and the start of the message.
const HELD_BACK: [(&str, &str); 2] = [
	("librespot_core::session", "could not dispatch command:"),
	("librespot_core::mercury", "can't handle Mercury response:"),
];

/// Whether `message` from `target` is one that never reaches the console.
fn held_back(target: &str, message: &str) -> bool {
	HELD_BACK.iter().any(|&(held, start)| target == held && message.starts_with(start))
}

/// `message` with each URL's query string replaced by `?…` and any `user:password@` by `…@`. A
/// URL starts at a scheme, such as `https://` or `wss://` in any case, and ends at whitespace, a
/// quote, `<`, `>` or the end.
fn redact_urls(message: &str) -> Cow<'_, str> {
	let Some(mut start) = url_start(message) else {
		return Cow::Borrowed(message);
	};
	let mut redacted = String::with_capacity(message.len());
	let mut rest = message;
	loop {
		redacted.push_str(&rest[..start]);
		let url = &rest[start..];
		let end = url
			.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
			.unwrap_or(url.len());
		redact_url(&url[..end], &mut redacted);
		rest = &url[end..];
		match url_start(rest) {
			Some(next) => start = next,
			None => break,
		}
	}
	redacted.push_str(rest);
	Cow::Owned(redacted)
}

/// Where the first URL in `text` starts: at its scheme, the letters, digits, `+`, `-` and `.`
/// right before a `://`, from the first letter.
fn url_start(text: &str) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut from = 0;
	while let Some(found) = text[from..].find("://") {
		let separator = from + found;
		let run = bytes[..separator]
			.iter()
			.rposition(|&byte| {
				!(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
			})
			.map_or(0, |before| before + 1);
		if let Some(letter) = bytes[run..separator].iter().position(u8::is_ascii_alphabetic) {
			return Some(run + letter);
		}
		from = separator + "://".len();
	}
	None
}

/// Appends `url`, which starts with its scheme, without its query string and password. The
/// password ends at the last `@` before the first `/`, so a `?` in it is cut too.
fn redact_url(url: &str, out: &mut String) {
	let (scheme, after) = url.split_once("://").unwrap_or(("", url));
	out.push_str(scheme);
	out.push_str("://");
	let authority_end = after.find('/').unwrap_or(after.len());
	let rest = match after[..authority_end].rfind('@') {
		Some(at) => {
			out.push('…');
			&after[at..]
		}
		None => after,
	};
	match rest.split_once('?') {
		Some((head, _)) => {
			out.push_str(head);
			out.push_str("?…");
		}
		None => out.push_str(rest),
	}
}

struct ConsoleLogger;

impl Log for ConsoleLogger {
	fn enabled(&self, metadata: &Metadata<'_>) -> bool {
		wanted(metadata.target(), metadata.level(), DETAILED.load(Ordering::Relaxed))
	}

	fn log(&self, record: &Record<'_>) {
		if !self.enabled(record.metadata()) {
			return;
		}
		let message = record.args().to_string();
		if !held_back(record.target(), &message) {
			foxide2k::console::print(&format!("foo_librespot: {}", redact_urls(&message)));
		}
	}

	fn flush(&self) {}
}

static LOGGER: ConsoleLogger = ConsoleLogger;

/// Turns detailed logging on or off, from the next message on.
pub fn set_detailed(on: bool) {
	DETAILED.store(on, Ordering::Relaxed);
	log::set_max_level(if on { LevelFilter::Debug } else { LevelFilter::Info });
}

/// Sends log messages to the console from now on, in detail if the stored switch says so. Called
/// once, at startup; a second call, or another logger already in place, changes nothing.
pub fn init() {
	if log::set_logger(&LOGGER).is_ok() {
		set_detailed(settings::detailed_logging());
	}
}

#[cfg(test)]
mod tests {
	use std::borrow::Cow;

	use foxide2k::testing::FakeApi;
	use log::{Level, LevelFilter, Log, Record};

	use super::{LOGGER, held_back, init, redact_urls, set_detailed, wanted};
	use crate::settings;
	use crate::testing::lock_shared_state;

	#[test]
	fn logging_shows_our_info_and_only_warnings_from_others_by_default() {
		assert!(wanted("foo_librespot::links", Level::Info, false));
		assert!(wanted("foo_librespot_core::session", Level::Info, false));
		assert!(!wanted("foo_librespot", Level::Debug, false));
		assert!(!wanted("librespot_core::session", Level::Info, false));
		assert!(wanted("librespot_core::session", Level::Warn, false));
		// librespot logs access tokens at trace level; they must never reach the console.
		assert!(!wanted("librespot_core::token", Level::Trace, false));
	}

	#[test]
	fn detailed_logging_shows_debug_from_us_and_librespot() {
		for target in
			["foo_librespot::links", "foo_librespot_core::resolve", "librespot_core::session"]
		{
			assert!(wanted(target, Level::Debug, true), "{target}");
		}
	}

	#[test]
	fn detailed_logging_never_shows_trace() {
		assert!(!wanted("librespot_core::token", Level::Trace, true));
		assert!(!wanted("foo_librespot", Level::Trace, true));
	}

	#[test]
	fn detailed_logging_keeps_other_crates_at_warnings() {
		assert!(!wanted("hyper_util::client", Level::Debug, true));
		assert!(!wanted("tokio", Level::Info, true));
		assert!(wanted("tokio", Level::Warn, true));
	}

	#[test]
	fn raw_server_messages_are_held_back() {
		assert!(held_back(
			"librespot_core::session",
			"could not dispatch command: error handling Mercury response: MercuryResponse { uri: \"hm://pusher/v1/connections/x\", status_code: 200, payload: [[1, 2]] }"
		));
		assert!(held_back(
			"librespot_core::mercury",
			"can't handle Mercury response: MercuryResponse { uri: \"hm://x\", status_code: 200, payload: [] }"
		));
		assert!(!held_back(
			"librespot_core::session",
			"Connecting to AP \"ap-gew4.spotify.com:4070\""
		));
		assert!(!held_back("foo_librespot::links", "could not dispatch command: x"));
	}

	#[test]
	fn urls_lose_their_query_and_password() {
		assert_eq!(
			redact_urls(
				"Fetching https://audio4-fa.scdn.co/audio/ab12?1700000000_Sig-x returned 403 (expected 206 Partial Content), trying next"
			),
			"Fetching https://audio4-fa.scdn.co/audio/ab12?… returned 403 (expected 206 Partial Content), trying next"
		);
		assert_eq!(
			redact_urls(
				"Cannot parse CDN URL expiry timestamp 'x' from 'https://audio-ak.spotifycdn.com/audio/ab12?__token__=exp=1~hmac=ff'"
			),
			"Cannot parse CDN URL expiry timestamp 'x' from 'https://audio-ak.spotifycdn.com/audio/ab12?…'"
		);
		assert_eq!(
			redact_urls("Using proxy \"http://me:secret@proxy.local:8080/\""),
			"Using proxy \"http://…@proxy.local:8080/\""
		);
		assert_eq!(
			redact_urls("Requesting /metadata/4/track/ab12?market=from_token"),
			"Requesting /metadata/4/track/ab12?market=from_token"
		);
		let plain = redact_urls("Connecting to AP \"ap-gew4.spotify.com:4070\"");
		assert_eq!(plain, "Connecting to AP \"ap-gew4.spotify.com:4070\"");
		assert!(matches!(plain, Cow::Borrowed(_)));
	}

	#[test]
	fn every_scheme_in_any_case_loses_its_query() {
		assert_eq!(
			redact_urls("Fetching HTTPS://audio4-fa.scdn.co/audio/ab12?1700000000_Sig-x"),
			"Fetching HTTPS://audio4-fa.scdn.co/audio/ab12?…"
		);
		assert_eq!(
			redact_urls("Connecting to wss://dealer.spotify.com:443/?access_token=abc"),
			"Connecting to wss://dealer.spotify.com:443/?…"
		);
		assert_eq!(
			redact_urls("unknown subscription uri=hm://pusher/v1/connections/x?y=1"),
			"unknown subscription uri=hm://pusher/v1/connections/x?…"
		);
	}

	#[test]
	fn a_password_with_a_question_mark_is_cut_whole() {
		assert_eq!(
			redact_urls("Using proxy \"http://me:pa?ss@proxy.local:8080/\""),
			"Using proxy \"http://…@proxy.local:8080/\""
		);
	}

	#[test]
	fn the_console_gets_no_dumps_and_no_signed_urls() {
		let _shared = lock_shared_state();
		let api = FakeApi::new();
		api.install();
		set_detailed(true);
		LOGGER.log(
			&Record::builder()
				.target("librespot_core::session")
				.level(Level::Debug)
				.args(format_args!("could not dispatch command: payload: [[1, 2]]"))
				.build(),
		);
		LOGGER.log(
			&Record::builder()
				.target("librespot_audio::fetch")
				.level(Level::Warn)
				.args(format_args!(
					"Fetching https://audio4-fa.scdn.co/audio/ab12?1700000000_Sig-x returned 403 (expected 206 Partial Content), trying next"
				))
				.build(),
		);
		let lines = api.console_lines();
		assert_eq!(lines.len(), 1, "{lines:?}");
		assert!(lines[0].starts_with("foo_librespot: Fetching https://"), "{lines:?}");
		assert!(lines[0].contains("?…") && !lines[0].contains("Sig"), "{lines:?}");
	}

	#[test]
	fn the_switch_sets_the_log_level_at_once() {
		let _shared = lock_shared_state();
		set_detailed(true);
		assert_eq!(log::max_level(), LevelFilter::Debug);
		set_detailed(false);
		assert_eq!(log::max_level(), LevelFilter::Info);
	}

	#[test]
	fn init_applies_the_stored_switch() {
		let _shared = lock_shared_state();
		FakeApi::new().install();
		settings::set_detailed_logging(true).unwrap();
		init();
		assert_eq!(log::max_level(), LevelFilter::Debug);
	}
}
