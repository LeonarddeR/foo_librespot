//! Saving the Add to playlist window's ticks: every change starts at once, and each says in the
//! console how it ended. Once all have ended, the failures share one popup, so a save interrupts
//! the user at most once, however many changes fail and however far apart.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use foo_librespot_core::CoreError;
use foo_librespot_core::library::{Change, Edit, Target};
use foo_librespot_core::link::ItemId;
use foxide2k::console;
use foxide2k::popup::PopupIcon;

use crate::backend::backend;
use crate::errors::{log_failure, show_failure, show_message};
use crate::links::plural;

/// The title of the popups that say a playlist can't be changed: the one that says which
/// changes failed, and the one that says the Add to playlist window can't open.
pub const CANT_CHANGE: &str = "Can't change the Spotify playlist";

/// A change the user made: the items go to `target`, or out of it.
#[derive(Debug, PartialEq, Eq)]
pub struct Planned {
	pub edit: Edit,
	pub target: Target,
	pub name: String,
}

/// Starts each of `planned` with `items`, and returns at once.
pub fn save(items: &[ItemId], planned: Vec<Planned>) {
	let saving = Arc::new(Mutex::new(Saving { running: planned.len(), failures: BTreeMap::new() }));
	for (index, change) in planned.into_iter().enumerate() {
		let saving = Arc::clone(&saving);
		let (edit, target) = (change.edit, change.target);
		let on_done = Box::new(move |result| ended(&saving, index, change, result));
		backend().change(edit, target, items, on_done);
	}
}

/// What the changes of one save share.
struct Saving {
	/// How many changes haven't ended yet.
	running: usize,
	/// The name and error of each change that failed, by its place among the planned changes.
	failures: BTreeMap<usize, (String, CoreError)>,
}

/// Reports how `change`, the `index`th of its save, ended. The last change of the save to end
/// shows the failures.
fn ended(saving: &Mutex<Saving>, index: usize, change: Planned, result: Result<Change, CoreError>) {
	let failure = match result {
		Ok(done) => {
			report(&change, done);
			None
		}
		Err(CoreError::Aborted) => None,
		Err(error) => {
			log_failure(&format!("can't change {}", change.name), &error);
			Some((change.name, error))
		}
	};
	let failures = {
		let mut saving = saving.lock().unwrap_or_else(PoisonError::into_inner);
		if let Some(failure) = failure {
			saving.failures.insert(index, failure);
		}
		saving.running -= 1;
		if saving.running > 0 {
			return;
		}
		std::mem::take(&mut saving.failures)
	};
	show_failures(&failures.into_values().collect::<Vec<_>>());
}

/// Shows what failed and why in one popup, or none when nothing failed.
fn show_failures(failures: &[(String, CoreError)]) {
	match failures {
		[] => {}
		[(name, error)] => show_failure(CANT_CHANGE, PopupIcon::Error, name, error),
		several => {
			let lines: Vec<String> =
				several.iter().map(|(name, error)| format!("{name}: {error}.")).collect();
			let message = format!("Couldn't change these playlists:\n\n{}", lines.join("\n"));
			show_message(CANT_CHANGE, &message, PopupIcon::Error);
		}
	}
}

/// Says in the console what `planned` did, as `change` tells.
fn report(planned: &Planned, change: Change) {
	let tracks = count_tracks(change.changed);
	match planned.edit {
		Edit::Remove => {
			console::print(&format!("foo_librespot: removed {tracks} from {}", planned.name));
		}
		Edit::Add => {
			let verb = if planned.target == Target::LikedSongs { "saved" } else { "added" };
			console::print(&format!(
				"foo_librespot: {verb} {tracks} to {}{}",
				planned.name,
				already_there(change.unchanged)
			));
			let left_out = change.episodes_left_out;
			if left_out > 0 {
				console::print(&format!(
					"foo_librespot: left out {left_out} {}: Liked Songs holds songs only",
					plural(left_out, "episode", "episodes")
				));
			}
		}
	}
}

fn count_tracks(n: usize) -> String {
	format!("{n} {}", plural(n, "track", "tracks"))
}

/// `; 2 were already there`, or nothing when none were.
fn already_there(unchanged: usize) -> String {
	match unchanged {
		0 => String::new(),
		1 => "; 1 was already there".to_owned(),
		m => format!("; {m} were already there"),
	}
}

#[cfg(test)]
mod tests {
	use foo_librespot_core::CoreError;
	use foo_librespot_core::library::{Change, Edit, Target};
	use foxide2k::popup::PopupIcon;
	use foxide2k::testing::FakeApi;

	use super::{Planned, save};
	use crate::testing::{
		FakeBackend, getty_track, installed_api_and_backend, liked_change, other_track,
		shared_change, sunday_change, sunday_set_id, wait_until,
	};

	const TITLE: &str = "Can't change the Spotify playlist";

	fn is_sunday(target: Target) -> bool {
		target == Target::Playlist(sunday_set_id())
	}

	fn gone() -> CoreError {
		CoreError::Unavailable("playlist is gone".to_owned())
	}

	/// Saves `planned` for the two Getty tracks, and waits until each of its changes has ended.
	fn saved(fake: &FakeBackend, planned: Vec<Planned>) {
		let ended = fake.changes_answered() + planned.len();
		save(&[getty_track(), other_track()], planned);
		wait_until(|| fake.changes_answered() == ended);
	}

	fn printed(api: &FakeApi, line: &str) -> bool {
		api.console_lines().iter().any(|printed| printed == line)
	}

	#[test]
	fn save_starts_each_planned_change_with_the_items() {
		let (_, fake) = installed_api_and_backend();
		saved(&fake, vec![sunday_change(Edit::Add), liked_change(Edit::Remove)]);
		assert_eq!(
			fake.changes(),
			[
				(Edit::Add, Target::Playlist(sunday_set_id()), vec![getty_track(), other_track()]),
				(Edit::Remove, Target::LikedSongs, vec![getty_track(), other_track()]),
			]
		);
	}

	#[test]
	fn each_landed_change_says_so_in_the_console_and_no_popup_shows() {
		let (api, fake) = installed_api_and_backend();
		fake.change_with(|target| {
			Ok(match target {
				Target::LikedSongs => Change { changed: 1, unchanged: 0, episodes_left_out: 1 },
				_ if is_sunday(target) => Change { changed: 2, unchanged: 1, episodes_left_out: 0 },
				Target::Playlist(_) => Change { changed: 1, ..Change::default() },
			})
		});
		saved(
			&fake,
			vec![sunday_change(Edit::Add), liked_change(Edit::Add), shared_change(Edit::Remove)],
		);
		let mut lines = api.console_lines();
		lines.sort();
		let mut expected = [
			"foo_librespot: added 2 tracks to Church / Sunday set; 1 was already there",
			"foo_librespot: saved 1 track to Liked Songs",
			"foo_librespot: left out 1 episode: Liked Songs holds songs only",
			"foo_librespot: removed 1 track from Shared",
		];
		expected.sort_unstable();
		assert_eq!(lines, expected);
		assert_eq!(api.popups(), []);
	}

	#[test]
	fn leaving_out_several_episodes_says_episodes() {
		let (api, fake) = installed_api_and_backend();
		fake.change_with(|_| Ok(Change { changed: 1, unchanged: 0, episodes_left_out: 2 }));
		saved(&fake, vec![liked_change(Edit::Add)]);
		assert!(
			printed(api, "foo_librespot: left out 2 episodes: Liked Songs holds songs only"),
			"{:?}",
			api.console_lines()
		);
	}

	#[test]
	fn one_failure_among_several_shows_one_popup_for_it() {
		let (api, fake) = installed_api_and_backend();
		fake.change_with(|target| {
			if is_sunday(target) {
				return Err(gone());
			}
			std::thread::sleep(std::time::Duration::from_millis(100));
			Ok(Change { changed: 2, ..Change::default() })
		});
		save(
			&[getty_track(), other_track()],
			vec![sunday_change(Edit::Add), shared_change(Edit::Add)],
		);
		wait_until(|| !api.popups().is_empty());
		assert!(printed(api, "foo_librespot: added 2 tracks to Shared"), "the popup came too soon");
		wait_until(|| fake.changes_answered() == 2);
		let why = gone().to_string();
		assert_eq!(
			api.popups(),
			[(TITLE.to_owned(), format!("Church / Sunday set\n\n{why}."), PopupIcon::Error)]
		);
		assert!(printed(api, &format!("foo_librespot: can't change Church / Sunday set: {why}")));
	}

	#[test]
	fn several_failures_share_one_popup_in_planned_order() {
		let (api, fake) = installed_api_and_backend();
		fake.change_with(|target| {
			if is_sunday(target) {
				// Ends last, so the popup can't list the failures as they came.
				std::thread::sleep(std::time::Duration::from_millis(100));
				return Err(gone());
			}
			Err(CoreError::Unavailable("Spotify said no".to_owned()))
		});
		saved(&fake, vec![sunday_change(Edit::Add), shared_change(Edit::Remove)]);
		let message = format!(
			"Couldn't change these playlists:\n\nChurch / Sunday set: {}.\nShared: Spotify said no.",
			gone()
		);
		assert_eq!(api.popups(), [(TITLE.to_owned(), message, PopupIcon::Error)]);
		assert!(printed(api, "foo_librespot: can't change Shared: Spotify said no"));
	}

	#[test]
	fn a_cancelled_change_shows_nothing() {
		let (api, fake) = installed_api_and_backend();
		fake.change_with(|_| Err(CoreError::Aborted));
		saved(&fake, vec![shared_change(Edit::Add)]);
		assert_eq!(api.popups(), []);
		assert!(api.console_lines().is_empty(), "{:?}", api.console_lines());
	}

	#[test]
	fn a_cancelled_change_is_not_counted_among_the_failures() {
		let (api, fake) = installed_api_and_backend();
		fake.change_with(
			|target| if is_sunday(target) { Err(CoreError::Aborted) } else { Err(gone()) },
		);
		saved(&fake, vec![sunday_change(Edit::Add), shared_change(Edit::Add)]);
		let why = gone().to_string();
		assert_eq!(
			api.popups(),
			[(TITLE.to_owned(), format!("Shared\n\n{why}."), PopupIcon::Error)]
		);
	}
}
