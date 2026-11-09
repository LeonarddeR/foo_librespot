//! Album art for Spotify tracks.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use librespot_core::Session;

use crate::metadata::Cover;
use crate::{CoreError, FileId};

/// Spotify's public image server, which librespot also falls back to.
pub fn fallback_image_url(file_id: &FileId) -> String {
	format!("https://i.scdn.co/image/{}", file_id.to_base16())
}

/// Downloads one cover image, as JPEG bytes.
pub async fn fetch_cover(session: &Session, cover: &Cover) -> Result<Vec<u8>, CoreError> {
	tracing::debug!("downloading cover {}", cover.file_id.to_base16());
	match session.spclient().get_image(&cover.file_id).await {
		Ok(bytes) => Ok(bytes.to_vec()),
		Err(error) => {
			tracing::debug!(
				"cover through the image-url attribute failed ({error}); trying i.scdn.co"
			);
			let url = fallback_image_url(&cover.file_id);
			Ok(session.spclient().request_url(&url).await?.to_vec())
		}
	}
}

/// How many bytes of cover images [`CoverCache`] keeps by default: a few hundred covers.
pub const COVER_CACHE_LIMIT: usize = 32 * 1024 * 1024;

/// Cover images downloaded this run, by image, up to a total size. When a new image doesn't fit,
/// the images read longest ago are dropped first.
#[derive(Clone, Debug)]
pub struct CoverCache(Arc<Mutex<StoredCovers>>);

#[derive(Debug)]
struct StoredCovers {
	limit: usize,
	size: usize,
	clock: u64,
	images: HashMap<FileId, StoredCover>,
}

#[derive(Debug)]
struct StoredCover {
	jpeg: Arc<[u8]>,
	last_read: u64,
}

impl Default for CoverCache {
	fn default() -> Self {
		Self::with_limit(COVER_CACHE_LIMIT)
	}
}

impl CoverCache {
	/// Keeps at most `limit` bytes of images.
	pub fn with_limit(limit: usize) -> Self {
		Self(Arc::new(Mutex::new(StoredCovers {
			limit,
			size: 0,
			clock: 0,
			images: HashMap::new(),
		})))
	}

	pub fn get(&self, id: &FileId) -> Option<Arc<[u8]>> {
		self.lock().get(id)
	}

	/// Stores `jpeg`, dropping the images read longest ago until it fits. An image larger than
	/// the whole limit is not stored.
	pub fn insert(&self, id: FileId, jpeg: Arc<[u8]>) {
		self.lock().insert(id, jpeg);
	}

	/// The image `id`, calling `fetch` only when it isn't stored. A failed fetch stores nothing.
	pub async fn get_or_fetch<F, Fut>(&self, id: FileId, fetch: F) -> Result<Arc<[u8]>, CoreError>
	where
		F: FnOnce() -> Fut,
		Fut: Future<Output = Result<Vec<u8>, CoreError>>,
	{
		if let Some(jpeg) = self.get(&id) {
			return Ok(jpeg);
		}
		let jpeg: Arc<[u8]> = fetch().await?.into();
		self.insert(id, Arc::clone(&jpeg));
		Ok(jpeg)
	}

	fn lock(&self) -> MutexGuard<'_, StoredCovers> {
		self.0.lock().unwrap_or_else(PoisonError::into_inner)
	}
}

impl StoredCovers {
	fn tick(&mut self) -> u64 {
		self.clock += 1;
		self.clock
	}

	fn get(&mut self, id: &FileId) -> Option<Arc<[u8]>> {
		let now = self.tick();
		let cover = self.images.get_mut(id)?;
		cover.last_read = now;
		Some(Arc::clone(&cover.jpeg))
	}

	fn insert(&mut self, id: FileId, jpeg: Arc<[u8]>) {
		if let Some(old) = self.images.remove(&id) {
			self.size -= old.jpeg.len();
		}
		if jpeg.len() > self.limit {
			return;
		}
		while self.size + jpeg.len() > self.limit
			&& let Some((&oldest, cover)) =
				self.images.iter().min_by_key(|(_, cover)| cover.last_read)
		{
			self.size -= cover.jpeg.len();
			self.images.remove(&oldest);
		}
		let now = self.tick();
		self.size += jpeg.len();
		self.images.insert(id, StoredCover { jpeg, last_read: now });
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};

	use super::{CoverCache, fallback_image_url};
	use crate::{CoreError, FileId};

	fn image(byte: u8) -> FileId {
		FileId::from_raw(&[byte; 20])
	}

	fn jpeg(len: usize) -> Arc<[u8]> {
		vec![0xff; len].into()
	}

	#[test]
	fn fallback_image_url_uses_base16() {
		let url = fallback_image_url(&FileId::from_raw(&[0xab; 20]));
		assert_eq!(url, format!("https://i.scdn.co/image/{}", "ab".repeat(20)));
	}

	#[test]
	fn a_stored_cover_is_returned() {
		let cache = CoverCache::with_limit(10);
		cache.insert(image(1), jpeg(4));
		assert_eq!(cache.get(&image(1)).as_deref(), Some(&[0xff; 4][..]));
		assert!(cache.get(&image(2)).is_none());
	}

	#[test]
	fn the_cover_read_longest_ago_goes_first() {
		let cache = CoverCache::with_limit(10);
		cache.insert(image(1), jpeg(4));
		cache.insert(image(2), jpeg(4));
		cache.insert(image(3), jpeg(4));
		assert!(cache.get(&image(1)).is_none());
		assert!(cache.get(&image(2)).is_some());
		assert!(cache.get(&image(3)).is_some());
	}

	#[test]
	fn reading_a_cover_keeps_it() {
		let cache = CoverCache::with_limit(10);
		cache.insert(image(1), jpeg(4));
		cache.insert(image(2), jpeg(4));
		assert!(cache.get(&image(1)).is_some());
		cache.insert(image(3), jpeg(4));
		assert!(cache.get(&image(1)).is_some());
		assert!(cache.get(&image(2)).is_none());
		assert!(cache.get(&image(3)).is_some());
	}

	#[test]
	fn a_cover_stored_twice_counts_once() {
		let cache = CoverCache::with_limit(10);
		cache.insert(image(1), jpeg(4));
		cache.insert(image(1), jpeg(4));
		cache.insert(image(2), jpeg(6));
		assert!(cache.get(&image(1)).is_some());
		assert!(cache.get(&image(2)).is_some());
	}

	#[test]
	fn a_cover_larger_than_the_limit_is_not_kept() {
		let cache = CoverCache::with_limit(10);
		cache.insert(image(1), jpeg(4));
		cache.insert(image(2), jpeg(11));
		assert!(cache.get(&image(2)).is_none());
		assert!(cache.get(&image(1)).is_some());
	}

	#[tokio::test]
	async fn each_cover_is_downloaded_once() {
		let cache = CoverCache::with_limit(10);
		let downloads = AtomicUsize::new(0);
		let download = || {
			downloads.fetch_add(1, Ordering::SeqCst);
			async { Ok(vec![0xff; 4]) }
		};
		let first = cache.get_or_fetch(image(1), download).await.unwrap();
		let second = cache.get_or_fetch(image(1), download).await.unwrap();
		assert_eq!(first, second);
		assert_eq!(downloads.load(Ordering::SeqCst), 1);
		cache.get_or_fetch(image(2), download).await.unwrap();
		assert_eq!(downloads.load(Ordering::SeqCst), 2);
	}

	#[tokio::test]
	async fn a_failed_download_is_tried_again() {
		let cache = CoverCache::with_limit(10);
		let downloads = AtomicUsize::new(0);
		let failing = || {
			downloads.fetch_add(1, Ordering::SeqCst);
			async { Err(CoreError::Unavailable("no cover".to_owned())) }
		};
		assert!(cache.get_or_fetch(image(1), failing).await.is_err());
		assert!(cache.get_or_fetch(image(1), failing).await.is_err());
		assert_eq!(downloads.load(Ordering::SeqCst), 2);
	}
}
