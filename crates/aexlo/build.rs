fn main() {
	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
		cc::Build::new().file("src/seh_call.cpp").cpp(true).compile("aexlo_seh");
	}
}
