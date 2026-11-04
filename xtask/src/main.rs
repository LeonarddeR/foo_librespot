//! `cargo xtask package` builds the release DLL and packages it as a `.fb2k-component` with its
//! licence notices, in `target/package`. `cargo xtask release-notes <version>` prints that
//! version's section of `CHANGELOG.md`, for the GitHub release.

mod component;
mod notes;
mod notices;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const USAGE: &str =
	"usage: cargo xtask package [--expect-built] | cargo xtask release-notes <version>";

fn main() -> ExitCode {
	let args: Vec<String> = env::args().skip(1).collect();
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	let result = match args.as_slice() {
		["package"] => package(false),
		["package", "--expect-built"] => package(true),
		["release-notes", version] => print_release_notes(version),
		_ => Err(USAGE.into()),
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("xtask: {error}");
			ExitCode::FAILURE
		}
	}
}

/// The repository: the folder this crate sits in.
fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask is in the repository").to_owned()
}

fn cargo(root: &Path) -> Command {
	let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
	command.current_dir(root);
	command
}

/// The text of `path`; the error names the file.
fn read_text(path: &Path) -> Result<String> {
	fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()).into())
}

/// Copies `from` to `to`; the error names the file that couldn't be read.
fn copy(from: &Path, to: &Path) -> Result<()> {
	fs::copy(from, to).map(drop).map_err(|error| format!("{}: {error}", from.display()).into())
}

fn run(command: &mut Command, what: &str) -> Result<()> {
	if command.status()?.success() { Ok(()) } else { Err(format!("{what} failed").into()) }
}

/// The DLL `cargo build` made, as its JSON messages report it.
#[derive(Debug)]
struct Built {
	dll: PathBuf,
	pdb: PathBuf,
	version: String,
	/// Cargo found the DLL up to date and didn't build it again.
	fresh: bool,
}

/// foo_librespot's DLL in the messages of `cargo build --message-format=json`. The paths are
/// where cargo wrote the files, whatever the target folder and target.
fn built_dll(messages: &str) -> Result<Built> {
	let artifact = messages
		.lines()
		.filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
		.find(|message| {
			message["reason"] == "compiler-artifact" && message["target"]["name"] == "foo_librespot"
		})
		.ok_or("cargo build reported no foo_librespot artifact")?;
	let file = |extension: &str| {
		artifact["filenames"]
			.as_array()
			.into_iter()
			.flatten()
			.filter_map(serde_json::Value::as_str)
			.find(|name| name.ends_with(extension))
			.map(PathBuf::from)
			.ok_or_else(|| format!("cargo build made no foo_librespot {extension}"))
	};
	// "path+file:///…/foo_librespot#0.1.0", or "…#foo_librespot@0.1.0" when the folder's name
	// differs from the crate's.
	let id = artifact["package_id"].as_str().unwrap_or_default();
	let version = id.rsplit_once('#').map(|(_, rest)| rest.rsplit('@').next().unwrap_or(rest));
	Ok(Built {
		dll: file(".dll")?,
		pdb: file(".pdb")?,
		version: version.filter(|version| !version.is_empty()).ok_or("no version")?.to_owned(),
		fresh: artifact["fresh"].as_bool().unwrap_or(false),
	})
}

/// Builds the release DLL. With `expect_built`, as in CI, the DLL must already be built: a
/// rebuild here would mean the DLL CI uploaded differs from the one in the package.
fn build_dll(root: &Path, expect_built: bool) -> Result<Built> {
	let output = cargo(root)
		.args(["build", "--release", "--locked", "-p", "foo_librespot"])
		.arg("--message-format=json-render-diagnostics")
		.stderr(std::process::Stdio::inherit())
		.output()?;
	if !output.status.success() {
		return Err("cargo build failed".into());
	}
	let built = built_dll(&String::from_utf8_lossy(&output.stdout))?;
	if expect_built && !built.fresh {
		return Err("cargo build had to build foo_librespot.dll again, so it differs from the DLL built before: make that build select the same crates and features".into());
	}
	Ok(built)
}

fn package(expect_built: bool) -> Result<()> {
	let root = root();
	require_cargo_about(&root)?;
	let built = build_dll(&root, expect_built)?;
	// target\release, or target\<target>\release; the package goes next to it.
	let out_dir = built
		.dll
		.parent()
		.and_then(Path::parent)
		.ok_or("the DLL has no target folder")?
		.join("package");
	fs::create_dir_all(&out_dir)?;
	let licenses = out_dir.join("licenses.html");
	write_licenses(&root, &licenses)?;
	let component = out_dir.join(component::file_name(&built.version));
	component::write(&component, &component::entries(&root, &built.dll, &licenses))?;
	copy(&built.pdb, &out_dir.join("foo_librespot.pdb"))?;
	println!("{}", component.display());
	Ok(())
}

/// The third-party notices for the DLL's dependencies, from `about.toml` and `about.hbs`.
/// Fails unless cargo-about is installed. Checked before the release build, which takes minutes.
fn require_cargo_about(root: &Path) -> Result<()> {
	let installed = cargo(root)
		.args(["about", "--version"])
		.output()
		.is_ok_and(|output| output.status.success());
	if installed {
		Ok(())
	} else {
		Err("cargo-about is not installed: cargo install --locked cargo-about --features cli"
			.into())
	}
}

fn write_licenses(root: &Path, out: &Path) -> Result<()> {
	run(&mut about_command(root, out), "cargo about generate")?;
	if let Some(missing) = notices::missing_copyright(&read_text(out)?) {
		return Err(missing.into());
	}
	Ok(())
}

/// `cargo about generate` for the DLL's dependencies, into `out`.
fn about_command(root: &Path, out: &Path) -> Command {
	let mut command = cargo(root);
	command
		.args(["about", "generate", "--manifest-path"])
		.arg(root.join("crates").join("foo_librespot").join("Cargo.toml"))
		.args(["--config", "about.toml", "--locked", "--output-file"])
		.arg(out)
		// A crate whose licence can't be found would otherwise be left out with a warning.
		.arg("--fail")
		.arg("about.hbs");
	command
}

fn print_release_notes(version: &str) -> Result<()> {
	let changelog = read_text(&root().join("CHANGELOG.md"))?;
	let notes = notes::release_notes(&changelog, version)?;
	println!("{notes}");
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::path::{Path, PathBuf};

	use super::{about_command, built_dll};

	/// What `cargo build --message-format=json` prints for foo_librespot on Windows, trimmed.
	const ARTIFACT: &str = r#"{"reason":"compiler-artifact","package_id":"path+file:///P:/r/crates/foo_librespot#0.2.0","target":{"kind":["cdylib","rlib"],"name":"foo_librespot"},"filenames":["P:\\r\\target\\release\\foo_librespot.dll","P:\\r\\target\\release\\foo_librespot.dll.lib","P:\\r\\target\\release\\foo_librespot.pdb","P:\\r\\target\\release\\libfoo_librespot.rlib"],"fresh":true}"#;

	#[test]
	fn the_dll_pdb_version_and_freshness_come_from_cargos_messages() {
		let other = r#"{"reason":"compiler-artifact","package_id":"registry+x#tokio@1.0.0","target":{"name":"tokio"},"filenames":[],"fresh":true}"#;
		let finished = r#"{"reason":"build-finished","success":true}"#;
		let built = built_dll(&format!("{other}\n{ARTIFACT}\n{finished}\n")).unwrap();
		assert_eq!(built.dll, PathBuf::from(r"P:\r\target\release\foo_librespot.dll"));
		assert_eq!(built.pdb, PathBuf::from(r"P:\r\target\release\foo_librespot.pdb"));
		assert_eq!(built.version, "0.2.0");
		assert!(built.fresh);
	}

	#[test]
	fn a_package_id_that_names_the_crate_still_gives_the_version() {
		let messages = ARTIFACT.replace("foo_librespot#0.2.0", "foo#foo_librespot@0.3.1");
		assert_eq!(built_dll(&messages).unwrap().version, "0.3.1");
	}

	#[test]
	fn a_file_error_names_the_file() {
		let missing = Path::new("no such folder").join("CHANGELOG.md");
		let error = super::read_text(&missing).unwrap_err();
		assert!(error.to_string().starts_with(&missing.display().to_string()), "{error}");
		let error = super::copy(&missing, Path::new("out.pdb")).unwrap_err();
		assert!(error.to_string().starts_with(&missing.display().to_string()), "{error}");
	}

	#[test]
	fn no_foo_librespot_artifact_is_an_error() {
		assert!(built_dll(r#"{"reason":"build-finished","success":true}"#).is_err());
	}

	#[test]
	fn cargo_about_fails_on_unlisted_licences_and_uses_the_lockfile() {
		let command = about_command(Path::new("repo"), Path::new("out/licenses.html"));
		let args: Vec<_> =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
		assert_eq!(&args[..2], ["about", "generate"]);
		assert!(args.iter().any(|arg| arg == "--fail"), "{args:?}");
		assert!(args.iter().any(|arg| arg == "--locked"), "{args:?}");
		assert_eq!(args.last().map(String::as_str), Some("about.hbs"));
	}
}
