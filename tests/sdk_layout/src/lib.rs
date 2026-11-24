//! Checks that the struct layouts of `after-effects-sys` match the After
//! Effects SDK headers.
//!
//! aexlo and `playground/probe` both use these bindings, so a field at the
//! wrong offset is wrong on both sides and no trace diff between them shows
//! it. This crate compares the bindings with a snapshot measured from the real
//! headers by a C++ compiler (`fixtures/sdk-layout/<os>-<arch>.json`, written
//! by the `snapshot` binary). See `docs/sdk-layout.md`.

pub mod allowlist;
pub mod snapshot;

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use snapshot::{FieldSdk, Snapshot, StructSdk};

// Only the generated `measure.rs` names the bindings, and a target without a
// snapshot has none; the bindings still have to build there.
use after_effects_sys as _;

/// The sources whose use of the bindings the snapshot covers, from the
/// workspace root.
pub const SOURCE_DIRS: &[&str] = &["crates/aexlo/src", "playground/probe/src"];

/// The workspace root.
pub fn workspace_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Every identifier in the `.rs` files of [`SOURCE_DIRS`].
pub fn source_identifiers(root: &Path) -> BTreeSet<String> {
	let mut out = BTreeSet::new();
	let mut stack: Vec<PathBuf> = SOURCE_DIRS.iter().map(|dir| root.join(dir)).collect();
	while let Some(path) = stack.pop() {
		if path.is_dir() {
			stack.extend(
				std::fs::read_dir(&path)
					.into_iter()
					.flatten()
					.flatten()
					.map(|e| e.path()),
			);
		} else if path.extension().is_some_and(|e| e == "rs")
			&& let Ok(text) = std::fs::read_to_string(&path)
		{
			out.extend(
				text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
					.filter(|word| word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
					.map(str::to_string),
			);
		}
	}
	out
}

/// A struct of `after_effects_sys`, measured by the compiler.
#[derive(Debug)]
pub struct RustStruct {
	pub rust: &'static str,
	pub size: usize,
	pub align: usize,
	pub fields: Vec<RustField>,
}

/// A field of a [`RustStruct`].
#[derive(Debug)]
pub struct RustField {
	pub rust: &'static str,
	pub offset: usize,
	pub size: usize,
	/// `align_of` the field's type.
	pub align: usize,
}

/// What differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
	/// The SDK headers declare no type of this name.
	StructNotInSdk,
	/// The SDK declares the type but does not define it.
	StructIncomplete,
	/// The SDK headers have the struct or field under another name. Its
	/// layout is still compared, under that name.
	NamedDifferently,
	/// `size_of` differs.
	Size,
	/// `align_of` differs.
	Align,
	/// The Rust struct has a field the SDK struct does not name.
	FieldNotInSdk,
	/// The field is at another offset.
	FieldOffset,
	/// The field has another size.
	FieldSize,
	/// The field's type has another alignment.
	FieldAlign,
	/// The SDK struct has bytes no Rust field name accounts for: a field the
	/// bindings lack, or name differently.
	SdkFieldNotInRust,
}

impl Kind {
	fn title(self) -> &'static str {
		match self {
			Kind::StructNotInSdk => "structs the SDK headers do not declare",
			Kind::StructIncomplete => "structs the SDK only declares",
			Kind::NamedDifferently => "named differently in the SDK headers (layout compared under the SDK name)",
			Kind::Size => "size_of differs",
			Kind::Align => "align_of differs",
			Kind::FieldNotInSdk => "Rust fields the SDK struct does not have",
			Kind::FieldOffset => "field offsets differ",
			Kind::FieldSize => "field sizes differ",
			Kind::FieldAlign => "field alignments differ",
			Kind::SdkFieldNotInRust => "SDK fields the Rust struct does not have",
		}
	}
}

/// One difference between the bindings and the SDK.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mismatch {
	pub kind: Kind,
	pub rust_struct: String,
	/// The Rust field path; for [`Kind::SdkFieldNotInRust`], the SDK byte
	/// range as `@offset+size`.
	pub field: Option<String>,
	pub detail: String,
}

impl fmt::Display for Mismatch {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self.field {
			Some(field) => write!(f, "{}.{}: {}", self.rust_struct, field, self.detail),
			None => write!(f, "{}: {}", self.rust_struct, self.detail),
		}
	}
}

/// Every difference between `measured` and `snapshot`, and the number of
/// fields compared.
pub fn compare(snapshot: &Snapshot, measured: &[RustStruct]) -> (Vec<Mismatch>, usize) {
	let mut out = Vec::new();
	let mut compared = 0;
	assert_eq!(
		snapshot.structs.len(),
		measured.len(),
		"build.rs measured another list of structs than the snapshot has"
	);
	for (s, rust) in snapshot.structs.iter().zip(measured) {
		assert_eq!(s.rust, rust.rust, "build.rs measured the structs in another order");
		let mismatch = |kind, field: Option<&str>, detail: String| Mismatch {
			kind,
			rust_struct: s.rust.clone(),
			field: field.map(str::to_string),
			detail,
		};
		let (size, align, fields, holes) = match &s.sdk {
			StructSdk::Found {
				size,
				align,
				fields,
				holes,
				..
			} => (*size, *align, fields, holes),
			StructSdk::Missing => {
				out.push(mismatch(
					Kind::StructNotInSdk,
					None,
					format!("the SDK headers declare no `{}` (Rust size {})", s.cpp, rust.size),
				));
				continue;
			}
			StructSdk::Incomplete => {
				out.push(mismatch(
					Kind::StructIncomplete,
					None,
					format!("the SDK only declares `{}`, the bindings define it", s.cpp),
				));
				continue;
			}
		};
		if let Some(sdk_name) = &s.sdk_name {
			out.push(mismatch(
				Kind::NamedDifferently,
				None,
				format!("the SDK headers name it `{sdk_name}`"),
			));
		}
		if size != rust.size {
			out.push(mismatch(
				Kind::Size,
				None,
				format!("size SDK {size}, Rust {}", rust.size),
			));
		}
		if align != rust.align {
			out.push(mismatch(
				Kind::Align,
				None,
				format!("align SDK {align}, Rust {}", rust.align),
			));
		}
		assert_eq!(fields.len(), rust.fields.len(), "{}: field lists differ", s.rust);
		for (field, rust_field) in fields.iter().zip(&rust.fields) {
			let name = Some(field.rust.as_str());
			if let Some(sdk_name) = &field.sdk_name {
				out.push(mismatch(
					Kind::NamedDifferently,
					name,
					format!("the SDK struct names it `{sdk_name}`"),
				));
			}
			match field.sdk {
				FieldSdk::Found { offset, size, align } => {
					compared += 1;
					if offset != rust_field.offset {
						out.push(mismatch(
							Kind::FieldOffset,
							name,
							format!("offset SDK {offset}, Rust {}", rust_field.offset),
						));
					}
					if size != rust_field.size {
						out.push(mismatch(
							Kind::FieldSize,
							name,
							format!("size SDK {size}, Rust {}", rust_field.size),
						));
					}
					if align != rust_field.align {
						out.push(mismatch(
							Kind::FieldAlign,
							name,
							format!("align SDK {align}, Rust {}", rust_field.align),
						));
					}
				}
				FieldSdk::Missing => out.push(mismatch(
					Kind::FieldNotInSdk,
					name,
					format!(
						"the SDK struct has no `{}` (Rust offset {}, size {})",
						field.cpp, rust_field.offset, rust_field.size
					),
				)),
				// No byte offset to compare; the size and the neighbours
				// still are.
				FieldSdk::Bitfield => {}
			}
		}
		for hole in holes {
			out.push(mismatch(
				Kind::SdkFieldNotInRust,
				Some(&format!("@{}+{}", hole.offset, hole.size)),
				format!(
					"SDK bytes {}..{} belong to no field the Rust struct names",
					hole.offset,
					hole.offset + hole.size
				),
			));
		}
	}
	out.sort();
	(out, compared)
}

/// The report of a failed check: every mismatch, grouped by kind.
pub fn report(mismatches: &[&Mismatch]) -> String {
	let mut out = String::new();
	let mut kind = None;
	for m in mismatches {
		if kind != Some(m.kind) {
			kind = Some(m.kind);
			let count = mismatches.iter().filter(|other| other.kind == m.kind).count();
			out.push_str(&format!("\n{} ({count}):\n", m.kind.title()));
		}
		out.push_str(&format!("  {m}\n"));
	}
	out
}

#[cfg(sdk_layout_snapshot)]
macro_rules! field {
	($rust:literal, $ty:ty, $($field:ident).+) => {{
		fn layout<F>(_: *const F) -> (usize, usize) {
			(::core::mem::size_of::<F>(), ::core::mem::align_of::<F>())
		}
		let value = ::core::mem::MaybeUninit::<$ty>::uninit();
		// SAFETY: a raw borrow of a field reads nothing.
		let (size, align) = layout(unsafe { &raw const (*value.as_ptr()).$($field).+ });
		RustField {
			rust: $rust,
			offset: ::core::mem::offset_of!($ty, $($field).+),
			size,
			align,
		}
	}};
}

#[cfg(sdk_layout_snapshot)]
mod measured {
	use super::{RustField, RustStruct};

	include!(concat!(env!("OUT_DIR"), "/measure.rs"));
}

/// The snapshot for this target, if one is committed.
pub fn this_target_snapshot() -> Option<Snapshot> {
	#[cfg(sdk_layout_snapshot)]
	{
		let text = include_str!(concat!(
			"../../../fixtures/sdk-layout/",
			env!("SDK_LAYOUT_SNAPSHOT_NAME"),
			".json"
		));
		Some(serde_json::from_str(text).expect("the snapshot parsed in build.rs"))
	}
	#[cfg(not(sdk_layout_snapshot))]
	None
}

/// The structs of the snapshot for this target, measured in Rust.
pub fn this_target_measured() -> Vec<RustStruct> {
	#[cfg(sdk_layout_snapshot)]
	{
		measured::measure()
	}
	#[cfg(not(sdk_layout_snapshot))]
	Vec::new()
}

#[cfg(test)]
mod tests {
	use super::*;

	const NO_SNAPSHOT: &str = "no SDK layout snapshot for this target in fixtures/sdk-layout: \
		run `cargo run -p aexlo_sdk_layout --bin snapshot` here with the SDK (docs/sdk-layout.md)";

	/// `compare` on hand-made data, so each check is tested on every target.
	#[test]
	fn compare_reports_each_kind_of_field_mismatch() {
		use snapshot::{Bindings, Field, Hole, Struct};

		let found = |rust: &str, offset, size, align| Field {
			rust: rust.into(),
			cpp: rust.into(),
			sdk_name: None,
			sdk: FieldSdk::Found { offset, size, align },
		};
		let snapshot = Snapshot {
			target: String::new(),
			compiler: String::new(),
			sdk: Default::default(),
			bindings: Bindings {
				package: String::new(),
				version: String::new(),
				file: String::new(),
			},
			structs: vec![Struct {
				rust: "S".into(),
				cpp: "S".into(),
				sdk_name: None,
				union: false,
				sdk: StructSdk::Found {
					size: 24,
					align: 8,
					fields: vec![
						found("same", 0, 4, 4),
						found("moved", 4, 4, 4),
						found("resized", 8, 4, 4),
						found("realigned", 16, 8, 8),
						Field {
							rust: "extra".into(),
							cpp: "extra".into(),
							sdk_name: None,
							sdk: FieldSdk::Missing,
						},
					],
					holes: vec![Hole { offset: 12, size: 4 }],
					holes_unknown: None,
				},
			}],
			skipped: Vec::new(),
			unchecked: Vec::new(),
		};
		let field = |rust, offset, size, align| RustField {
			rust,
			offset,
			size,
			align,
		};
		let measured = [RustStruct {
			rust: "S",
			size: 32,
			align: 8,
			fields: vec![
				field("same", 0, 4, 4),
				field("moved", 8, 4, 4),
				field("resized", 8, 8, 4),
				field("realigned", 16, 8, 4),
				field("extra", 24, 4, 4),
			],
		}];

		let (mismatches, compared) = compare(&snapshot, &measured);
		let kinds: Vec<(Kind, Option<&str>)> = mismatches.iter().map(|m| (m.kind, m.field.as_deref())).collect();
		assert_eq!(compared, 4);
		assert_eq!(
			kinds,
			[
				(Kind::Size, None),
				(Kind::FieldNotInSdk, Some("extra")),
				(Kind::FieldOffset, Some("moved")),
				(Kind::FieldSize, Some("resized")),
				(Kind::FieldAlign, Some("realigned")),
				(Kind::SdkFieldNotInRust, Some("@12+4")),
			]
		);
	}

	fn snapshot() -> Snapshot {
		this_target_snapshot().unwrap_or_else(|| {
			panic!("{NO_SNAPSHOT} ({}.json)", env!("SDK_LAYOUT_SNAPSHOT_NAME"));
		})
	}

	#[test]
	#[cfg_attr(
		not(sdk_layout_snapshot),
		ignore = "no SDK layout snapshot for this target in fixtures/sdk-layout (docs/sdk-layout.md)"
	)]
	fn struct_layouts_match_the_sdk() {
		let snapshot = snapshot();
		let (mismatches, fields) = compare(&snapshot, &this_target_measured());
		// A snapshot taken while the sources could not be read would cover
		// nothing and pass.
		assert!(
			snapshot.structs.iter().any(|s| s.rust == "PF_OutData") && fields > 0,
			"the snapshot does not cover PF_OutData: regenerate it (docs/sdk-layout.md)"
		);

		let mut unexpected = Vec::new();
		let mut allowed = Vec::new();
		for m in &mismatches {
			match allowlist::KNOWN.iter().find(|known| known.matches(m)) {
				Some(known) => allowed.push((m, known)),
				None => unexpected.push(m),
			}
		}
		let stale: Vec<_> = allowlist::KNOWN
			.iter()
			.filter(|known| !mismatches.iter().any(|m| known.matches(m)))
			.collect();

		println!(
			"checked {} structs and {} fields of {} {} against {} ({}); {} skipped",
			snapshot.structs.len(),
			fields,
			snapshot.bindings.package,
			snapshot.bindings.version,
			snapshot.compiler,
			snapshot
				.sdk
				.iter()
				.map(|(k, v)| format!("{k}={v}"))
				.collect::<Vec<_>>()
				.join(", "),
			snapshot.skipped.len(),
		);
		for (m, known) in &allowed {
			println!("known: {m} ({})", known.reason);
		}

		let mut message = String::new();
		if !unexpected.is_empty() {
			message.push_str(&format!(
				"{} layout mismatches between after_effects_sys and the SDK headers:\n{}",
				unexpected.len(),
				report(&unexpected)
			));
		}
		if !stale.is_empty() {
			message.push_str("\nallowlist entries that match no mismatch any more (remove them):\n");
			for known in stale {
				message.push_str(&format!("  {known:?}\n"));
			}
		}
		assert!(message.is_empty(), "{message}");
	}

	#[test]
	#[cfg_attr(
		not(sdk_layout_snapshot),
		ignore = "no SDK layout snapshot for this target in fixtures/sdk-layout (docs/sdk-layout.md)"
	)]
	fn snapshot_is_of_the_locked_bindings() {
		let snapshot = snapshot();
		assert_eq!(
			snapshot.bindings.version,
			env!("SDK_LAYOUT_BINDINGS_VERSION"),
			"the snapshot measured another version of after-effects-sys than Cargo.lock has: regenerate it"
		);
		assert_eq!(snapshot.target, env!("SDK_LAYOUT_TARGET"));
	}

	/// A struct aexlo or the probe starts using must be in the snapshot, or
	/// the check would not cover it.
	#[test]
	#[cfg_attr(
		not(sdk_layout_snapshot),
		ignore = "no SDK layout snapshot for this target in fixtures/sdk-layout (docs/sdk-layout.md)"
	)]
	fn snapshot_covers_every_struct_the_sources_use() {
		let snapshot = snapshot();
		let root = workspace_root();
		for dir in SOURCE_DIRS {
			assert!(root.join(dir).is_dir(), "{dir} is not a directory: update SOURCE_DIRS");
		}
		let used = source_identifiers(&root);
		let uncovered: Vec<&String> = snapshot.unchecked.iter().filter(|name| used.contains(*name)).collect();
		assert!(
			uncovered.is_empty(),
			"{} mention these names (in code or comments), which lead to structs the snapshot does not cover: \
			 {uncovered:?}\nRegenerate it: `cargo run -p aexlo_sdk_layout --bin snapshot` (docs/sdk-layout.md)",
			SOURCE_DIRS.join(" and ")
		);
	}
}
