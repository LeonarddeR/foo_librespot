//! The audio keys Spotify has handed over, so a track played before doesn't ask for its key again.
//!
//! Spotify refuses keys for a while when many are asked for in a short time. A key belongs to one
//! audio file and never changes, so remembering it spares the request on every later play.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use librespot_core::audio_key::AudioKey;

use crate::FileId;

/// How many keys [`AudioKeys`] keeps by default, about 3.6 MB stored.
pub const AUDIO_KEY_LIMIT: usize = 100_000;

/// The first byte of the stored form. Another value means a form this version can't read.
const VERSION: u8 = 1;
const FILE_ID_LEN: usize = 20;
const KEY_LEN: usize = 16;
const RECORD_LEN: usize = FILE_ID_LEN + KEY_LEN;

/// Remembered audio keys, by audio file, up to a number of keys. When a new key doesn't fit, the
/// keys used longest ago are dropped first.
#[derive(Clone, Debug)]
pub struct AudioKeys(Arc<Mutex<StoredKeys>>);

#[derive(Debug)]
struct StoredKeys {
	limit: usize,
	clock: u64,
	keys: HashMap<FileId, StoredKey>,
}

#[derive(Debug)]
struct StoredKey {
	key: AudioKey,
	last_used: u64,
}

impl Default for AudioKeys {
	fn default() -> Self {
		Self::with_limit(AUDIO_KEY_LIMIT)
	}
}

impl AudioKeys {
	/// Keeps at most `limit` keys.
	pub fn with_limit(limit: usize) -> Self {
		Self(Arc::new(Mutex::new(StoredKeys { limit, clock: 0, keys: HashMap::new() })))
	}

	/// The key of `file`, marked as just used.
	pub fn get(&self, file: &FileId) -> Option<AudioKey> {
		self.lock().get(file)
	}

	/// Remembers `key`, dropping the key used longest ago when the limit is reached.
	pub fn insert(&self, file: FileId, key: AudioKey) {
		self.lock().insert(file, key);
	}

	pub fn forget(&self, file: &FileId) {
		self.lock().keys.remove(file);
	}

	pub fn clear(&self) {
		self.lock().keys.clear();
	}

	pub fn is_empty(&self) -> bool {
		self.lock().keys.is_empty()
	}

	/// The stored form: a version byte, then each file id and its key, the key used longest ago
	/// first.
	pub fn to_bytes(&self) -> Vec<u8> {
		let stored = self.lock();
		let mut keys: Vec<_> = stored.keys.iter().collect();
		keys.sort_unstable_by_key(|(_, stored)| stored.last_used);
		let mut bytes = Vec::with_capacity(1 + keys.len() * RECORD_LEN);
		bytes.push(VERSION);
		for (file, stored) in keys {
			bytes.extend_from_slice(&file.0);
			bytes.extend_from_slice(&stored.key.0);
		}
		bytes
	}

	/// Reads what [`to_bytes`](Self::to_bytes) wrote, keeping at most `limit` keys: the ones used
	/// most recently. `None` when the bytes aren't in that form.
	pub fn from_bytes(bytes: &[u8], limit: usize) -> Option<Self> {
		let (&VERSION, records) = bytes.split_first()? else { return None };
		let (records, []) = records.as_chunks::<RECORD_LEN>() else { return None };
		let keys = Self::with_limit(limit);
		{
			let mut stored = keys.lock();
			for record in records {
				let (file, key) = record.split_at(FILE_ID_LEN);
				stored.insert(FileId::from_raw(file), AudioKey(key.try_into().ok()?));
			}
		}
		Some(keys)
	}

	fn lock(&self) -> MutexGuard<'_, StoredKeys> {
		self.0.lock().unwrap_or_else(PoisonError::into_inner)
	}
}

impl StoredKeys {
	fn tick(&mut self) -> u64 {
		self.clock += 1;
		self.clock
	}

	fn get(&mut self, file: &FileId) -> Option<AudioKey> {
		let now = self.tick();
		let stored = self.keys.get_mut(file)?;
		stored.last_used = now;
		Some(stored.key)
	}

	fn insert(&mut self, file: FileId, key: AudioKey) {
		self.keys.remove(&file);
		while self.keys.len() >= self.limit
			&& let Some((&oldest, _)) = self.keys.iter().min_by_key(|(_, stored)| stored.last_used)
		{
			self.keys.remove(&oldest);
		}
		let now = self.tick();
		self.keys.insert(file, StoredKey { key, last_used: now });
	}
}

#[cfg(test)]
mod tests {
	use librespot_core::audio_key::AudioKey;

	use super::AudioKeys;
	use crate::FileId;

	fn file(byte: u8) -> FileId {
		FileId::from_raw(&[byte; 20])
	}

	fn key(byte: u8) -> AudioKey {
		AudioKey([byte; 16])
	}

	#[test]
	fn a_remembered_key_is_returned() {
		let keys = AudioKeys::with_limit(10);
		keys.insert(file(1), key(11));
		assert_eq!(keys.get(&file(1)), Some(key(11)));
		assert_eq!(keys.get(&file(2)), None);
		assert!(!keys.is_empty());
	}

	#[test]
	fn the_key_used_longest_ago_goes_first() {
		let keys = AudioKeys::with_limit(2);
		keys.insert(file(1), key(11));
		keys.insert(file(2), key(12));
		// Using the first key makes the second the one used longest ago.
		assert!(keys.get(&file(1)).is_some());
		keys.insert(file(3), key(13));
		assert_eq!(keys.get(&file(1)), Some(key(11)));
		assert_eq!(keys.get(&file(2)), None);
		assert_eq!(keys.get(&file(3)), Some(key(13)));
	}

	#[test]
	fn a_key_inserted_again_is_replaced_not_doubled() {
		let keys = AudioKeys::with_limit(2);
		keys.insert(file(1), key(11));
		keys.insert(file(1), key(21));
		keys.insert(file(2), key(12));
		assert_eq!(keys.get(&file(1)), Some(key(21)));
		assert_eq!(keys.get(&file(2)), Some(key(12)));
	}

	#[test]
	fn a_forgotten_key_is_gone() {
		let keys = AudioKeys::with_limit(10);
		keys.insert(file(1), key(11));
		keys.insert(file(2), key(12));
		keys.forget(&file(1));
		assert_eq!(keys.get(&file(1)), None);
		assert_eq!(keys.get(&file(2)), Some(key(12)));
	}

	#[test]
	fn clear_forgets_every_key() {
		let keys = AudioKeys::with_limit(10);
		keys.insert(file(1), key(11));
		keys.clear();
		assert_eq!(keys.get(&file(1)), None);
		assert!(keys.is_empty());
	}

	#[test]
	fn keys_come_back_from_their_bytes() {
		let keys = AudioKeys::with_limit(10);
		keys.insert(file(1), key(11));
		keys.insert(file(2), key(12));
		let bytes = keys.to_bytes();
		assert_eq!(bytes.len(), 1 + 2 * 36);
		let read = AudioKeys::from_bytes(&bytes, 10).unwrap();
		assert_eq!(read.get(&file(1)), Some(key(11)));
		assert_eq!(read.get(&file(2)), Some(key(12)));
	}

	#[test]
	fn the_bytes_keep_which_key_was_used_longest_ago() {
		let keys = AudioKeys::with_limit(10);
		keys.insert(file(1), key(11));
		keys.insert(file(2), key(12));
		assert!(keys.get(&file(1)).is_some());
		// Read back with room for one key, only the one used last stays.
		let read = AudioKeys::from_bytes(&keys.to_bytes(), 1).unwrap();
		assert_eq!(read.get(&file(1)), Some(key(11)));
		assert_eq!(read.get(&file(2)), None);
	}

	#[test]
	fn no_keys_make_a_version_byte_alone() {
		let bytes = AudioKeys::with_limit(10).to_bytes();
		assert_eq!(bytes.len(), 1);
		assert!(AudioKeys::from_bytes(&bytes, 10).unwrap().is_empty());
	}

	#[test]
	fn bytes_in_another_form_are_rejected() {
		let mut bytes = AudioKeys::with_limit(10).to_bytes();
		bytes.extend_from_slice(&[0; 36]);
		let unknown_version = [&[bytes[0] + 1][..], &bytes[1..]].concat();
		let cut_short = &bytes[..bytes.len() - 1];
		for bad in [&[][..], &unknown_version, cut_short] {
			assert!(AudioKeys::from_bytes(bad, 10).is_none(), "{bad:?}");
		}
	}
}
