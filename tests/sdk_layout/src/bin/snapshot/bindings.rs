//! Reads the structs, unions and type aliases out of a bindgen output file.
//!
//! bindgen writes every item at column 0 and ends a struct body with a `}`
//! line of its own, so the file is read line by line, not parsed as Rust.

use std::collections::{BTreeMap, BTreeSet};

/// A struct or union.
pub struct Record {
	pub union: bool,
	pub fields: Vec<RecordField>,
}

/// One field of a [`Record`].
pub struct RecordField {
	pub name: String,
	/// The type, as written in the bindings.
	pub ty: String,
}

impl Record {
	/// bindgen's stand-in for a type the C headers only declare.
	pub fn is_opaque(&self) -> bool {
		self.fields.iter().any(|field| field.name == "_unused")
	}
}

/// The items of one bindings file.
#[derive(Default)]
pub struct Bindings {
	pub records: BTreeMap<String, Record>,
	/// `pub type NAME = RHS;`, by name.
	pub aliases: BTreeMap<String, String>,
}

impl Bindings {
	/// Read every struct, union and type alias in `text`.
	pub fn parse(text: &str) -> Bindings {
		let mut bindings = Bindings::default();
		let mut lines = text.lines();
		while let Some(line) = lines.next() {
			let record = line
				.strip_prefix("pub struct ")
				.map(|rest| (false, rest))
				.or_else(|| line.strip_prefix("pub union ").map(|rest| (true, rest)));
			if let Some((union, rest)) = record {
				// Generic helpers (`__BindgenBitfieldUnit<Storage>`) and tuple
				// structs are not SDK types.
				let Some(name) = rest.strip_suffix(" {") else { continue };
				let mut body = Vec::new();
				for line in lines.by_ref() {
					if line == "}" {
						break;
					}
					body.push(line);
				}
				if name.contains('<') {
					continue;
				}
				let fields = parse_fields(&body);
				bindings.records.insert(name.to_string(), Record { union, fields });
			} else if let Some(rest) = line.strip_prefix("pub type ") {
				let mut item = rest.to_string();
				while !item.trim_end().ends_with(';') {
					let Some(next) = lines.next() else { break };
					item.push('\n');
					item.push_str(next);
				}
				if let Some((name, rhs)) = item.split_once(" = ") {
					let rhs = rhs.trim_end().trim_end_matches(';');
					bindings.aliases.insert(name.trim().to_string(), rhs.to_string());
				}
			}
		}
		bindings
	}

	/// The names `name` refers to directly: field types of a record, or the
	/// right-hand side of an alias.
	pub fn references(&self, name: &str) -> BTreeSet<String> {
		let mut out = BTreeSet::new();
		if let Some(record) = self.records.get(name) {
			for field in &record.fields {
				out.extend(identifiers(&field.ty));
			}
		} else if let Some(rhs) = self.aliases.get(name) {
			out.extend(identifiers(rhs));
		}
		out.retain(|other| other != name && self.is_item(other));
		out
	}

	/// Whether `name` is a record or an alias in these bindings.
	pub fn is_item(&self, name: &str) -> bool {
		self.records.contains_key(name) || self.aliases.contains_key(name)
	}

	/// Every record reachable from `roots` through field types and aliases,
	/// the roots included.
	pub fn closure<'a>(&self, roots: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
		let mut seen = BTreeSet::new();
		let mut stack: Vec<String> = roots.into_iter().map(str::to_string).collect();
		while let Some(name) = stack.pop() {
			if !self.is_item(&name) || !seen.insert(name.clone()) {
				continue;
			}
			stack.extend(self.references(&name));
		}
		seen.retain(|name| self.records.contains_key(name));
		seen
	}
}

/// Split a struct body into fields at the commas outside any brackets.
fn parse_fields(body: &[&str]) -> Vec<RecordField> {
	// Attributes (`#[doc = "..."]`) sit on lines of their own.
	let text: String = body
		.iter()
		.map(|line| line.trim())
		.filter(|line| !line.starts_with("#[") && !line.starts_with("//"))
		.collect::<Vec<_>>()
		.join(" ")
		.replace("->", " returns ");

	let mut fields = Vec::new();
	let mut depth = 0i32;
	let mut start = 0;
	for (i, c) in text.char_indices() {
		match c {
			'(' | '<' | '[' => depth += 1,
			')' | '>' | ']' => depth -= 1,
			',' if depth == 0 => {
				push_field(&mut fields, &text[start..i]);
				start = i + 1;
			}
			_ => {}
		}
	}
	push_field(&mut fields, &text[start..]);
	fields
}

fn push_field(fields: &mut Vec<RecordField>, item: &str) {
	let item = item.trim();
	let item = item.strip_prefix("pub ").unwrap_or(item);
	if let Some((name, ty)) = item.split_once(':') {
		fields.push(RecordField {
			name: name.trim().to_string(),
			ty: ty.trim().to_string(),
		});
	}
}

/// Every identifier in `text`.
pub fn identifiers(text: &str) -> impl Iterator<Item = String> + '_ {
	text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
		.filter(|word| word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
		.map(str::to_string)
}

/// The C name of a Rust field: bindgen appends `_` to a Rust keyword.
pub fn c_name(rust: &str) -> &str {
	const KEYWORDS: &[&str] = &[
		"abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate", "do", "dyn",
		"else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "macro",
		"match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return", "self", "static", "struct", "super",
		"trait", "true", "try", "type", "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
	];
	match rust.strip_suffix('_') {
		Some(stripped) if KEYWORDS.contains(&stripped) => stripped,
		_ => rust,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const SAMPLE: &str = r#"
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct PF_Thing {
    #[doc = " A doc, with a comma, and { braces }."]
    pub inter: PF_InteractCallbacks,
    pub cb: ::std::option::Option<
        unsafe extern "C" fn(a: *mut PF_Pixel, b: A_long) -> PF_Err,
    >,
    pub type_: [A_char; 32usize],
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct _PF_Opaque {
    _unused: [u8; 0],
}
pub type PF_World = PF_Thing;
pub type PF_Func = ::std::option::Option<
    unsafe extern "C" fn(x: *mut _PF_Opaque) -> PF_Err,
>;
"#;

	#[test]
	fn reads_fields_aliases_and_opaque_records() {
		let bindings = Bindings::parse(SAMPLE);
		let thing = &bindings.records["PF_Thing"];
		let names: Vec<_> = thing.fields.iter().map(|f| f.name.as_str()).collect();
		assert_eq!(names, ["inter", "cb", "type_"]);
		assert!(thing.fields[1].ty.contains("PF_Pixel"));
		assert!(bindings.records["_PF_Opaque"].is_opaque());
		assert_eq!(bindings.aliases["PF_World"], "PF_Thing");
		assert!(bindings.aliases["PF_Func"].contains("_PF_Opaque"));
		assert_eq!(c_name("type_"), "type");
		assert_eq!(c_name("ui_flags_"), "ui_flags_");
	}

	#[test]
	fn closure_follows_aliases_and_function_pointers() {
		let bindings = Bindings::parse(SAMPLE);
		let reached = bindings.closure(["PF_Func"]);
		assert_eq!(reached.into_iter().collect::<Vec<_>>(), ["_PF_Opaque"]);
		let reached = bindings.closure(["PF_World"]);
		assert_eq!(reached.into_iter().collect::<Vec<_>>(), ["PF_Thing"]);
	}
}
