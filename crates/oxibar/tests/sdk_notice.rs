//! Every file transcribed from the foobar2000 SDK must carry the SDK's whole notice: its licence
//! lets source copies be redistributed only with the copyright notice, the conditions and the
//! disclaimer kept.

use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = "transcribed from the foobar2000 2.x SDK";

fn words(text: &str) -> String {
	text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The notice from `sdk-license.txt`, up to the end of its disclaimer.
fn notice() -> String {
	let licence = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("sdk-license.txt"))
		.expect("sdk-license.txt");
	let end = licence.find("SUCH DAMAGE.").expect("the disclaimer") + "SUCH DAMAGE.".len();
	words(&licence[..end])
}

/// The comment lines at the top of `source`, without their `//`.
fn header(source: &str) -> String {
	let lines: Vec<&str> = source
		.lines()
		.take_while(|line| line.starts_with("//") && !line.starts_with("//!"))
		.map(|line| line.trim_start_matches('/'))
		.collect();
	words(&lines.join("\n"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
	for entry in fs::read_dir(dir).expect("readable source folder") {
		let path = entry.expect("a folder entry").path();
		if path.is_dir() {
			rust_files(&path, out);
		} else if path.extension().is_some_and(|extension| extension == "rs") {
			out.push(path);
		}
	}
}

#[test]
fn every_transcribed_file_carries_the_whole_sdk_notice() {
	let notice = notice();
	let mut files = Vec::new();
	rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
	let transcribed: Vec<PathBuf> = files
		.into_iter()
		.filter(|path| fs::read_to_string(path).is_ok_and(|source| source.contains(MARKER)))
		.collect();
	assert!(transcribed.len() > 10, "found only {transcribed:?}");
	let incomplete: Vec<&PathBuf> = transcribed
		.iter()
		.filter(|path| !header(&fs::read_to_string(path).unwrap()).contains(&notice))
		.collect();
	assert!(incomplete.is_empty(), "missing the whole SDK notice: {incomplete:?}");
}
