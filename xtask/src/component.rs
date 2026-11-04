//! The `.fb2k-component` package: a zip that foobar2000 installs from Preferences > Components.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::Result;

/// The package's file name for `version`.
pub fn file_name(version: &str) -> String {
	format!("foo_librespot-{version}.fb2k-component")
}

/// What goes in the package, as (source file, name in the archive). A 64-bit foobar2000 extracts
/// the whole archive, then puts the files from `x64/` over the root ones, so the DLL goes under
/// `x64/` and the notices at the root, where they end up next to it.
pub fn entries(root: &Path, dll: &Path, licenses: &Path) -> Vec<(PathBuf, &'static str)> {
	vec![
		(dll.to_path_buf(), "x64/foo_librespot.dll"),
		(root.join("LICENSE"), "LICENSE.txt"),
		(licenses.to_path_buf(), "licenses.html"),
	]
}

/// Writes `entries` to a new archive at `out`, deflated, in the order given. The archive is
/// written beside `out` and only takes its name once it is complete, so a failure leaves
/// whatever was at `out` alone instead of a valid but incomplete package under the release name.
pub fn write(out: &Path, entries: &[(PathBuf, &'static str)]) -> Result<()> {
	let partial = out.with_extension("partial");
	let written = write_archive(&partial, entries);
	if written.is_err() {
		let _ = fs::remove_file(&partial);
	}
	written?;
	fs::rename(&partial, out).map_err(|error| format!("{}: {error}", out.display()))?;
	Ok(())
}

fn write_archive(out: &Path, entries: &[(PathBuf, &'static str)]) -> Result<()> {
	let file = File::create(out).map_err(|error| format!("{}: {error}", out.display()))?;
	let mut archive = ZipWriter::new(file);
	let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
	for (source, name) in entries {
		archive.start_file(*name, options)?;
		let mut source_file =
			File::open(source).map_err(|error| format!("{}: {error}", source.display()))?;
		io::copy(&mut source_file, &mut archive)?;
	}
	archive.finish()?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::fs;
	use std::io::Read;
	use std::path::Path;

	use super::{entries, file_name, write};

	#[test]
	fn file_name_carries_the_version() {
		assert_eq!(file_name("0.1.0"), "foo_librespot-0.1.0.fb2k-component");
	}

	#[test]
	fn entries_put_the_dll_under_x64_and_the_notices_at_the_root() {
		let root = Path::new("repo");
		let dll = Path::new("repo/target/release/foo_librespot.dll");
		let licenses = Path::new("repo/target/package/licenses.html");
		let entries = entries(root, dll, licenses);
		let names: Vec<_> = entries.iter().map(|(_, name)| *name).collect();
		assert_eq!(names, ["x64/foo_librespot.dll", "LICENSE.txt", "licenses.html"]);
		let sources: Vec<_> = entries.iter().map(|(source, _)| source.clone()).collect();
		assert_eq!(sources, [dll.to_path_buf(), root.join("LICENSE"), licenses.to_path_buf()]);
	}

	#[test]
	fn write_stores_each_entry_under_its_name() {
		let dir = tempfile::tempdir().unwrap();
		let dll = dir.path().join("foo_librespot.dll");
		let licence = dir.path().join("LICENSE");
		fs::write(&dll, b"MZ not really a DLL").unwrap();
		fs::write(&licence, b"MIT License").unwrap();
		let out = dir.path().join("out.fb2k-component");
		write(&out, &[(dll, "x64/foo_librespot.dll"), (licence, "LICENSE.txt")]).unwrap();

		let mut archive = zip::ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
		let mut found = Vec::new();
		for index in 0..archive.len() {
			let mut entry = archive.by_index(index).unwrap();
			let mut bytes = Vec::new();
			entry.read_to_end(&mut bytes).unwrap();
			found.push((entry.name().unwrap().into_owned(), bytes));
		}
		assert_eq!(
			found,
			[
				("x64/foo_librespot.dll".to_owned(), b"MZ not really a DLL".to_vec()),
				("LICENSE.txt".to_owned(), b"MIT License".to_vec()),
			]
		);
		assert!(found.iter().all(|(name, _)| !name.contains('\\')));
	}

	#[test]
	fn a_failed_write_leaves_the_previous_package_alone() {
		let dir = tempfile::tempdir().unwrap();
		let out = dir.path().join("foo_librespot-0.1.0.fb2k-component");
		fs::write(&out, b"the good package").unwrap();
		let missing = dir.path().join("foo_librespot.dll");
		let error = write(&out, &[(missing.clone(), "x64/foo_librespot.dll")]).unwrap_err();
		assert_eq!(fs::read(&out).unwrap(), b"the good package");
		// No half-written archive is left next to it either.
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
		// The error says which file it couldn't read.
		assert!(error.to_string().contains(&missing.display().to_string()), "{error}");
	}
}
