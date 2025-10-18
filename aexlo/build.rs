fn main() {
	let dst = cmake::build("unsafe");

	println!("cargo:rustc-link-search=native={}", dst.display());
	println!("cargo:rustc-link-lib=static=unsafe");
}
