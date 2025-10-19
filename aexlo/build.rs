fn main() {
	let dst = cmake::Config::new("unsafe")
		.define("CMAKE_BUILD_TYPE", "Release")
		.define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
		.build();

	println!("cargo:rustc-link-search=native={}", dst.display());
	println!("cargo:rustc-link-lib=static=unsafe");
}
