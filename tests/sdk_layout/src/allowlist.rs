//! Mismatches that are known and explained, so the test does not fail on
//! them. Each entry matches one mismatch exactly; an entry that matches
//! nothing fails the test, so the list cannot outlive what it explains.

use crate::{Kind, Mismatch};

/// One known mismatch.
#[derive(Debug)]
pub struct Known {
	pub kind: Kind,
	/// The Rust struct.
	pub rust_struct: &'static str,
	/// The Rust field path, or the `@offset+size` of SDK bytes; `None` for
	/// the struct itself.
	pub field: Option<&'static str>,
	/// Why it is not a bug in aexlo, or why it does not matter.
	pub reason: &'static str,
}

impl Known {
	pub fn matches(&self, mismatch: &Mismatch) -> bool {
		self.kind == mismatch.kind
			&& self.rust_struct == mismatch.rust_struct
			&& self.field == mismatch.field.as_deref()
	}
}

/// The bindings come from SDK 25.6 and the Windows snapshot from the 23.4 SDK
/// (`PF_AE_PLUG_IN_SUBVERS` 28), which spells these three names the old way.
/// Each is measured under its old name, and the layout is the same.
const OLDER_SDK_NAME: &str = "renamed after SDK 23.4; the snapshot measures the old name, at the same offset and size";

pub const KNOWN: &[Known] = &[
	Known {
		kind: Kind::NamedDifferently,
		rust_struct: "PF_ANSICallbacksBlock",
		field: None,
		reason: OLDER_SDK_NAME,
	},
	Known {
		kind: Kind::NamedDifferently,
		rust_struct: "PF_ANSICallbacksBlock",
		field: Some("unused_longA"),
		reason: OLDER_SDK_NAME,
	},
	Known {
		kind: Kind::NamedDifferently,
		rust_struct: "PF_ParamDef",
		field: Some("name_do_not_use_directly"),
		reason: OLDER_SDK_NAME,
	},
];
