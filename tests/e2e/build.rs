use std::env::consts::DLL_PREFIX;

fn main() {
	//== Rerun Triggers ==//
	println!("cargo:rerun-if-changed=client/CMakeLists.txt");
	println!("cargo:rerun-if-changed=client/about_message_test/CMakeLists.txt");
	println!("cargo:rerun-if-changed=client/about_message_test/about_message.cc");
	println!("cargo:rerun-if-changed=client/pointer_validation_test/CMakeLists.txt");
	println!("cargo:rerun-if-changed=client/pointer_validation_test/main.cc");

	let dst = cmake::build("client");
	println!("cargo:rustc-link-search=native={}/lib", dst.display());

	//== Resolve Extension ==//
	let dll_ext = match std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() {
		Some("windows") => "dll",
		Some("macos") => "dylib",
		_ => "so",
	};

	//== Environment Variables ==//
	println!(
		"cargo:rustc-env=E2E_ABOUT_MESSAGE_TEST_CLIENT={}",
		dst.join(format!("{DLL_PREFIX}about_message_test_client.{dll_ext}"))
			.display()
	);

	println!(
		"cargo:rustc-env=E2E_POINTER_VALIDATION_TEST_CLIENT={}",
		dst.join(format!("{DLL_PREFIX}pointer_validation_test_client.{dll_ext}"))
			.display()
	);

	//== Linker Settings ==//
	let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
	if target_env == "gnu" {
		println!("cargo:rustc-link-lib=dylib=stdc++");
	}
}
