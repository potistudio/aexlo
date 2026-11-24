//! Measure the After Effects SDK headers and write the layout snapshot that
//! the `aexlo_sdk_layout` test compares `after-effects-sys` against.
//!
//! ```text
//! cargo run -p aexlo_sdk_layout --bin snapshot
//! ```
//!
//! It needs the SDK (`AE_SDK_DIR`, by default `refs/AE SDK` in the workspace)
//! and the C++ compiler the `cc` crate finds. It writes
//! `fixtures/sdk-layout/<os>-<arch>.json` for the host. See
//! `docs/sdk-layout.md`.

mod bindings;
mod cpp;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use aexlo_sdk_layout::snapshot::{self, FieldSdk, Hole, Skipped, Snapshot, StructSdk};
use bindings::{Bindings, c_name};

/// A struct or field that older SDK headers name differently from the SDK the
/// bindings were generated from.
///
/// When the headers lack the Rust name, the generator measures the SDK name
/// instead, so the layout is still compared, and the test reports the
/// difference as its own kind of mismatch.
struct Renamed {
	rust_struct: &'static str,
	/// The Rust field, or `None` for the struct itself.
	rust_field: Option<&'static str>,
	sdk: &'static str,
}

/// The bindings of `after-effects-sys` 0.4.0 come from SDK 25.6; the 23.4 SDK
/// (`PF_AE_PLUG_IN_SUBVERS` 28) uses these older names.
const RENAMED: &[Renamed] = &[
	Renamed {
		rust_struct: "PF_ParamDef",
		rust_field: Some("name_do_not_use_directly"),
		sdk: "name",
	},
	Renamed {
		rust_struct: "PF_ANSICallbacksBlock",
		rust_field: None,
		sdk: "PF_ANSICallbacks",
	},
	Renamed {
		rust_struct: "PF_ANSICallbacksBlock",
		rust_field: Some("unused_longA"),
		sdk: "ansi_procs",
	},
];

/// Structs checked although `crates/aexlo/src` and `playground/probe/src` do
/// not name them. The rest come from the names those sources use, followed
/// through field types, function pointer signatures and type aliases.
const EXTRA_ROOTS: &[&str] = &[
	// Part of the effect API in AE_Effect.h, next to PF_LRect, but no
	// struct aexlo uses holds it yet.
	"PF_FixedRect",
];

/// How C++ names the type of an entry.
pub enum Spelling {
	/// By name.
	Plain(String),
	/// As the type of a named member of another entry: bindgen's
	/// `Holder__bindgen_ty_N` for an anonymous type.
	Member { holder: String, member: String },
}

/// A type to measure.
pub struct Entry {
	pub rust: String,
	pub spelling: Spelling,
	pub union: bool,
	pub fields: Vec<EntryField>,
	/// Whether a Rust field is a bitfield unit, which C++ cannot name.
	pub has_bitfields: bool,
	/// The SDK name, when the headers only have it under an older one.
	pub sdk_name: Option<String>,
}

/// A field of an [`Entry`], with anonymous members flattened.
pub struct EntryField {
	/// The path in Rust.
	pub rust: String,
	/// The name in C++.
	pub cpp: String,
	/// The type, as the bindings write it.
	pub ty: String,
	/// The name older SDK headers use instead of `cpp`.
	pub renamed: Option<&'static str>,
	/// The entry for the field's type, when that type is anonymous.
	pub child: Option<usize>,
}

fn main() {
	if let Err(message) = run() {
		eprintln!("error: {message}");
		std::process::exit(1);
	}
}

fn run() -> Result<(), String> {
	let root = aexlo_sdk_layout::workspace_root();
	let sdk = std::env::var_os("AE_SDK_DIR")
		.filter(|dir| !dir.is_empty())
		.map(PathBuf::from)
		.unwrap_or_else(|| root.join("refs").join("AE SDK"));
	if !sdk.join("Headers").join("AE_Effect.h").is_file() {
		return Err(format!(
			"no SDK headers at {}: set AE_SDK_DIR to the folder that holds Headers/ and Util/",
			sdk.display()
		));
	}
	if std::env::var_os("AESDK_ROOT").is_some_and(|value| !value.is_empty()) {
		return Err(
			"AESDK_ROOT is set, so after-effects-sys generates its own bindings instead of the prebuilt ones \
			 aexlo ships with: unset it"
				.into(),
		);
	}

	let target = env!("SDK_LAYOUT_TARGET");
	let (version, bindings_path) = bindings_file(&root, target)?;
	let text = std::fs::read_to_string(&bindings_path).map_err(|e| format!("{}: {e}", bindings_path.display()))?;
	let bindings = Bindings::parse(&text).map_err(|e| format!("{}: {e}", bindings_path.display()))?;

	for dir in aexlo_sdk_layout::SOURCE_DIRS {
		if !root.join(dir).is_dir() {
			return Err(format!("{dir} is not a directory: update SOURCE_DIRS"));
		}
	}
	let used = aexlo_sdk_layout::source_identifiers(&root);
	let reached = bindings.closure(used.iter().map(String::as_str).chain(EXTRA_ROOTS.iter().copied()));

	let (mut entries, skipped) = plan(&bindings, &reached);

	let work = PathBuf::from(env!("OUT_DIR")).join("snapshot");
	std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
	let compiler = cpp::Compiler::new(target, &sdk);
	let declared: BTreeSet<String> = bindings::identifiers(&compiler.preprocess(&work)?).collect();
	for entry in &mut entries {
		if let Spelling::Plain(name) = &entry.spelling
			&& !declared.contains(name)
			&& let Some(renamed) = RENAMED
				.iter()
				.find(|r| r.rust_struct == entry.rust && r.rust_field.is_none() && declared.contains(r.sdk))
		{
			entry.spelling = Spelling::Plain(renamed.sdk.to_string());
			entry.sdk_name = Some(renamed.sdk.to_string());
		}
	}
	let present = |i: usize| match &entries[i].spelling {
		Spelling::Plain(name) => declared.contains(name),
		Spelling::Member { .. } => true,
	};
	let output = compiler.build_and_run(&work, &cpp::program(&entries, target.contains("windows"), present))?;
	let measured = Measured::parse(&output, &entries)?;

	let mut structs: Vec<snapshot::Struct> = entries
		.iter()
		.enumerate()
		.map(|(i, entry)| measured.to_struct(i, entry))
		.collect();
	structs.sort_by(|a, b| a.rust.cmp(&b.rust));

	let checked: BTreeSet<&str> = entries
		.iter()
		.map(|entry| entry.rust.as_str())
		.chain(skipped.iter().map(|skip| skip.rust.as_str()))
		.collect();
	let unchecked: Vec<String> = bindings
		.records
		.keys()
		.chain(bindings.aliases.keys())
		.filter(|name| {
			bindings
				.closure([name.as_str()])
				.iter()
				.any(|r| !checked.contains(r.as_str()))
		})
		.cloned()
		.collect();

	let file_name = bindings_path.file_name().unwrap().to_string_lossy().into_owned();
	let snapshot = Snapshot {
		target: target.to_string(),
		compiler: measured.compiler.clone(),
		sdk: measured.versions.clone(),
		bindings: snapshot::Bindings {
			package: "after-effects-sys".into(),
			version,
			file: file_name,
		},
		structs,
		skipped,
		unchecked,
	};

	let out = root
		.join("fixtures")
		.join("sdk-layout")
		.join(format!("{}.json", env!("SDK_LAYOUT_SNAPSHOT_NAME")));
	std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
	let value = serde_json::to_value(&snapshot).map_err(|e| e.to_string())?;
	let mut json = String::new();
	write_json(&mut json, &value, 0);
	json.push('\n');
	std::fs::write(&out, json).map_err(|e| format!("{}: {e}", out.display()))?;

	let missing = snapshot
		.structs
		.iter()
		.filter(|s| !matches!(s.sdk, StructSdk::Found { .. }))
		.count();
	let fields: usize = snapshot
		.structs
		.iter()
		.map(|s| match &s.sdk {
			StructSdk::Found { fields, .. } => fields.len(),
			_ => 0,
		})
		.sum();
	println!(
		"{}: {} structs ({} not found in the SDK), {} fields, {} skipped. Compare with `cargo test -p aexlo_sdk_layout`.",
		out.display(),
		snapshot.structs.len(),
		missing,
		fields,
		snapshot.skipped.len()
	);
	Ok(())
}

/// The version of `after-effects-sys` in the lock file and its prebuilt
/// bindings for this target.
fn bindings_file(root: &Path, target: &str) -> Result<(String, PathBuf), String> {
	let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
	let output = Command::new(cargo)
		.args(["metadata", "--format-version", "1", "--locked"])
		.current_dir(root)
		.output()
		.map_err(|e| format!("cargo metadata: {e}"))?;
	if !output.status.success() {
		return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&output.stderr)));
	}
	let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
	let package = metadata["packages"]
		.as_array()
		.and_then(|packages| packages.iter().find(|p| p["name"] == "after-effects-sys"))
		.ok_or("after-effects-sys is not in cargo metadata")?;
	let version = package["version"].as_str().ok_or("no version")?.to_string();
	let manifest = PathBuf::from(package["manifest_path"].as_str().ok_or("no manifest_path")?);
	// As `after-effects-sys`'s lib.rs picks them.
	let file = if target.contains("windows") {
		"bindings_win.rs"
	} else {
		"bindings_macos.rs"
	};
	Ok((version, manifest.parent().ok_or("manifest has no parent")?.join(file)))
}

/// Turn the reached records into entries to measure, and the rest into
/// skipped ones.
fn plan(bindings: &Bindings, reached: &BTreeSet<String>) -> (Vec<Entry>, Vec<Skipped>) {
	// Anonymous types: one held by a named member is measured through that
	// member, one held by an anonymous member is flattened into its holder.
	let mut held_by_member: BTreeMap<String, (String, String)> = BTreeMap::new();
	let mut flattened = BTreeSet::new();
	for name in reached {
		for field in &bindings.records[name].fields {
			if let Some(ty) = anonymous_type(&field.ty) {
				if field.name.starts_with("__bindgen_anon_") {
					flattened.insert(ty);
				} else {
					held_by_member.insert(ty, (name.clone(), c_name(&field.name).to_string()));
				}
			}
		}
	}

	let mut entries = Vec::new();
	let mut skipped = Vec::new();
	for name in reached {
		let record = &bindings.records[name];
		let reason = if record.is_opaque() {
			Some("opaque: the C headers only declare it, so the bindings have no fields")
		} else if flattened.contains(name) {
			Some("anonymous member: its fields are checked as fields of the struct that holds it")
		} else if name.contains("__bindgen_ty_") && !held_by_member.contains_key(name) {
			Some("anonymous type that no checked struct holds")
		} else {
			None
		};
		if let Some(reason) = reason {
			skipped.push(Skipped {
				rust: name.clone(),
				reason: reason.into(),
			});
			continue;
		}
		let spelling = match held_by_member.get(name) {
			Some((holder, member)) => Spelling::Member {
				holder: holder.clone(),
				member: member.clone(),
			},
			None => Spelling::Plain(name.clone()),
		};
		let mut fields = Vec::new();
		let mut has_bitfields = false;
		flatten(bindings, name, "", &mut fields, &mut has_bitfields);
		for field in &mut fields {
			field.renamed = RENAMED
				.iter()
				.find(|r| r.rust_struct == name && r.rust_field == Some(field.rust.as_str()))
				.map(|r| r.sdk);
		}
		entries.push(Entry {
			rust: name.clone(),
			spelling,
			union: record.union,
			fields,
			has_bitfields,
			sdk_name: None,
		});
	}

	// Point each named member of an anonymous type at that type's entry.
	let index: BTreeMap<String, usize> = entries.iter().enumerate().map(|(i, e)| (e.rust.clone(), i)).collect();
	for entry in &mut entries {
		for field in &mut entry.fields {
			field.child = anonymous_type(&field.ty)
				.filter(|ty| held_by_member.contains_key(ty))
				.and_then(|ty| index.get(&ty).copied());
		}
	}
	(entries, skipped)
}

/// The fields of `name` with anonymous members replaced by their fields.
fn flatten(bindings: &Bindings, name: &str, prefix: &str, out: &mut Vec<EntryField>, has_bitfields: &mut bool) {
	for field in &bindings.records[name].fields {
		let rust = format!("{prefix}{}", field.name);
		if field.name.starts_with("__bindgen_anon_")
			&& let Some(ty) = anonymous_type(&field.ty)
			&& bindings.records.contains_key(&ty)
		{
			flatten(bindings, &ty, &format!("{rust}."), out, has_bitfields);
			continue;
		}
		if field.name.starts_with("_bitfield_") {
			*has_bitfields = true;
		}
		out.push(EntryField {
			cpp: c_name(&field.name).to_string(),
			rust,
			ty: field.ty.clone(),
			renamed: None,
			child: None,
		});
	}
}

/// The bindgen anonymous type a field type is, or is an array of.
fn anonymous_type(ty: &str) -> Option<String> {
	bindings::identifiers(ty).find(|word| word.contains("__bindgen_ty_"))
}

/// What the layout program printed.
struct Measured {
	compiler: String,
	versions: BTreeMap<String, i64>,
	/// Per entry: size, align, union.
	layouts: BTreeMap<usize, (usize, usize, bool)>,
	incomplete: BTreeSet<usize>,
	fields: BTreeMap<(usize, usize), FieldSdk>,
	/// Fields found under their [`RENAMED`] name.
	renamed: BTreeSet<(usize, usize)>,
}

impl Measured {
	fn parse(output: &str, entries: &[Entry]) -> Result<Measured, String> {
		let mut measured = Measured {
			compiler: String::new(),
			versions: BTreeMap::new(),
			layouts: BTreeMap::new(),
			incomplete: BTreeSet::new(),
			fields: BTreeMap::new(),
			renamed: BTreeSet::new(),
		};
		for line in output.lines() {
			let bad = || format!("unexpected line from the layout program: {line:?}");
			let mut words = line.split(' ');
			let tag = words.next().unwrap_or_default();
			if tag == "C" {
				measured.compiler = line[2..].trim().to_string();
				continue;
			}
			if tag == "V" {
				let name = words.next().ok_or_else(bad)?;
				let value = words.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?;
				measured.versions.insert(name.to_string(), value);
				continue;
			}
			let numbers: Vec<usize> = words.map(|w| w.parse().map_err(|_| bad())).collect::<Result<_, _>>()?;
			let at = |k: usize| numbers.get(k).copied().ok_or_else(bad);
			let i = at(0)?;
			if i >= entries.len() {
				return Err(bad());
			}
			match tag {
				"S" => {
					measured.layouts.insert(i, (at(1)?, at(2)?, at(3)? == 1));
				}
				"I" => {
					measured.incomplete.insert(i);
				}
				"F" => {
					let sdk = FieldSdk::Found {
						offset: at(2)?,
						size: at(3)?,
						align: at(4)?,
					};
					measured.fields.insert((i, at(1)?), sdk);
				}
				"M" => {
					measured.fields.insert((i, at(1)?), FieldSdk::Missing);
				}
				"B" => {
					measured.fields.insert((i, at(1)?), FieldSdk::Bitfield);
				}
				"R" => {
					measured.renamed.insert((i, at(1)?));
				}
				_ => return Err(bad()),
			}
		}
		Ok(measured)
	}

	fn to_struct(&self, i: usize, entry: &Entry) -> snapshot::Struct {
		let cpp = match &entry.spelling {
			Spelling::Plain(name) => name.clone(),
			Spelling::Member { holder, member } => format!("decltype({holder}::{member})"),
		};
		let sdk = if self.incomplete.contains(&i) {
			StructSdk::Incomplete
		} else if let Some(&(size, align, union)) = self.layouts.get(&i) {
			let fields: Vec<snapshot::Field> = entry
				.fields
				.iter()
				.enumerate()
				.map(|(j, f)| snapshot::Field {
					rust: f.rust.clone(),
					cpp: f.cpp.clone(),
					sdk_name: f.renamed.filter(|_| self.renamed.contains(&(i, j))).map(str::to_string),
					sdk: match self.fields.get(&(i, j)) {
						Some(FieldSdk::Found { offset, size, align }) => FieldSdk::Found {
							offset: *offset,
							size: *size,
							align: *align,
						},
						Some(FieldSdk::Bitfield) => FieldSdk::Bitfield,
						Some(FieldSdk::Missing) | None => FieldSdk::Missing,
					},
				})
				.collect();
			let (holes, holes_unknown) =
				if entry.has_bitfields || fields.iter().any(|f| matches!(f.sdk, FieldSdk::Bitfield)) {
					(Vec::new(), Some("bitfields have no byte offset".to_string()))
				} else {
					(holes(&fields, size, align, union), None)
				};
			StructSdk::Found {
				size,
				align,
				fields,
				holes,
				holes_unknown,
			}
		} else {
			StructSdk::Missing
		};
		snapshot::Struct {
			rust: entry.rust.clone(),
			cpp,
			sdk_name: entry.sdk_name.clone(),
			union: entry.union,
			sdk,
		}
	}
}

/// SDK bytes that no found field covers and that alignment does not explain.
///
/// A field of alignment `a` starts at most at the end of the previous field
/// rounded up to `a` (less under `#pragma pack`), and the struct ends at most
/// at its last field's end rounded up to its own alignment. Anything past
/// that is a field the Rust struct does not name.
fn holes(fields: &[snapshot::Field], size: usize, align: usize, union: bool) -> Vec<Hole> {
	let mut found: Vec<(usize, usize, usize)> = fields
		.iter()
		.filter_map(|f| match f.sdk {
			FieldSdk::Found { offset, size, align } => Some((offset, size, align)),
			_ => None,
		})
		.collect();
	found.sort();
	let round_up = |n: usize, a: usize| n.div_ceil(a.max(1)) * a.max(1);
	let mut holes = Vec::new();
	let mut end = 0;
	for &(offset, field_size, field_align) in &found {
		if !union && offset > round_up(end, field_align) {
			holes.push(Hole {
				offset: end,
				size: offset - end,
			});
		}
		end = end.max(offset + field_size);
	}
	if size > round_up(end, align) {
		holes.push(Hole {
			offset: end,
			size: size - end,
		});
	}
	holes
}

/// Pretty JSON with each small object on one line, so a field is one line of
/// the snapshot and a diff of it reads one changed field per line.
fn write_json(out: &mut String, value: &serde_json::Value, indent: usize) {
	use serde_json::Value;

	const ONE_LINE: usize = 140;
	let pad = |out: &mut String, n: usize| out.extend(std::iter::repeat_n("  ", n));
	match value {
		Value::Object(map) => {
			let compact = value.to_string();
			if map.is_empty() || compact.len() <= ONE_LINE {
				out.push_str(&compact);
				return;
			}
			out.push_str("{\n");
			for (k, (key, item)) in map.iter().enumerate() {
				pad(out, indent + 1);
				out.push_str(&Value::String(key.clone()).to_string());
				out.push_str(": ");
				write_json(out, item, indent + 1);
				out.push_str(if k + 1 < map.len() { ",\n" } else { "\n" });
			}
			pad(out, indent);
			out.push('}');
		}
		Value::Array(items) if !items.is_empty() => {
			out.push_str("[\n");
			for (k, item) in items.iter().enumerate() {
				pad(out, indent + 1);
				write_json(out, item, indent + 1);
				out.push_str(if k + 1 < items.len() { ",\n" } else { "\n" });
			}
			pad(out, indent);
			out.push(']');
		}
		other => out.push_str(&other.to_string()),
	}
}
