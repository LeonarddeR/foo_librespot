//! librespot's and our own log messages, in the foobar2000 console.

use std::borrow::Cow;
use std::fmt::{self, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_log::NormalizeEvent as _;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::{LookupSpan, Registry};

use crate::settings;

/// Whether detailed logging is on.
static DETAILED: AtomicBool = AtomicBool::new(false);

/// Ours at info level and up, and everyone else's from warnings up. With `detailed`, ours and
/// librespot's from debug level up. Trace level never shows: librespot logs access tokens there.
pub fn wanted(target: &str, level: Level, detailed: bool) -> bool {
	let ours = target.starts_with("foo_librespot");
	if detailed && (ours || target.starts_with("librespot")) {
		level <= Level::DEBUG
	} else if ours {
		level <= Level::INFO
	} else {
		level <= Level::WARN
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

/// Fields as `name=value`, with the value's `Debug` form, separated by single spaces. Each span
/// keeps its own in its extensions.
///
/// Record a URL as `url = ?url.as_str()`, never `?url` on a `url::Url` or `%url`. A
/// `url::Url`'s `Debug` form lists its parts without `://`, so [`redact_urls`] can't find the
/// password and query in it; `%url` leaves the URL unquoted, so redaction also cuts whatever
/// follows it up to the next space, such as a span's closing `}:`. As a quoted string, the
/// closing quote ends the URL.
#[derive(Default)]
struct FieldList(String);

impl FieldList {
	fn push(&mut self, field: &Field, value: &dyn fmt::Debug) {
		if !self.0.is_empty() {
			self.0.push(' ');
		}
		let _ = write!(self.0, "{}={value:?}", field.name());
	}
}

impl Visit for FieldList {
	fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
		self.push(field, value);
	}
}

/// An event's message, as it is, and its other fields, without the ones tracing-log adds to a
/// `log` record.
#[derive(Default)]
struct EventFields {
	message: String,
	others: FieldList,
}

impl Visit for EventFields {
	fn record_str(&mut self, field: &Field, value: &str) {
		if field.name() == "message" {
			self.message.push_str(value);
		} else {
			self.record_debug(field, &value);
		}
	}

	fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
		match field.name() {
			"message" => {
				let _ = write!(self.message, "{value:?}");
			}
			"log.target" | "log.module_path" | "log.file" | "log.line" => {}
			_ => self.others.push(field, value),
		}
	}
}

/// Prints each event [`wanted`] lets through as one console line: "foo_librespot: ", the spans it
/// happened in from the outermost, its message and its other fields, with every URL redacted.
pub struct ConsoleLayer;

impl<S> Layer<S> for ConsoleLayer
where
	S: Subscriber + for<'a> LookupSpan<'a>,
{
	/// Trace never shows. Whether anything else does depends on the switch, which can change at
	/// any time, so it is asked again each time.
	fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
		if *metadata.level() == Level::TRACE { Interest::never() } else { Interest::sometimes() }
	}

	fn max_level_hint(&self) -> Option<LevelFilter> {
		Some(LevelFilter::DEBUG)
	}

	fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
		wanted(metadata.target(), *metadata.level(), DETAILED.load(Ordering::Relaxed))
	}

	fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
		if let Some(span) = ctx.span(id) {
			let mut fields = FieldList::default();
			attributes.record(&mut fields);
			span.extensions_mut().insert(fields);
		}
	}

	fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
		if let Some(span) = ctx.span(id)
			&& let Some(fields) = span.extensions_mut().get_mut::<FieldList>()
		{
			values.record(fields);
		}
	}

	fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
		// `enabled` has usually let this event through already, but it is asked again here: the
		// switch may have changed since, and an event dispatched straight to the subscriber skips
		// `enabled`. A bridged `log` record's event carries the bridge's target `log`, so its own
		// target and level come from `normalized_metadata`.
		let normalized = event.normalized_metadata();
		let metadata = normalized.as_ref().unwrap_or_else(|| event.metadata());
		let target = metadata.target();
		if !wanted(target, *metadata.level(), DETAILED.load(Ordering::Relaxed)) {
			return;
		}
		let mut line = String::from("foo_librespot: ");
		if let Some(scope) = ctx.event_scope(event) {
			for span in scope.from_root() {
				line.push_str(span.name());
				if let Some(FieldList(fields)) = span.extensions().get::<FieldList>()
					&& !fields.is_empty()
				{
					let _ = write!(line, "{{{fields}}}");
				}
				line.push_str(": ");
			}
		}
		let mut fields = EventFields::default();
		event.record(&mut fields);
		if held_back(target, &fields.message) {
			return;
		}
		line.push_str(&fields.message);
		if !fields.others.0.is_empty() {
			if !fields.message.is_empty() {
				line.push(' ');
			}
			line.push_str(&fields.others.0);
		}
		foxide2k::console::print(&redact_urls(&line));
	}
}

/// The registry, which keeps track of spans, with [`ConsoleLayer`] on top.
pub fn console_subscriber() -> impl Subscriber + Send + Sync {
	Registry::default().with(ConsoleLayer)
}

/// Turns detailed logging on or off, from the next message on.
pub fn set_detailed(on: bool) {
	DETAILED.store(on, Ordering::Relaxed);
	// librespot's `log` macros check this level before they format anything.
	log::set_max_level(if on { log::LevelFilter::Debug } else { log::LevelFilter::Info });
}

/// Sends our and librespot's log messages to the console from now on, in detail if the stored
/// switch says so. librespot's `log` records come through tracing-log. Called once, at startup; a
/// second call, or another `log` logger already in place, changes nothing.
pub fn init() {
	// `LogTracer::init` sets `log`'s level to trace, so the switch comes after it.
	if tracing_log::LogTracer::init().is_ok()
		&& tracing::subscriber::set_global_default(console_subscriber()).is_ok()
	{
		set_detailed(settings::detailed_logging());
	}
}

#[cfg(test)]
mod tests {
	use std::borrow::Cow;

	use foxide2k::testing::FakeApi;
	use log::LevelFilter;
	use tracing::Level;

	use super::{console_subscriber, held_back, init, redact_urls, set_detailed, wanted};
	use crate::settings;
	use crate::testing::lock_shared_state;

	#[test]
	fn logging_shows_our_info_and_only_warnings_from_others_by_default() {
		assert!(wanted("foo_librespot::links", Level::INFO, false));
		assert!(wanted("foo_librespot_core::session", Level::INFO, false));
		assert!(!wanted("foo_librespot", Level::DEBUG, false));
		assert!(!wanted("librespot_core::session", Level::INFO, false));
		assert!(wanted("librespot_core::session", Level::WARN, false));
		// librespot logs access tokens at trace level; they must never reach the console.
		assert!(!wanted("librespot_core::token", Level::TRACE, false));
	}

	#[test]
	fn detailed_logging_shows_debug_from_us_and_librespot() {
		for target in
			["foo_librespot::links", "foo_librespot_core::resolve", "librespot_core::session"]
		{
			assert!(wanted(target, Level::DEBUG, true), "{target}");
		}
	}

	#[test]
	fn detailed_logging_never_shows_trace() {
		assert!(!wanted("librespot_core::token", Level::TRACE, true));
		assert!(!wanted("foo_librespot", Level::TRACE, true));
	}

	#[test]
	fn detailed_logging_keeps_other_crates_at_warnings() {
		assert!(!wanted("hyper_util::client", Level::DEBUG, true));
		assert!(!wanted("tokio", Level::INFO, true));
		assert!(wanted("tokio", Level::WARN, true));
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

	/// What reaches a new fake console while `events` run with the console subscriber.
	fn console_lines_of(events: impl FnOnce()) -> Vec<String> {
		let api = FakeApi::new();
		api.install();
		tracing::subscriber::with_default(console_subscriber(), events);
		api.console_lines()
	}

	#[test]
	fn the_console_gets_no_dumps_and_no_signed_urls() {
		let _shared = lock_shared_state();
		let lines = console_lines_of(|| {
			set_detailed(true);
			tracing::debug!(
				target: "librespot_core::session",
				"could not dispatch command: payload: [[1, 2]]"
			);
			tracing::warn!(
				target: "librespot_audio::fetch",
				"Fetching https://audio4-fa.scdn.co/audio/ab12?1700000000_Sig-x returned 403 (expected 206 Partial Content), trying next"
			);
		});
		assert_eq!(lines.len(), 1, "{lines:?}");
		assert!(lines[0].starts_with("foo_librespot: Fetching https://"), "{lines:?}");
		assert!(lines[0].contains("?…") && !lines[0].contains("Sig"), "{lines:?}");
	}

	#[test]
	fn spans_show_before_the_message() {
		let _shared = lock_shared_state();
		let lines = console_lines_of(|| {
			let span =
				tracing::info_span!(target: "foo_librespot::search", "search", words = "getty");
			let _in = span.enter();
			tracing::info!(target: "foo_librespot::search", found = 2, "listed");
		});
		assert_eq!(lines, ["foo_librespot: search{words=\"getty\"}: listed found=2"]);
	}

	#[test]
	fn fields_without_a_message_follow_a_single_space() {
		let _shared = lock_shared_state();
		let lines = console_lines_of(|| {
			tracing::info!(target: "foo_librespot::x", found = 2);
		});
		assert_eq!(lines, ["foo_librespot: found=2"]);
	}

	#[test]
	fn nested_spans_show_from_the_outermost_with_fields_recorded_later() {
		let _shared = lock_shared_state();
		let lines = console_lines_of(|| {
			let play = tracing::info_span!(target: "foo_librespot::input", "play");
			let _play = play.enter();
			let open = tracing::info_span!(
				target: "foo_librespot::input",
				"open",
				track = "ab12",
				tries = tracing::field::Empty
			);
			open.record("tries", 2);
			let _open = open.enter();
			tracing::info!(target: "foo_librespot::input", "opened");
		});
		assert_eq!(lines, ["foo_librespot: play: open{track=\"ab12\" tries=2}: opened"]);
	}

	#[test]
	fn fields_and_span_fields_are_redacted() {
		let _shared = lock_shared_state();
		let lines = console_lines_of(|| {
			let span = tracing::info_span!(
				target: "foo_librespot::input",
				"play",
				link = %"https://b.example/y?token=2"
			);
			let _in = span.enter();
			tracing::warn!(target: "foo_librespot::input", url = %"https://a.scdn.co/x?sig=1", "no audio");
		});
		assert_eq!(lines.len(), 1, "{lines:?}");
		assert!(lines[0].contains("https://a.scdn.co/x?…"), "{lines:?}");
		assert!(lines[0].contains("https://b.example/y?…"), "{lines:?}");
		assert!(!lines[0].contains("sig=1") && !lines[0].contains("token=2"), "{lines:?}");
	}

	#[test]
	fn turning_detail_on_shows_debug_from_a_callsite_already_seen() {
		let _shared = lock_shared_state();
		fn detail() {
			tracing::debug!(target: "foo_librespot::links", "detail");
		}
		let lines = console_lines_of(|| {
			set_detailed(false);
			detail();
			set_detailed(true);
			detail();
		});
		assert_eq!(lines, ["foo_librespot: detail"]);
	}

	#[test]
	fn other_crates_stay_at_warnings_even_in_detail() {
		let _shared = lock_shared_state();
		let lines = console_lines_of(|| {
			set_detailed(true);
			tracing::trace!(target: "h2::codec", "received frame");
			tracing::debug!(target: "h2::codec", "decoded frame");
			tracing::warn!(target: "h2::codec", "connection reset");
		});
		assert_eq!(lines, ["foo_librespot: connection reset"]);
	}

	#[test]
	fn trace_never_shows() {
		let _shared = lock_shared_state();
		let lines = console_lines_of(|| {
			set_detailed(true);
			tracing::trace!(target: "librespot_core::token", "access token: abc");
			tracing::trace!(target: "foo_librespot", "step");
		});
		assert!(lines.is_empty(), "{lines:?}");
	}

	#[test]
	fn the_switch_sets_the_log_level_at_once() {
		let _shared = lock_shared_state();
		set_detailed(true);
		assert_eq!(log::max_level(), LevelFilter::Debug);
		set_detailed(false);
		assert_eq!(log::max_level(), LevelFilter::Info);
	}

	// The three tests below call the global `init`, which takes effect only the first time in a
	// process. They rely on cargo-nextest running each test in its own process, as CI and
	// AGENTS.md do; under plain `cargo test` they share one and depend on the order they run in.

	#[test]
	fn init_applies_the_stored_switch() {
		let _shared = lock_shared_state();
		FakeApi::new().install();
		settings::set_detailed_logging(true).unwrap();
		init();
		assert_eq!(log::max_level(), LevelFilter::Debug);
	}

	/// librespot logs through `log`; tracing-log hands its records on with the target `log`, but
	/// the console filters and holds them back by their own target.
	#[test]
	fn bridged_log_records_keep_their_own_target() {
		let _shared = lock_shared_state();
		let api = FakeApi::new();
		api.install();
		init();
		set_detailed(true);
		log::debug!(target: "librespot_core::session", "could not dispatch command: x");
		log::debug!(target: "librespot_core::session", "Connecting to AP");
		log::warn!(target: "librespot_audio::fetch", "Fetching https://a.scdn.co/x?sig=1");
		assert_eq!(
			api.console_lines(),
			["foo_librespot: Connecting to AP", "foo_librespot: Fetching https://a.scdn.co/x?…"]
		);
	}

	#[test]
	fn a_second_init_changes_nothing() {
		let _shared = lock_shared_state();
		let api = FakeApi::new();
		api.install();
		settings::set_detailed_logging(true).unwrap();
		init();
		init();
		assert_eq!(log::max_level(), LevelFilter::Debug);
		log::debug!(target: "librespot_core::session", "Connecting to AP");
		assert_eq!(api.console_lines(), ["foo_librespot: Connecting to AP"]);
	}
}
