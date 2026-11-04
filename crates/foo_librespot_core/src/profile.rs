//! What foo_librespot keeps between runs: a stable device id and the encrypted Spotify login.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use librespot_core::SessionConfig;

use crate::Credentials;
use crate::dpapi;

const DEVICE_ID_FILE: &str = "device_id";
const CREDENTIALS_FILE: &str = "credentials.bin";

/// Why the profile folder couldn't be used.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
	#[error("can't use the foo_librespot profile folder: {0}")]
	Io(#[from] io::Error),
	/// The file exists but can't be decrypted or parsed, for example because it was copied
	/// from another Windows account.
	#[error("the stored Spotify login can't be read: {0}")]
	Unreadable(String),
}

/// The folder where foo_librespot keeps its state.
#[derive(Debug)]
pub struct Profile {
	dir: PathBuf,
}

impl Profile {
	pub fn new(dir: impl Into<PathBuf>) -> Self {
		Self { dir: dir.into() }
	}

	pub fn dir(&self) -> &Path {
		&self.dir
	}

	/// The device id Spotify sees. librespot makes a new random one every time, so the first one
	/// is kept, and Spotify sees one device instead of a new one per run.
	pub fn device_id(&self) -> Result<String, ProfileError> {
		let path = self.dir.join(DEVICE_ID_FILE);
		match fs::read_to_string(&path) {
			Ok(id) if !id.trim().is_empty() => return Ok(id.trim().to_owned()),
			Ok(_) => {}
			Err(error) if error.kind() == io::ErrorKind::NotFound => {}
			Err(error) => return Err(error.into()),
		}
		let id = SessionConfig::default().device_id;
		self.write(&path, id.as_bytes())?;
		Ok(id)
	}

	/// The stored login, or `None` when nobody has logged in.
	pub fn load_credentials(&self) -> Result<Option<Credentials>, ProfileError> {
		let cipher = match fs::read(self.credentials_path()) {
			Ok(cipher) => cipher,
			Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
			Err(error) => return Err(error.into()),
		};
		let plain = dpapi::unprotect(&cipher)
			.map_err(|error| ProfileError::Unreadable(error.to_string()))?;
		serde_json::from_slice(&plain)
			.map(Some)
			.map_err(|error| ProfileError::Unreadable(error.to_string()))
	}

	pub fn save_credentials(&self, credentials: &Credentials) -> Result<(), ProfileError> {
		let plain = serde_json::to_vec(credentials)
			.map_err(|error| ProfileError::Io(io::Error::other(error)))?;
		let cipher = dpapi::protect(&plain)?;
		self.write(&self.credentials_path(), &cipher)
	}

	pub fn clear_credentials(&self) -> Result<(), ProfileError> {
		match fs::remove_file(self.credentials_path()) {
			Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
			_ => Ok(()),
		}
	}

	pub fn has_credentials(&self) -> bool {
		self.credentials_path().is_file()
	}

	fn credentials_path(&self) -> PathBuf {
		self.dir.join(CREDENTIALS_FILE)
	}

	/// Writes through a temporary file, so a crash never leaves a half-written file behind.
	fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
		fs::create_dir_all(&self.dir)?;
		let mut temporary = OsString::from(path);
		temporary.push(".tmp");
		fs::write(&temporary, bytes)?;
		fs::rename(&temporary, path)?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::{Profile, ProfileError};
	use crate::Credentials;

	fn alice() -> Credentials {
		Credentials::with_password("alice", "secret")
	}

	#[test]
	fn saved_credentials_load_back_equal() {
		let dir = tempfile::tempdir().unwrap();
		let profile = Profile::new(dir.path());
		profile.save_credentials(&alice()).unwrap();
		assert_eq!(profile.load_credentials().unwrap(), Some(alice()));
		assert!(profile.has_credentials());
	}

	#[test]
	fn credentials_file_is_not_plaintext() {
		let dir = tempfile::tempdir().unwrap();
		Profile::new(dir.path()).save_credentials(&alice()).unwrap();
		let raw = std::fs::read(dir.path().join("credentials.bin")).unwrap();
		for word in [&b"alice"[..], b"secret"] {
			assert!(!raw.windows(word.len()).any(|window| window == word));
		}
	}

	#[test]
	fn missing_credentials_load_as_none() {
		let dir = tempfile::tempdir().unwrap();
		let profile = Profile::new(dir.path());
		assert_eq!(profile.load_credentials().unwrap(), None);
		assert!(!profile.has_credentials());
	}

	#[test]
	fn clear_removes_credentials() {
		let dir = tempfile::tempdir().unwrap();
		let profile = Profile::new(dir.path());
		profile.save_credentials(&alice()).unwrap();
		profile.clear_credentials().unwrap();
		assert!(!profile.has_credentials());
		assert_eq!(profile.load_credentials().unwrap(), None);
	}

	#[test]
	fn clear_without_credentials_is_ok() {
		let dir = tempfile::tempdir().unwrap();
		Profile::new(dir.path()).clear_credentials().unwrap();
	}

	#[test]
	fn corrupt_credentials_file_is_unreadable_error() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("credentials.bin"), b"garbage").unwrap();
		let result = Profile::new(dir.path()).load_credentials();
		assert!(matches!(result, Err(ProfileError::Unreadable(_))), "{result:?}");
	}

	#[test]
	fn save_creates_missing_directory() {
		let dir = tempfile::tempdir().unwrap();
		let profile = Profile::new(dir.path().join("a").join("b"));
		profile.save_credentials(&alice()).unwrap();
		assert_eq!(profile.load_credentials().unwrap(), Some(alice()));
	}

	#[test]
	fn device_id_is_created_once_and_reused() {
		let dir = tempfile::tempdir().unwrap();
		let first = Profile::new(dir.path()).device_id().unwrap();
		let second = Profile::new(dir.path()).device_id().unwrap();
		assert_eq!(first, second);
		assert_eq!(first.len(), 36);
		assert!(dir.path().join("device_id").exists());
	}
}
