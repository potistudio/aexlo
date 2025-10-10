use std::env::var;
use std::path::PathBuf;

fn main() {
	let manifest = var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR env var is not set."); // path to Cargo.toml
	let target = var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS env var is not set."); // windows, macos, etc.

	let mock_dir = match target.as_str() {
		"windows" => PathBuf::from(&manifest).join("tests/mocks/windows"),
		"macos" => PathBuf::from(&manifest).join("tests/mocks/macos"),
		_ => return,
	};

	let out_dir = var("OUT_DIR").expect("OUT_DIR env var is not set."); // path to target/debug/build/<>/out
	let dest = PathBuf::from(&out_dir).join("../../../"); // target/debug or target/release

	for entry in std::fs::read_dir(mock_dir).expect("mock_dir does not exist or is not a directory.") {
		let entry = entry.unwrap();
		let src = entry.path();

		if src.is_file() {
			std::fs::copy(&src, dest.join(src.file_name().unwrap())).unwrap();
		} else if src.is_dir() {
			copy_dir_all(&src, &dest.join(src.file_name().unwrap())).unwrap();
		} else {
			panic!("Unknown file type: {:?}", src);
		}
	}
}

fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
	std::fs::create_dir_all(dst)?;

	for entry in std::fs::read_dir(src)? {
		let entry = entry?;
		let ty = entry.file_type()?;

		if ty.is_dir() {
			copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
		} else {
			std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
		}
	}

	Ok(())
}
