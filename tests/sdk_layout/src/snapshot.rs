//! The snapshot file: struct layouts as the After Effects SDK headers declare
//! them, measured by a C++ compiler.
//!
//! The snapshot generator writes it, `build.rs` reads it to generate the Rust
//! side of the check, and the test compares the two. `build.rs` includes this
//! file with `#[path]`, so it uses nothing outside `serde`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One `fixtures/sdk-layout/<os>-<arch>.json` file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
	/// The Rust target triple the snapshot was taken for.
	pub target: String,
	/// The C++ compiler that measured the SDK.
	pub compiler: String,
	/// Version macros of the SDK headers that were measured.
	pub sdk: BTreeMap<String, i64>,
	/// The Rust bindings the struct and field names come from.
	pub bindings: Bindings,
	/// Every struct checked, sorted by Rust name.
	pub structs: Vec<Struct>,
	/// Structs that were reached but cannot be measured, with the reason.
	pub skipped: Vec<Skipped>,
	/// Every name in the bindings that leads to a struct not in `structs` or
	/// `skipped`. `crates/aexlo/src` and `playground/probe/src` must not use
	/// them until the snapshot is regenerated.
	pub unchecked: Vec<String>,
}

/// Where the Rust names come from.
#[derive(Debug, Serialize, Deserialize)]
pub struct Bindings {
	/// The crate: `after-effects-sys`.
	pub package: String,
	/// Its version, as `Cargo.lock` records it.
	pub version: String,
	/// The prebuilt bindings file inside the crate.
	pub file: String,
}

/// A struct or union, as the SDK lays it out.
#[derive(Debug, Serialize, Deserialize)]
pub struct Struct {
	/// The name in `after_effects_sys`.
	pub rust: String,
	/// How the C++ generator named the type.
	pub cpp: String,
	/// The SDK's name for it, when the headers lack the Rust name and have
	/// it under an older one.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub sdk_name: Option<String>,
	/// `true` for a union.
	pub union: bool,
	/// The SDK side, or why there is none.
	#[serde(flatten)]
	pub sdk: StructSdk,
}

/// The SDK side of a struct.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "sdk", rename_all = "snake_case")]
pub enum StructSdk {
	/// The SDK declares the type.
	Found {
		size: usize,
		align: usize,
		/// Every field of the Rust struct, in declaration order. A field of
		/// an anonymous member is listed with its path, `__bindgen_anon_1.x`.
		fields: Vec<Field>,
		/// Bytes of the SDK struct that no field name of the Rust struct
		/// accounts for, beyond alignment padding: a field the bindings lack.
		holes: Vec<Hole>,
		/// Why `holes` was not computed, when it was not.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		holes_unknown: Option<String>,
	},
	/// The SDK headers do not declare a type of this name.
	Missing,
	/// The SDK declares the name but not its definition.
	Incomplete,
}

/// A field of a Rust struct, and the SDK field of the same name.
#[derive(Debug, Serialize, Deserialize)]
pub struct Field {
	/// The path in Rust, for `offset_of!`.
	pub rust: String,
	/// The name in C++.
	pub cpp: String,
	/// The SDK's name for it, when the headers lack `cpp` and have the field
	/// under an older name.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub sdk_name: Option<String>,
	#[serde(flatten)]
	pub sdk: FieldSdk,
}

/// The SDK side of a field.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "sdk", rename_all = "snake_case")]
pub enum FieldSdk {
	/// The SDK field, measured.
	Found { offset: usize, size: usize, align: usize },
	/// The SDK struct has no field of this name.
	Missing,
	/// The SDK field is a bitfield, which has no offset in bytes.
	Bitfield,
}

/// A run of SDK bytes no Rust field name accounts for.
#[derive(Debug, Serialize, Deserialize)]
pub struct Hole {
	pub offset: usize,
	pub size: usize,
}

/// A struct the check cannot measure.
#[derive(Debug, Serialize, Deserialize)]
pub struct Skipped {
	pub rust: String,
	pub reason: String,
}
