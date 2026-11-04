//! librespot's and our own log messages, in the foobar2000 console.

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Ours at info level and up; everyone else's only from warnings up. librespot is chatty at
/// info level and logs access tokens at trace level, which must never reach the console.
pub fn wanted(target: &str, level: Level) -> bool {
	if target.starts_with("foo_librespot") { level <= Level::Info } else { level <= Level::Warn }
}

struct ConsoleLogger;

impl Log for ConsoleLogger {
	fn enabled(&self, metadata: &Metadata<'_>) -> bool {
		wanted(metadata.target(), metadata.level())
	}

	fn log(&self, record: &Record<'_>) {
		if self.enabled(record.metadata()) {
			foxide2k::console::print(&format!("foo_librespot: {}", record.args()));
		}
	}

	fn flush(&self) {}
}

static LOGGER: ConsoleLogger = ConsoleLogger;

/// Sends log messages to the console from now on. Called once, at startup; a second call, or
/// another logger already in place, changes nothing.
pub fn init() {
	if log::set_logger(&LOGGER).is_ok() {
		log::set_max_level(LevelFilter::Info);
	}
}

#[cfg(test)]
mod tests {
	use log::Level;

	use super::wanted;

	#[test]
	fn logging_shows_our_info_and_only_warnings_from_others() {
		assert!(wanted("foo_librespot::links", Level::Info));
		assert!(wanted("foo_librespot_core::session", Level::Info));
		assert!(!wanted("foo_librespot", Level::Debug));
		assert!(!wanted("librespot_core::session", Level::Info));
		assert!(wanted("librespot_core::session", Level::Warn));
		// librespot logs access tokens at trace level; they must never reach the console.
		assert!(!wanted("librespot_core::token", Level::Trace));
	}
}
