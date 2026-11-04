//! Encryption for the stored login, tied to the current Windows user (DPAPI).

use std::io;
use std::ptr;

use crate::bindings::Windows::Win32::{
	CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData, HANDLE,
	LocalFree,
};

/// Mixed into the encryption, so other programs running as the same user can't decrypt the
/// login by simply calling `CryptUnprotectData` on the file.
const ENTROPY: &[u8] = b"foo_librespot credentials v1";

/// Encrypts `plain` for the current Windows user.
pub fn protect(plain: &[u8]) -> io::Result<Vec<u8>> {
	let input = blob(plain)?;
	let entropy = blob(ENTROPY)?;
	let mut output = CRYPT_INTEGER_BLOB::default();
	// SAFETY: every blob points at memory that outlives the call; DPAPI allocates `output`.
	let ok = unsafe {
		CryptProtectData(
			&raw const input,
			windows_core::PCWSTR::null(),
			Some(&raw const entropy),
			None,
			None,
			CRYPTPROTECT_UI_FORBIDDEN.cast_unsigned(),
			&raw mut output,
		)
	};
	take_output(ok.as_bool(), output)
}

/// Decrypts what [`protect`] returned, for the same Windows user.
pub fn unprotect(cipher: &[u8]) -> io::Result<Vec<u8>> {
	let input = blob(cipher)?;
	let entropy = blob(ENTROPY)?;
	let mut output = CRYPT_INTEGER_BLOB::default();
	// SAFETY: as in `protect`.
	let ok = unsafe {
		CryptUnprotectData(
			&raw const input,
			ptr::null_mut(),
			Some(&raw const entropy),
			None,
			None,
			CRYPTPROTECT_UI_FORBIDDEN.cast_unsigned(),
			&raw mut output,
		)
	};
	take_output(ok.as_bool(), output)
}

fn blob(bytes: &[u8]) -> io::Result<CRYPT_INTEGER_BLOB> {
	let len = u32::try_from(bytes.len())
		.map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "data too large for DPAPI"))?;
	// DPAPI never writes through the input pointer.
	Ok(CRYPT_INTEGER_BLOB { cbData: len, pbData: bytes.as_ptr().cast_mut() })
}

fn take_output(ok: bool, output: CRYPT_INTEGER_BLOB) -> io::Result<Vec<u8>> {
	if !ok {
		return Err(io::Error::last_os_error());
	}
	// SAFETY: on success DPAPI returns `cbData` bytes at `pbData`, allocated with LocalAlloc.
	let bytes =
		unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
	// SAFETY: the buffer came from DPAPI and is freed exactly once.
	unsafe { LocalFree(HANDLE(output.pbData.cast())) };
	Ok(bytes)
}

#[cfg(test)]
mod tests {
	use super::{protect, unprotect};

	#[test]
	fn protect_then_unprotect_round_trips() {
		let cipher = protect(b"alice-secret-token").unwrap();
		assert_eq!(unprotect(&cipher).unwrap(), b"alice-secret-token");
	}

	#[test]
	fn protected_bytes_do_not_contain_the_plaintext() {
		let plain = b"alice-secret-token";
		let cipher = protect(plain).unwrap();
		assert!(!cipher.windows(plain.len()).any(|window| window == plain));
	}

	#[test]
	fn unprotect_rejects_garbage() {
		assert!(unprotect(b"not dpapi").is_err());
	}
}
