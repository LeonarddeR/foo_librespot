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

const USAGE: &str = "usage: cargo xtask package | cargo xtask release-notes <version>";

fn main() -> ExitCode {
	let args: Vec<String> = env::args().skip(1).collect();
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	let result = match args.as_slice() {
		["package"] => package(),
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

fn run(command: &mut Command, what: &str) -> Result<()> {
	if command.status()?.success() { Ok(()) } else { Err(format!("{what} failed").into()) }
}

/// foo_librespot's version and the target folder, as `cargo metadata` resolves them.
fn metadata(root: &Path) -> Result<(String, PathBuf)> {
	let output = cargo(root).args(["metadata", "--format-version", "1", "--no-deps"]).output()?;
	if !output.status.success() {
		return Err("cargo metadata failed".into());
	}
	let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
	let version = metadata["packages"]
		.as_array()
		.and_then(|packages| packages.iter().find(|package| package["name"] == "foo_librespot"))
		.and_then(|package| package["version"].as_str())
		.ok_or("cargo metadata has no foo_librespot")?;
	let target =
		metadata["target_directory"].as_str().ok_or("cargo metadata has no target folder")?;
	Ok((version.to_owned(), PathBuf::from(target)))
}

fn package() -> Result<()> {
	let root = root();
	run(
		cargo(&root).args(["build", "--release", "--locked", "-p", "foo_librespot"]),
		"cargo build",
	)?;
	let (version, target) = metadata(&root)?;
	let release_dir = target.join("release");
	let out_dir = target.join("package");
	fs::create_dir_all(&out_dir)?;
	let licenses = out_dir.join("licenses.html");
	write_licenses(&root, &licenses)?;
	let component = out_dir.join(component::file_name(&version));
	component::write(&component, &component::entries(&root, &release_dir, &licenses))?;
	fs::copy(release_dir.join("foo_librespot.pdb"), out_dir.join("foo_librespot.pdb"))?;
	println!("{}", component.display());
	Ok(())
}

/// The third-party notices for the DLL's dependencies, from `about.toml` and `about.hbs`.
fn write_licenses(root: &Path, out: &Path) -> Result<()> {
	let installed = cargo(root)
		.args(["about", "--version"])
		.output()
		.is_ok_and(|output| output.status.success());
	if !installed {
		return Err(
			"cargo-about is not installed: cargo install --locked cargo-about --features cli"
				.into(),
		);
	}
	run(&mut about_command(root, out), "cargo about generate")?;
	if let Some(placeholder) = notices::placeholder(&fs::read_to_string(out)?) {
		return Err(format!(
			"licenses.html has the generic licence text's {placeholder} instead of a copyright line: give the crate a clarify entry in about.toml"
		)
		.into());
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
	let changelog = fs::read_to_string(root().join("CHANGELOG.md"))?;
	let notes = notes::release_notes(&changelog, version)
		.ok_or_else(|| format!("CHANGELOG.md has no section for version {version}"))?;
	println!("{notes}");
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use super::about_command;

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
