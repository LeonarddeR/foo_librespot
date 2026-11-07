//! Starting the Spotify side once, on first use, and stopping it for good when foobar2000 quits.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use foo_librespot_core::CoreError;

/// Where a [`Lifecycle`] is.
pub enum Phase<T> {
	NotStarted,
	/// A start runs on some thread.
	Starting,
	Running(Arc<T>),
	/// The last start failed, for this reason; the next call tries again.
	Failed(String),
	ShutDown,
}

impl<T> Clone for Phase<T> {
	fn clone(&self) -> Self {
		match self {
			Self::NotStarted => Self::NotStarted,
			Self::Starting => Self::Starting,
			Self::Running(value) => Self::Running(Arc::clone(value)),
			Self::Failed(reason) => Self::Failed(reason.clone()),
			Self::ShutDown => Self::ShutDown,
		}
	}
}

/// A value that is started on first use, shared by every caller, and shut down once.
pub struct Lifecycle<T> {
	phase: Mutex<Phase<T>>,
	/// Signalled whenever a start ends.
	started: Condvar,
}

/// Ends a start: wakes whoever waits for it, and turns a start that panicked back into
/// [`Phase::NotStarted`].
struct StartEnd<'a, T>(&'a Lifecycle<T>);

impl<T> Drop for StartEnd<'_, T> {
	fn drop(&mut self) {
		let mut phase = self.0.lock();
		if matches!(*phase, Phase::Starting) {
			*phase = Phase::NotStarted;
		}
		drop(phase);
		self.0.started.notify_all();
	}
}

impl<T> Lifecycle<T> {
	pub const fn new() -> Self {
		Self { phase: Mutex::new(Phase::NotStarted), started: Condvar::new() }
	}

	fn lock(&self) -> MutexGuard<'_, Phase<T>> {
		self.phase.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Where it is, without waiting for a start that runs.
	pub fn now(&self) -> Phase<T> {
		self.lock().clone()
	}

	/// The running value, started by `start` when none runs. A start running on another thread is
	/// waited for, and tried again when it failed. `start` runs without the lock held, so
	/// [`now`](Self::now) answers meanwhile. After [`shutdown`](Self::shutdown) every call fails
	/// with [`CoreError::Aborted`]: foobar2000 is quitting, and starting Spotify again then would
	/// leave it running.
	pub fn get_or_start(
		&self,
		start: impl FnOnce() -> Result<T, CoreError>,
	) -> Result<Arc<T>, CoreError> {
		let mut phase = self.lock();
		loop {
			match &*phase {
				Phase::Running(value) => return Ok(Arc::clone(value)),
				Phase::ShutDown => return Err(CoreError::Aborted),
				Phase::Starting => {
					phase = self.started.wait(phase).unwrap_or_else(PoisonError::into_inner);
				}
				Phase::NotStarted | Phase::Failed(_) => break,
			}
		}
		*phase = Phase::Starting;
		drop(phase);
		let _end = StartEnd(self);
		let started = start();
		let mut phase = self.lock();
		match started {
			Ok(value) if matches!(*phase, Phase::Starting) => {
				let value = Arc::new(value);
				*phase = Phase::Running(Arc::clone(&value));
				Ok(value)
			}
			Ok(value) => {
				std::mem::forget(value);
				Err(CoreError::Aborted)
			}
			Err(error) => {
				if matches!(*phase, Phase::Starting) {
					*phase = Phase::Failed(error.to_string());
				}
				Err(error)
			}
		}
	}

	/// Stops for good. A start that runs gets up to `wait` to end, and calls still using the
	/// value get what is left of it to finish (foobar2000 aborts them as it quits); then `finish`
	/// gets the value. If some are still running after that, the value is deliberately leaked
	/// rather than dropped by whichever thread finishes last, because dropping a tokio runtime
	/// waits for all of its blocking work. A start that ends later leaks its value too.
	pub fn shutdown(&self, wait: Duration, finish: impl FnOnce(T)) {
		let deadline = Instant::now() + wait;
		let phase = {
			let mut phase = self.lock();
			while matches!(*phase, Phase::Starting) {
				let left = deadline.saturating_duration_since(Instant::now());
				if left.is_zero() {
					break;
				}
				phase = self
					.started
					.wait_timeout(phase, left)
					.unwrap_or_else(PoisonError::into_inner)
					.0;
			}
			std::mem::replace(&mut *phase, Phase::ShutDown)
		};
		let Phase::Running(mut value) = phase else {
			return;
		};
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
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::sync::{Arc, mpsc};
	use std::time::{Duration, Instant};

	use foo_librespot_core::CoreError;

	use super::{Lifecycle, Phase};

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
	fn a_failed_start_says_why_and_is_tried_again() {
		let lifecycle = Lifecycle::new();
		let busy =
			|| Err::<i32, _>(CoreError::Unavailable("the profile folder is busy".to_owned()));
		assert!(lifecycle.get_or_start(busy).is_err());
		assert!(
			matches!(lifecycle.now(), Phase::Failed(reason) if reason == "the profile folder is busy")
		);
		assert_eq!(*lifecycle.get_or_start(|| Ok(7)).unwrap(), 7);
	}

	/// Waits until `lifecycle` is starting.
	fn until_starting<T>(lifecycle: &Lifecycle<T>) {
		let deadline = Instant::now() + Duration::from_secs(5);
		while !matches!(lifecycle.now(), Phase::Starting) {
			assert!(Instant::now() < deadline, "it never started");
			std::thread::sleep(Duration::from_millis(5));
		}
	}

	#[test]
	fn while_a_start_runs_others_learn_so_at_once_or_wait_for_it() {
		let lifecycle = &Lifecycle::new();
		let starts = &AtomicUsize::new(0);
		let (release, released) = mpsc::channel::<()>();
		let start = move || {
			starts.fetch_add(1, Ordering::SeqCst);
			let _ = released.recv();
			Ok(7)
		};
		std::thread::scope(|scope| {
			let first = scope.spawn(move || *lifecycle.get_or_start(start).unwrap());
			until_starting(lifecycle);
			let second = scope.spawn(|| *lifecycle.get_or_start(|| Ok(8)).unwrap());
			let asked = Instant::now();
			assert!(matches!(lifecycle.now(), Phase::Starting));
			assert!(asked.elapsed() < Duration::from_millis(100), "{:?}", asked.elapsed());
			release.send(()).unwrap();
			assert_eq!(first.join().unwrap(), 7);
			assert_eq!(second.join().unwrap(), 7);
		});
		assert_eq!(starts.load(Ordering::SeqCst), 1);
		assert!(matches!(lifecycle.now(), Phase::Running(value) if *value == 7));
	}

	#[test]
	fn a_start_that_panics_can_be_tried_again() {
		let lifecycle = Lifecycle::<i32>::new();
		let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
			lifecycle.get_or_start(|| panic!("the start broke"))
		}));
		assert!(started.is_err());
		assert!(matches!(lifecycle.now(), Phase::NotStarted));
		assert_eq!(*lifecycle.get_or_start(|| Ok(7)).unwrap(), 7);
	}

	#[test]
	fn shutdown_waits_for_a_start_in_progress_and_finishes_it() {
		let lifecycle = &Lifecycle::new();
		let (release, released) = mpsc::channel::<()>();
		let mut finished = None;
		std::thread::scope(|scope| {
			scope.spawn(move || {
				drop(lifecycle.get_or_start(|| {
					let _ = released.recv();
					Ok(7)
				}));
			});
			until_starting(lifecycle);
			scope.spawn(move || {
				std::thread::sleep(Duration::from_millis(50));
				release.send(()).unwrap();
			});
			lifecycle.shutdown(Duration::from_secs(2), |value| finished = Some(value));
		});
		assert_eq!(finished, Some(7));
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
