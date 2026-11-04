//! The tokio runtime the plugin owns, and blocking calls foobar2000's threads can abort.

use std::future::Future;
use std::io;
use std::time::Duration;

use crate::CoreError;

/// How often a blocking call checks whether foobar2000 wants it to stop.
pub const ABORT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The tokio runtime librespot runs on.
///
/// foobar2000 calls components on its own threads and expects them to block, so every call into
/// Spotify goes through [`CoreRuntime::block_on`], which also stops when foobar2000 aborts.
#[derive(Debug)]
pub struct CoreRuntime {
	runtime: tokio::runtime::Runtime,
}

impl CoreRuntime {
	pub fn new() -> io::Result<Self> {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(2)
			.enable_all()
			.thread_name("foo_librespot")
			.build()?;
		Ok(Self { runtime })
	}

	/// Runs `future` to completion on the calling thread, or returns [`CoreError::Aborted`]
	/// within [`ABORT_POLL_INTERVAL`] of `is_aborting` turning true, dropping the future.
	///
	/// Never call this from async code: tokio panics when a runtime thread blocks on a future.
	pub fn block_on<F: Future>(
		&self,
		future: F,
		is_aborting: &dyn Fn() -> bool,
	) -> Result<F::Output, CoreError> {
		if is_aborting() {
			return Err(CoreError::Aborted);
		}
		self.runtime.block_on(async {
			tokio::select! {
				output = future => Ok(output),
				() = wait_for_abort(is_aborting) => Err(CoreError::Aborted),
			}
		})
	}

	pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
	where
		F: Future + Send + 'static,
		F::Output: Send + 'static,
	{
		self.runtime.spawn(future)
	}

	pub fn handle(&self) -> tokio::runtime::Handle {
		self.runtime.handle().clone()
	}

	/// Stops the runtime, giving running tasks at most `timeout` to finish.
	pub fn shutdown(self, timeout: Duration) {
		self.runtime.shutdown_timeout(timeout);
	}
}

// Not Send on purpose: it only runs inside `block_on`, on the thread that called it.
#[allow(clippy::future_not_send)]
async fn wait_for_abort(is_aborting: &dyn Fn() -> bool) {
	loop {
		tokio::time::sleep(ABORT_POLL_INTERVAL).await;
		if is_aborting() {
			return;
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::time::{Duration, Instant};

	use super::CoreRuntime;
	use crate::CoreError;

	/// Sets its flag when dropped, to prove a future was dropped.
	struct DropFlag(Arc<AtomicBool>);

	impl Drop for DropFlag {
		fn drop(&mut self) {
			self.0.store(true, Ordering::SeqCst);
		}
	}

	fn never() -> bool {
		false
	}

	#[test]
	fn block_on_returns_the_output() {
		let runtime = CoreRuntime::new().unwrap();
		assert_eq!(runtime.block_on(async { 42 }, &never).unwrap(), 42);
	}

	#[test]
	fn already_aborted_never_polls_the_future() {
		let runtime = CoreRuntime::new().unwrap();
		let polled = AtomicBool::new(false);
		let result = runtime.block_on(async { polled.store(true, Ordering::SeqCst) }, &|| true);
		assert!(matches!(result, Err(CoreError::Aborted)), "{result:?}");
		assert!(!polled.load(Ordering::SeqCst));
	}

	#[test]
	fn block_on_stops_when_aborted_and_drops_the_future() {
		let runtime = CoreRuntime::new().unwrap();
		let aborting = Arc::new(AtomicBool::new(false));
		let dropped = Arc::new(AtomicBool::new(false));
		let setter = {
			let aborting = Arc::clone(&aborting);
			std::thread::spawn(move || {
				std::thread::sleep(Duration::from_millis(100));
				aborting.store(true, Ordering::SeqCst);
			})
		};
		let guard = DropFlag(Arc::clone(&dropped));
		let started = Instant::now();
		let result = runtime.block_on(
			async move {
				let _guard = guard;
				std::future::pending::<()>().await;
			},
			&|| aborting.load(Ordering::SeqCst),
		);
		assert!(matches!(result, Err(CoreError::Aborted)), "{result:?}");
		assert!(started.elapsed() < Duration::from_secs(1));
		assert!(dropped.load(Ordering::SeqCst));
		setter.join().unwrap();
	}

	#[test]
	fn block_on_from_four_threads_at_once() {
		let runtime = Arc::new(CoreRuntime::new().unwrap());
		// Start all four before joining any, so the calls overlap.
		let mut threads = Vec::new();
		for index in 0..4 {
			let runtime = Arc::clone(&runtime);
			threads.push(std::thread::spawn(move || {
				runtime.block_on(
					async move {
						tokio::time::sleep(Duration::from_millis(50)).await;
						index
					},
					&never,
				)
			}));
		}
		for (index, thread) in threads.into_iter().enumerate() {
			assert_eq!(thread.join().unwrap().unwrap(), index);
		}
	}

	#[test]
	fn futures_see_the_runtime() {
		let runtime = CoreRuntime::new().unwrap();
		let inside =
			runtime.block_on(async { tokio::runtime::Handle::try_current().is_ok() }, &never);
		assert!(inside.unwrap());
	}

	#[test]
	fn shutdown_returns_despite_a_stuck_task() {
		let runtime = CoreRuntime::new().unwrap();
		let _task = runtime.spawn(std::future::pending::<()>());
		let started = Instant::now();
		runtime.shutdown(Duration::from_millis(100));
		assert!(started.elapsed() < Duration::from_secs(1));
	}
}
