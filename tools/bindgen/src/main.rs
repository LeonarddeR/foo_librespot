fn main() {
	let time = std::time::Instant::now();

	let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
	std::env::set_current_dir(repo_root).expect("repository root");

	for etc in ["tools/bindgen/oxibar.txt", "tools/bindgen/core.txt"] {
		windows_bindgen::bindgen(["--etc", etc]);
	}

	println!("Finished in {:.2}s", time.elapsed().as_secs_f32());
}
