//! The C++ translation unit that measures the SDK, and the compiler that
//! builds it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Entry, Spelling};

/// The SDK headers `after-effects-sys` generates its bindings from (its
/// `wrapper.hpp`), in the same order.
///
/// `wrapper.hpp` also includes `Mach-O_prefix.h`, which only defines
/// `__MACH__`. A Windows plugin never includes it, so it is left out here: the
/// snapshot is the layout plugins are compiled with.
const HEADERS: &[&str] = &[
	"AEConfig.h",
	"A.h",
	"AE_AdvEffectSuites.h",
	"AE_CacheOnLoadSuite.h",
	"AE_ChannelSuites.h",
	"AE_CreatorInfo.h",
	"AE_Effect.h",
	"AE_EffectCB.h",
	"AE_EffectCBSuites.h",
	"AE_EffectGPUSuites.h",
	"AE_EffectPixelFormat.h",
	"AE_EffectSuites.h",
	"AE_EffectSuitesHelper.h",
	"AE_EffectUI.h",
	"AE_EffectVers.h",
	"AE_GeneralPlug.h",
	"AE_Hook.h",
	"AE_IO.h",
	"AE_IO_FileExt.h",
	"AE_Macros.h",
	"AE_PluginData.h",
	"AE_HashSuite.h",
	"AE_ComputeCacheSuite.h",
	"FIEL_Public.h",
	"PF_Masks.h",
	"PR_Public.h",
	"PT_Public.h",
	"PrSDKAESupport.h",
	"PrSDKPixelFormat.h",
	"SuiteHelper.h",
	"SPBasic.h",
	"entry.h",
	"adobesdk/DrawbotSuite.h",
];

/// The version macros the snapshot records.
pub const VERSION_MACROS: &[&str] = &[
	"PF_PLUG_IN_VERSION",
	"PF_PLUG_IN_SUBVERS",
	"PF_AE_PLUG_IN_VERSION",
	"PF_AE_PLUG_IN_SUBVERS",
];

/// The C++ compiler `cc` picks for `target`, set up to see the SDK.
pub struct Compiler {
	tool: cc::Tool,
	sdk: PathBuf,
	windows: bool,
}

impl Compiler {
	pub fn new(target: &str, sdk: &Path) -> Compiler {
		let tool = cc::Build::new()
			.cpp(true)
			.target(target)
			.host(target)
			.opt_level(0)
			.debug(false)
			.cargo_metadata(false)
			.get_compiler();
		Compiler {
			tool,
			sdk: sdk.to_path_buf(),
			windows: target.contains("windows"),
		}
	}

	fn command(&self) -> Command {
		let mut command = self.tool.to_command();
		let headers = self.sdk.join("Headers");
		let includes = [headers.clone(), headers.join("SP"), self.sdk.join("Util")];
		if self.tool.is_like_msvc() {
			command.args(["/nologo", "/std:c++20", "/EHsc", "/W0"]);
			for dir in includes {
				command.arg(format!("/I{}", dir.display()));
			}
		} else {
			command.args(["-std=c++20", "-w"]);
			for dir in includes {
				command.arg(format!("-I{}", dir.display()));
			}
		}
		if self.windows {
			// As `after-effects-sys` passes to bindgen. `NOMINMAX` keeps
			// <windows.h> from turning field names into macros.
			command
				.args(define(&self.tool, "_WINDOWS"))
				.args(define(&self.tool, "NOMINMAX"));
		}
		command
	}

	/// The headers, preprocessed: every identifier the SDK declares is in it.
	pub fn preprocess(&self, dir: &Path) -> Result<String, String> {
		let source = dir.join("sdk_headers.cpp");
		std::fs::write(&source, includes()).map_err(|e| format!("writing {}: {e}", source.display()))?;
		let mut command = self.command();
		if self.tool.is_like_msvc() {
			command.arg("/EP");
		} else {
			command.args(["-E", "-P"]);
		}
		command.arg(&source);
		run(command, "preprocessing the SDK headers")
	}

	/// Compile `source` into a program, run it and return what it printed.
	pub fn build_and_run(&self, dir: &Path, source: &str) -> Result<String, String> {
		let path = dir.join("sdk_layout.cpp");
		std::fs::write(&path, source).map_err(|e| format!("writing {}: {e}", path.display()))?;
		let exe = dir.join(if self.windows { "sdk_layout.exe" } else { "sdk_layout" });
		let mut command = self.command();
		if self.tool.is_like_msvc() {
			command
				.arg(format!("/Fe{}", exe.display()))
				.arg(format!("/Fo{}\\", dir.display()));
		} else {
			command.arg("-o").arg(&exe);
		}
		command.arg(&path);
		run(command, "compiling the layout program")?;
		run(Command::new(&exe), "running the layout program")
	}
}

fn define(tool: &cc::Tool, name: &str) -> [String; 1] {
	[if tool.is_like_msvc() {
		format!("/D{name}")
	} else {
		format!("-D{name}")
	}]
}

fn run(mut command: Command, what: &str) -> Result<String, String> {
	let output = command.output().map_err(|e| format!("{what}: {e}"))?;
	if !output.status.success() {
		return Err(format!(
			"{what} failed ({}):\n{}\n{}",
			output.status,
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr)
		));
	}
	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn includes() -> String {
	let mut out = String::from("#include <stdint.h>\n");
	for header in HEADERS {
		writeln!(out, "#include \"{header}\"").unwrap();
	}
	out
}

/// The program: one function template per entry, which prints the layout of
/// its type and of each field the Rust struct names.
///
/// A field is probed with a `requires` expression, so a name the SDK lacks
/// prints `M` instead of failing to compile. A name that exists but cannot
/// have its address taken is a bitfield (`B`). Output lines:
///
/// - `C <compiler>` and `V <macro> <value>`;
/// - `S <entry> <size> <align> <union>` for each type measured;
/// - `I <entry>` for a type the SDK declares but does not define;
/// - `F <entry> <field> <offset> <size> <align>`, `M <entry> <field>`,
///   `B <entry> <field>`;
/// - `R <entry> <field>` after the `F` of a field found under its older name.
///
/// An entry for an anonymous type is measured by the entry holding it,
/// through `decltype` of the member, so it is never named in C++.
pub fn program(entries: &[Entry], present: impl Fn(usize) -> bool) -> String {
	let mut out = String::from("// Generated by aexlo_sdk_layout's `snapshot` binary.\n");
	out.push_str(
		r#"#include <cstddef>
#include <cstdio>
#include <type_traits>
"#,
	);
	out.push_str(&includes());
	out.push_str(
		r#"
#if defined(__clang__)
#define LAYOUT_COMPILER "clang " __clang_version__
#elif defined(_MSC_VER)
#define LAYOUT_STR2(x) #x
#define LAYOUT_STR(x) LAYOUT_STR2(x)
#define LAYOUT_COMPILER "MSVC " LAYOUT_STR(_MSC_FULL_VER)
#else
#define LAYOUT_COMPILER "gcc " __VERSION__
#endif

static void layout(int i, std::size_t size, std::size_t align, bool is_union) {
	std::printf("S %d %zu %zu %d\n", i, size, align, is_union ? 1 : 0);
}
static void incomplete(int i) { std::printf("I %d\n", i); }
static void field(int i, int j, std::size_t offset, std::size_t size, std::size_t align) {
	std::printf("F %d %d %zu %zu %zu\n", i, j, offset, size, align);
}
static void missing(int i, int j) { std::printf("M %d %d\n", i, j); }
static void bitfield(int i, int j) { std::printf("B %d %d\n", i, j); }
static void renamed(int i, int j) { std::printf("R %d %d\n", i, j); }

"#,
	);
	for i in 0..entries.len() {
		writeln!(out, "template <class T> void record_{i}();").unwrap();
	}
	out.push('\n');
	for (i, entry) in entries.iter().enumerate() {
		writeln!(out, "// {}", entry.rust).unwrap();
		writeln!(out, "template <class T> void record_{i}() {{").unwrap();
		writeln!(out, "\tif constexpr (!requires {{ sizeof(T); }}) {{").unwrap();
		writeln!(out, "\t\tincomplete({i});").unwrap();
		writeln!(out, "\t}} else {{").unwrap();
		writeln!(out, "\t\tlayout({i}, sizeof(T), alignof(T), std::is_union_v<T>);").unwrap();
		for (j, f) in entry.fields.iter().enumerate() {
			let name = &f.cpp;
			writeln!(out, "\t\tif constexpr (requires(T &t) {{ &t.{name}; }}) {{").unwrap();
			writeln!(
				out,
				"\t\t\tfield({i}, {j}, offsetof(T, {name}), sizeof(decltype(T::{name})), alignof(decltype(T::{name})));"
			)
			.unwrap();
			if let Some(child) = f.child {
				writeln!(
					out,
					"\t\t\trecord_{child}<std::remove_all_extents_t<decltype(T::{name})>>();"
				)
				.unwrap();
			}
			if let Some(old) = f.renamed {
				writeln!(out, "\t\t}} else if constexpr (requires(T &t) {{ &t.{old}; }}) {{").unwrap();
				writeln!(
					out,
					"\t\t\tfield({i}, {j}, offsetof(T, {old}), sizeof(decltype(T::{old})), alignof(decltype(T::{old})));"
				)
				.unwrap();
				writeln!(out, "\t\t\trenamed({i}, {j});").unwrap();
			}
			writeln!(out, "\t\t}} else if constexpr (requires(T &t) {{ t.{name}; }}) {{").unwrap();
			writeln!(out, "\t\t\tbitfield({i}, {j});").unwrap();
			writeln!(out, "\t\t}} else {{").unwrap();
			writeln!(out, "\t\t\tmissing({i}, {j});").unwrap();
			writeln!(out, "\t\t}}").unwrap();
		}
		writeln!(out, "\t}}").unwrap();
		writeln!(out, "}}\n").unwrap();
	}
	out.push_str("int main() {\n\tstd::printf(\"C %s\\n\", LAYOUT_COMPILER);\n");
	for name in VERSION_MACROS {
		writeln!(out, "\tstd::printf(\"V {name} %d\\n\", (int)({name}));").unwrap();
	}
	for (i, entry) in entries.iter().enumerate() {
		if let Spelling::Plain(name) = &entry.spelling
			&& present(i)
		{
			writeln!(out, "\trecord_{i}<{name}>();").unwrap();
		}
	}
	out.push_str("\treturn 0;\n}\n");
	out
}
