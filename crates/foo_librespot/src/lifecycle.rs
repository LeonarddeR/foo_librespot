//! Starting the Spotify side once, on first use, and stopping it for good when foobar2000 quits.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use foo_librespot_core::CoreError;

enum State<T> {
	NotStarted,
	Running(Arc<T>),
	ShutDown,
}

/// A value that is started on first use, shared by every caller, and shut down once.
pub struct Lifecycle<T> {
	state: Mutex<State<T>>,
}

impl<T> Lifecycle<T> {
	pub const fn new() -> Self {
		Self { state: Mutex::new(State::NotStarted) }
	}

	/// The running value, started by `start` if this is the first call. After
	/// [`shutdown`](Self::shutdown) every call fails with [`CoreError::Aborted`]: foobar2000 is
	/// quitting, and starting Spotify again then would leave it running.
	pub fn get_or_start(
		&self,
		start: impl FnOnce() -> Result<T, CoreError>,
	) -> Result<Arc<T>, CoreError> {
		let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
		match &*state {
			State::Running(value) => Ok(Arc::clone(value)),
			State::ShutDown => Err(CoreError::Aborted),
			State::NotStarted => {
				let value = Arc::new(start()?);
				*state = State::Running(Arc::clone(&value));
				Ok(value)
			}
		}
	}

	/// Stops for good. Calls still using the value get up to `wait` to finish (foobar2000 aborts
	/// them as it quits); then `finish` gets the value. If some are still running after that,
	/// the value is deliberately leaked rather than dropped by whichever thread finishes last,
	/// because dropping a tokio runtime waits for all of its blocking work.
	pub fn shutdown(&self, wait: Duration, finish: impl FnOnce(T)) {
		let state = std::mem::replace(
			&mut *self.state.lock().unwrap_or_else(PoisonError::into_inner),
			State::ShutDown,
		);
		let State::Running(mut value) = state else {
			return;
		};
		let deadline = Instant::now() + wait;
		loop {
			match Arc::try_unwrap(value) {
				Ok(value) => return finish(value),
				Err(shared) if Instant::now() >= deadline => {
					std::mem::forget(shared);
					return;
				}
				Err(shared) => {
					value = shared;
					std::thread::sleep(Duration::from_millis(10));
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::time::{Duration, Instant};

	use foo_librespot_core::CoreError;

	use super::Lifecycle;

	#[test]
	fn starts_once_and_shares_the_value() {
		let lifecycle = Lifecycle::new();
		let starts = AtomicUsize::new(0);
		let start = || {
			starts.fetch_add(1, Ordering::SeqCst);
			Ok(7)
		};
		let first = lifecycle.get_or_start(start).unwrap();
		let second = lifecycle.get_or_start(start).unwrap();
		assert!(Arc::ptr_eq(&first, &second));
		assert_eq!(starts.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn a_failed_start_is_tried_again() {
		let lifecycle = Lifecycle::new();
		assert!(lifecycle.get_or_start(|| Err::<i32, _>(CoreError::NotLoggedIn)).is_err());
		assert_eq!(*lifecycle.get_or_start(|| Ok(7)).unwrap(), 7);
	}

	#[test]
	fn after_shutdown_nothing_starts_again() {
		let lifecycle = Lifecycle::<i32>::new();
		lifecycle.shutdown(Duration::ZERO, |_| {});
		let result = lifecycle.get_or_start(|| panic!("started after shutdown"));
		assert!(matches!(result, Err(CoreError::Aborted)), "{result:?}");
	}

	#[test]
	fn shutdown_finishes_an_unused_value() {
		let lifecycle = Lifecycle::new();
		drop(lifecycle.get_or_start(|| Ok(7)).unwrap());
		let mut finished = None;
		lifecycle.shutdown(Duration::from_secs(1), |value| finished = Some(value));
		assert_eq!(finished, Some(7));
	}

	#[test]
	fn shutdown_waits_for_a_value_in_use() {
		let lifecycle = Lifecycle::new();
		let in_use = lifecycle.get_or_start(|| Ok(7)).unwrap();
		let holder = std::thread::spawn(move || {
			std::thread::sleep(Duration::from_millis(100));
			drop(in_use);
		});
		let started = Instant::now();
		let mut finished = None;
		lifecycle.shutdown(Duration::from_secs(2), |value| finished = Some(value));
		assert_eq!(finished, Some(7));
		assert!(started.elapsed() >= Duration::from_millis(90), "{:?}", started.elapsed());
		holder.join().unwrap();
	}

	#[test]
	fn shutdown_gives_up_on_a_value_still_in_use() {
		let lifecycle = Lifecycle::new();
		let in_use = lifecycle.get_or_start(|| Ok(7)).unwrap();
		let started = Instant::now();
		let mut finished = None;
		lifecycle.shutdown(Duration::from_millis(200), |value| finished = Some(value));
		assert_eq!(finished, None);
		assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
		drop(in_use);
	}
}
