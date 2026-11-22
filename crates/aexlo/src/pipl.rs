//! The PiPL ("plug-in property list") resource: what After Effects knows about
//! an effect before loading it, and what it compares the effect's
//! `PF_Cmd_GLOBAL_SETUP` answers against.
//!
//! On Windows the PiPL is a `PIPL` resource (id 16000) in the `.aex` and is
//! little-endian. On macOS it is a `PiPL` resource in the bundle's
//! `Contents/Resources/*.rsrc` (a classic resource file, or the binary's
//! resource fork) and is big-endian.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::core::error::{AexloError, Result};
use crate::{OutFlags, OutFlags2};

/// The resource id After Effects reads the PiPL from.
const PIPL_ID: i16 = 16000;

/// A plugin's PiPL resource.
///
/// ```no_run
/// # fn main() -> aexlo::Result<()> {
/// if let Some(pipl) = aexlo::Pipl::read("SDK_Noise.plugin")? {
///     println!("{:?} {:?}", pipl.name(), pipl.effect_version());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipl {
	source: PathBuf,
	version: u32,
	properties: Vec<PiplProperty>,
}

/// One property of a [`Pipl`]: a vendor, a key and its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiplProperty {
	vendor: String,
	key: String,
	id: u32,
	data: Vec<u8>,
	big_endian: bool,
}

/// A property's data, decoded by what its key means (see [`PiplProperty::value`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PiplValue {
	/// A name, category, match name, URL or entry point.
	Text(String),
	/// A four-character code (`kind`).
	FourCc(String),
	/// A `major.minor` pair (`ePVR`, `eSVR`).
	Pair(u16, u16),
	/// `eVER`.
	Version(EffectVersion),
	/// `eGLO`.
	OutFlags(OutFlags),
	/// `eGL2`.
	OutFlags2(OutFlags2),
	/// A number (`eINF`, `aeFL`, `aeRD`).
	Number(u32),
	/// Data of a key this crate does not decode.
	Bytes(Vec<u8>),
}

/// An effect's version as `PF_VERSION` packs it: in the PiPL's `eVER` and in
/// `PF_OutData::my_version` at `PF_Cmd_GLOBAL_SETUP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectVersion {
	/// The version (0 to 127).
	pub major: u32,
	/// The subversion (0 to 15).
	pub minor: u32,
	/// The bug-fix version (0 to 15).
	pub bugfix: u32,
	/// The release stage.
	pub stage: Stage,
	/// The build number (0 to 511).
	pub build: u32,
}

/// The release stage of an [`EffectVersion`] (`PF_Stage_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
	/// `PF_Stage_DEVELOP`.
	Develop,
	/// `PF_Stage_ALPHA`.
	Alpha,
	/// `PF_Stage_BETA`.
	Beta,
	/// `PF_Stage_RELEASE`.
	Release,
}

impl EffectVersion {
	/// Unpack a `PF_VERSION` value.
	pub fn from_bits(bits: u32) -> Self {
		let low = (bits >> 19) & 0x7;
		let high = (bits >> 26) & 0xf;
		Self {
			major: (high << 3) | low,
			minor: (bits >> 15) & 0xf,
			bugfix: (bits >> 11) & 0xf,
			stage: match (bits >> 9) & 0x3 {
				0 => Stage::Develop,
				1 => Stage::Alpha,
				2 => Stage::Beta,
				_ => Stage::Release,
			},
			build: bits & 0x1ff,
		}
	}

	/// Pack it the way `PF_VERSION` does.
	pub fn bits(&self) -> u32 {
		let stage = match self.stage {
			Stage::Develop => 0,
			Stage::Alpha => 1,
			Stage::Beta => 2,
			Stage::Release => 3,
		};
		((self.major >> 3) & 0xf) << 26
			| (self.major & 0x7) << 19
			| (self.minor & 0xf) << 15
			| (self.bugfix & 0xf) << 11
			| stage << 9
			| (self.build & 0x1ff)
	}
}

impl fmt::Display for EffectVersion {
	/// `5.6.0`, then the stage unless it is a release, then the build:
	/// `5.6.0 develop build 1`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}.{}.{}", self.major, self.minor, self.bugfix)?;
		let stage = match self.stage {
			Stage::Develop => " develop",
			Stage::Alpha => " alpha",
			Stage::Beta => " beta",
			Stage::Release => "",
		};
		write!(f, "{stage} build {}", self.build)
	}
}

impl fmt::Display for PiplValue {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			PiplValue::Text(text) => write!(f, "{text:?}"),
			PiplValue::FourCc(code) => f.write_str(code),
			PiplValue::Pair(major, minor) => write!(f, "{major}.{minor}"),
			PiplValue::Version(version) => write!(f, "{version}"),
			PiplValue::OutFlags(flags) => write!(f, "{flags:?}"),
			PiplValue::OutFlags2(flags) => write!(f, "{flags:?}"),
			PiplValue::Number(value) => write!(f, "{value:#x}"),
			PiplValue::Bytes(bytes) => {
				for (i, byte) in bytes.iter().enumerate() {
					if i > 0 {
						f.write_str(" ")?;
					}
					write!(f, "{byte:02x}")?;
				}
				Ok(())
			}
		}
	}
}

impl PiplProperty {
	/// The vendor code, `8BIM` for every property After Effects reads.
	pub fn vendor(&self) -> &str {
		&self.vendor
	}

	/// The key (`name`, `eGLO`, `ma64`, ...).
	pub fn key(&self) -> &str {
		&self.key
	}

	/// The property id (0 for every property After Effects reads).
	pub fn id(&self) -> u32 {
		self.id
	}

	/// The raw data, in the PiPL's byte order.
	pub fn data(&self) -> &[u8] {
		&self.data
	}

	/// The data decoded by what the key means.
	pub fn value(&self) -> PiplValue {
		let decoded = match self.key.as_str() {
			"name" | "catg" | "eMNA" | "eURL" | "mi32" | "mi64" | "ma64" | "mach" | "pwpc" | "ppcb" => {
				self.pascal_string().map(PiplValue::Text)
			}
			"8664" | "wx86" => self.c_string().map(PiplValue::Text),
			"kind" => self.u32().map(|v| PiplValue::FourCc(fourcc_text(v))),
			"ePVR" | "eSVR" => self.pair().map(|(a, b)| PiplValue::Pair(a, b)),
			"eVER" => self.u32().map(|v| PiplValue::Version(EffectVersion::from_bits(v))),
			"eGLO" => self.u32().map(|v| PiplValue::OutFlags(OutFlags::from_bits(v))),
			"eGL2" => self.u32().map(|v| PiplValue::OutFlags2(OutFlags2::from_bits(v))),
			"eINF" | "aeFL" | "aeRD" => self.u32().or_else(|| self.u16().map(u32::from)).map(PiplValue::Number),
			_ => None,
		};
		decoded.unwrap_or_else(|| PiplValue::Bytes(self.data.clone()))
	}

	fn u32(&self) -> Option<u32> {
		let bytes: [u8; 4] = self.data.get(..4)?.try_into().ok()?;
		Some(if self.big_endian {
			u32::from_be_bytes(bytes)
		} else {
			u32::from_le_bytes(bytes)
		})
	}

	fn u16(&self) -> Option<u16> {
		let bytes: [u8; 2] = self.data.get(..2)?.try_into().ok()?;
		Some(if self.big_endian {
			u16::from_be_bytes(bytes)
		} else {
			u16::from_le_bytes(bytes)
		})
	}

	fn pair(&self) -> Option<(u16, u16)> {
		let read = |at: usize| -> Option<u16> {
			let bytes: [u8; 2] = self.data.get(at..at + 2)?.try_into().ok()?;
			Some(if self.big_endian {
				u16::from_be_bytes(bytes)
			} else {
				u16::from_le_bytes(bytes)
			})
		};
		Some((read(0)?, read(2)?))
	}

	fn pascal_string(&self) -> Option<String> {
		let len = *self.data.first()? as usize;
		text(self.data.get(1..1 + len)?)
	}

	fn c_string(&self) -> Option<String> {
		let end = self.data.iter().position(|&b| b == 0).unwrap_or(self.data.len());
		text(&self.data[..end])
	}
}

fn text(bytes: &[u8]) -> Option<String> {
	std::str::from_utf8(bytes)
		.ok()
		.map(|s| s.trim_end_matches('\0').to_string())
}

impl Pipl {
	/// Find and read the PiPL of the plugin at `path`: a `.plugin` bundle, a
	/// `.aex`/`.dll`, a bundle's binary, or a `.rsrc` file.
	///
	/// A bundle's PiPL is the `PiPL` resource in a `.rsrc` file in its
	/// `Contents/Resources/` (or at its top level), else its binary's
	/// `__TEXT,__pipl` section, else the `PiPL` resource in its binary's
	/// resource fork. Beside a binary outside a bundle, only a `.rsrc` named
	/// after it counts.
	///
	/// # Errors
	/// [`AexloError::PluginNotFound`] if nothing is at `path`, and
	/// [`AexloError::InvalidPipl`] if a file cannot be read or its PiPL is
	/// malformed. `Ok(None)` means the plugin has no PiPL to find.
	pub fn read(path: impl AsRef<Path>) -> Result<Option<Pipl>> {
		let path = path.as_ref();
		if !path.exists() {
			return Err(AexloError::PluginNotFound {
				path: path.to_path_buf(),
			});
		}
		if path.is_dir() {
			return read_bundle(path);
		}
		if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("rsrc")) {
			return read_rsrc_file(path);
		}
		let bytes = read_file(path)?;
		if bytes.starts_with(b"MZ") {
			return pe_pipl_resource(&bytes)
				.map(|resource| parse(path, resource, false))
				.transpose();
		}
		// A bundle's binary, `<bundle>/Contents/MacOS/<binary>` or at the top
		// of a bundle without `Contents/`: the bundle's `.rsrc` files first.
		let parent = path.parent().unwrap_or(Path::new("."));
		let stem = path.file_stem().and_then(|s| s.to_str());
		let in_contents = parent.ends_with(Path::new("Contents").join("MacOS"));
		if in_contents
			&& let Some(contents) = parent.parent()
			&& let Some(pipl) = read_rsrc_dir(&contents.join("Resources"), stem, false)?
		{
			return Ok(Some(pipl));
		}
		let in_bundle = parent.extension().is_some_and(|e| e.eq_ignore_ascii_case("plugin"));
		if in_bundle && let Some(pipl) = read_rsrc_dir(parent, stem, false)? {
			return Ok(Some(pipl));
		}
		if let Some(pipl) = read_mach_o(path, &bytes)? {
			return Ok(Some(pipl));
		}
		if in_contents || in_bundle {
			return Ok(None);
		}
		read_rsrc_dir(parent, stem, true)
	}

	/// Where the PiPL was read from: the binary (its PE resource or Mach-O
	/// `__TEXT,__pipl` section), a `.rsrc` file, or a resource fork.
	pub fn source(&self) -> &Path {
		&self.source
	}

	/// The PiPL format version from its header (0).
	pub fn format_version(&self) -> u32 {
		self.version
	}

	/// Every property, in the PiPL's order.
	pub fn properties(&self) -> &[PiplProperty] {
		&self.properties
	}

	/// The first property with this key.
	pub fn get(&self, key: &str) -> Option<&PiplProperty> {
		self.properties.iter().find(|p| p.key == key)
	}

	fn text(&self, key: &str) -> Option<String> {
		match self.get(key)?.value() {
			PiplValue::Text(text) => Some(text),
			_ => None,
		}
	}

	/// `kind`: `eFKT` for an effect.
	pub fn kind(&self) -> Option<String> {
		match self.get("kind")?.value() {
			PiplValue::FourCc(code) => Some(code),
			_ => None,
		}
	}

	/// `name`: the name in the Effect menu.
	pub fn name(&self) -> Option<String> {
		self.text("name")
	}

	/// `catg`: the Effect menu category.
	pub fn category(&self) -> Option<String> {
		self.text("catg")
	}

	/// `eMNA`: the match name After Effects identifies the effect by.
	pub fn match_name(&self) -> Option<String> {
		self.text("eMNA")
	}

	/// The entry-point symbol declared for this process's platform (`8664`,
	/// `ma64` or `mi64`).
	pub fn entry_point(&self) -> Option<String> {
		self.text(platform_entry_key()?)
			.filter(|name| !name.is_empty() && name.len() <= 128 && name.bytes().all(|b| b.is_ascii_graphic()))
	}

	/// `eVER`: the effect's version, which After Effects expects
	/// `PF_Cmd_GLOBAL_SETUP` to report too.
	pub fn effect_version(&self) -> Option<EffectVersion> {
		match self.get("eVER")?.value() {
			PiplValue::Version(version) => Some(version),
			_ => None,
		}
	}

	/// `eGLO`: the `out_flags` After Effects expects `PF_Cmd_GLOBAL_SETUP`
	/// to declare.
	pub fn out_flags(&self) -> Option<OutFlags> {
		match self.get("eGLO")?.value() {
			PiplValue::OutFlags(flags) => Some(flags),
			_ => None,
		}
	}

	/// `eGL2`: the `out_flags2` After Effects expects `PF_Cmd_GLOBAL_SETUP`
	/// to declare.
	pub fn out_flags2(&self) -> Option<OutFlags2> {
		match self.get("eGL2")?.value() {
			PiplValue::OutFlags2(flags) => Some(flags),
			_ => None,
		}
	}
}

fn invalid(path: &Path, reason: impl Into<String>) -> AexloError {
	AexloError::InvalidPipl {
		path: path.to_path_buf(),
		reason: reason.into(),
	}
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
	std::fs::read(path).map_err(|e| invalid(path, e.to_string()))
}

/// A `.plugin` bundle: its `.rsrc` files, then its binary's resource fork.
fn read_bundle(bundle: &Path) -> Result<Option<Pipl>> {
	let stem = bundle.file_stem().and_then(|s| s.to_str());
	for dir in [bundle.join("Contents").join("Resources"), bundle.to_path_buf()] {
		if let Some(pipl) = read_rsrc_dir(&dir, stem, false)? {
			return Ok(Some(pipl));
		}
	}
	let mut binaries: Vec<PathBuf> = std::fs::read_dir(bundle.join("Contents").join("MacOS"))
		.into_iter()
		.flatten()
		.filter_map(|entry| entry.ok().map(|e| e.path()))
		.filter(|path| path.is_file())
		.collect();
	binaries.sort();
	// A bundle without `Contents/` keeps its binary at the top.
	if let Some(stem) = stem {
		binaries.push(bundle.join(stem));
	}
	for binary in binaries.iter().filter(|path| path.is_file()) {
		if let Some(pipl) = read_mach_o(binary, &read_file(binary)?)? {
			return Ok(Some(pipl));
		}
	}
	Ok(None)
}

/// The PiPL a Mach-O binary carries: in its `__TEXT,__pipl` section, else the
/// `PiPL` resource in its resource fork.
fn read_mach_o(binary: &Path, bytes: &[u8]) -> Result<Option<Pipl>> {
	if let Some(section) = mach_o_pipl_section(bytes) {
		// The section holds a whole resource file (what Rez writes), or
		// else the bare PiPL resource.
		return match rsrc_pipl_resource(section) {
			Ok(Some(resource)) => parse(binary, resource, true).map(Some),
			_ => parse(binary, section, true).map(Some),
		};
	}
	read_resource_fork(binary)
}

/// The first `.rsrc` in `dir` with a PiPL, preferring the one named `stem`;
/// with `named_only`, only that one.
fn read_rsrc_dir(dir: &Path, stem: Option<&str>, named_only: bool) -> Result<Option<Pipl>> {
	let Ok(entries) = std::fs::read_dir(dir) else {
		return Ok(None);
	};
	let mut files: Vec<PathBuf> = entries
		.filter_map(|entry| entry.ok().map(|e| e.path()))
		.filter(|path| path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("rsrc")))
		.collect();
	files.sort();
	let named = |path: &PathBuf| stem.is_some() && path.file_stem().and_then(|s| s.to_str()) == stem;
	files.sort_by_key(|path| !named(path));
	for file in files {
		if named_only && !named(&file) {
			continue;
		}
		if let Some(pipl) = read_rsrc_file(&file)? {
			return Ok(Some(pipl));
		}
	}
	Ok(None)
}

fn read_rsrc_file(path: &Path) -> Result<Option<Pipl>> {
	let bytes = read_file(path)?;
	match rsrc_pipl_resource(&bytes).map_err(|reason| invalid(path, reason))? {
		Some(resource) => parse(path, resource, true).map(Some),
		None => Ok(None),
	}
}

/// The PiPL in `binary`'s resource fork, where older macOS plugins keep it.
fn read_resource_fork(binary: &Path) -> Result<Option<Pipl>> {
	if !cfg!(target_os = "macos") {
		return Ok(None);
	}
	let fork = binary.join("..namedfork").join("rsrc");
	match std::fs::read(&fork) {
		Ok(bytes) if !bytes.is_empty() => match rsrc_pipl_resource(&bytes).map_err(|reason| invalid(&fork, reason))? {
			Some(resource) => parse(&fork, resource, true).map(Some),
			None => Ok(None),
		},
		_ => Ok(None),
	}
}

/// The `__TEXT,__pipl` section of a Mach-O binary, in the slice for this
/// process's architecture when the binary is universal (else its first).
fn mach_o_pipl_section(binary: &[u8]) -> Option<&[u8]> {
	let be32 = |at: usize| Some(u32::from_be_bytes(binary.get(at..at + 4)?.try_into().ok()?));
	match be32(0)? {
		// FAT_MAGIC: big-endian `fat_arch`s of 20 bytes after the count.
		0xcafe_babe => {
			let count = be32(4)? as usize;
			if count == 0 || count > 32 {
				return None;
			}
			let slices: Vec<(u32, usize, usize)> = (0..count)
				.filter_map(|index| {
					let at = 8 + index * 20;
					Some((be32(at)?, be32(at + 8)? as usize, be32(at + 12)? as usize))
				})
				.collect();
			let native = slices.iter().find(|(cpu, ..)| Some(*cpu) == native_cpu_type());
			let (_, offset, size) = native.or(slices.first())?;
			mach_o_64_section(binary.get(*offset..offset.checked_add(*size)?)?, b"__TEXT", b"__pipl")
		}
		_ => mach_o_64_section(binary, b"__TEXT", b"__pipl"),
	}
}

/// `CPU_TYPE_ARM64` or `CPU_TYPE_X86_64`, for picking a universal binary's slice.
fn native_cpu_type() -> Option<u32> {
	match std::env::consts::ARCH {
		"aarch64" => Some(0x0100_000c),
		"x86_64" => Some(0x0100_0007),
		_ => None,
	}
}

/// A section of a thin 64-bit little-endian Mach-O image.
fn mach_o_64_section<'a>(image: &'a [u8], segment: &[u8], section: &[u8]) -> Option<&'a [u8]> {
	let le32 = |at: usize| Some(u32::from_le_bytes(image.get(at..at + 4)?.try_into().ok()?));
	let le64 = |at: usize| Some(u64::from_le_bytes(image.get(at..at + 8)?.try_into().ok()?));
	if le32(0)? != 0xfeed_facf {
		return None;
	}
	let name_is = |at: usize, name: &[u8]| {
		image
			.get(at..at + 16)
			.is_some_and(|field| field.split(|&b| b == 0).next() == Some(name))
	};
	let commands = le32(16)? as usize;
	let mut at = 32;
	for _ in 0..commands.min(4096) {
		let (cmd, size) = (le32(at)?, le32(at + 4)? as usize);
		// LC_SEGMENT_64: 72 bytes, then `nsects` sections of 80 bytes.
		if cmd == 0x19 && name_is(at + 8, segment) {
			let sections = le32(at + 64)? as usize;
			for index in 0..sections.min(256) {
				let header = at + 72 + index * 80;
				if name_is(header, section) {
					let len = usize::try_from(le64(header + 40)?).ok()?;
					let offset = le32(header + 48)? as usize;
					return image.get(offset..offset.checked_add(len)?);
				}
			}
		}
		if size == 0 {
			return None;
		}
		at = at.checked_add(size)?;
	}
	None
}

/// The `PiPL` resource of a classic Mac resource file: id 16000, else the
/// first one.
fn rsrc_pipl_resource(file: &[u8]) -> std::result::Result<Option<&[u8]>, String> {
	let be32 = |at: usize| -> std::result::Result<usize, String> {
		file.get(at..at + 4)
			.map(|b| u32::from_be_bytes(b.try_into().unwrap()) as usize)
			.ok_or_else(|| format!("resource file truncated at {at}"))
	};
	let be16 = |at: usize| -> std::result::Result<usize, String> {
		file.get(at..at + 2)
			.map(|b| u16::from_be_bytes(b.try_into().unwrap()) as usize)
			.ok_or_else(|| format!("resource file truncated at {at}"))
	};
	let data = be32(0)?;
	let map = be32(4)?;
	let types = map + be16(map + 24)?;
	let type_count = (be16(types)? + 1) & 0xffff;
	for index in 0..type_count {
		let entry = types + 2 + index * 8;
		if file.get(entry..entry + 4) != Some(b"PiPL") {
			continue;
		}
		let count = be16(entry + 4)? + 1;
		let refs = types + be16(entry + 6)?;
		let mut chosen = None;
		for index in 0..count {
			let at = refs + index * 12;
			let id = be16(at)? as u16 as i16;
			let offset = be32(at + 4)? & 0x00ff_ffff;
			if chosen.is_none() || id == PIPL_ID {
				chosen = Some(offset);
			}
			if id == PIPL_ID {
				break;
			}
		}
		let Some(offset) = chosen else { return Ok(None) };
		let at = data + offset;
		let len = be32(at)?;
		return file
			.get(at + 4..at + 4 + len)
			.map(Some)
			.ok_or_else(|| "PiPL resource runs past the end of the file".to_string());
	}
	Ok(None)
}

/// Parse a PiPL resource: little-endian with a 2-byte prefix on Windows,
/// big-endian on macOS.
fn parse(source: &Path, bytes: &[u8], big_endian: bool) -> Result<Pipl> {
	let read = |at: usize| -> Result<u32> {
		let raw: [u8; 4] = bytes
			.get(at..at + 4)
			.and_then(|b| b.try_into().ok())
			.ok_or_else(|| invalid(source, format!("PiPL truncated at byte {at}")))?;
		Ok(if big_endian {
			u32::from_be_bytes(raw)
		} else {
			u32::from_le_bytes(raw)
		})
	};
	let header = if big_endian { 0 } else { 2 };
	let version = read(header)?;
	let count = read(header + 4)? as usize;
	let mut offset = header + 8;
	let mut properties = Vec::with_capacity(count.min(256));
	for index in 0..count {
		let vendor = fourcc_text(read(offset)?);
		let key = fourcc_text(read(offset + 4)?);
		let id = read(offset + 8)?;
		let length = read(offset + 12)? as usize;
		offset += 16;
		let data = bytes
			.get(offset..offset + length)
			.ok_or_else(|| invalid(source, format!("PiPL property {index} ({key}) runs past the end")))?;
		offset += length + (4 - length % 4) % 4;
		properties.push(PiplProperty {
			vendor,
			key,
			id,
			data: data.to_vec(),
			big_endian,
		});
	}
	Ok(Pipl {
		source: source.to_path_buf(),
		version,
		properties,
	})
}

/// A four-character code read as a number in the PiPL's byte order.
fn fourcc_text(value: u32) -> String {
	let bytes = value.to_be_bytes();
	if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
		bytes.iter().map(|&b| b as char).collect()
	} else {
		format!("{value:#010x}")
	}
}

fn platform_entry_key() -> Option<&'static str> {
	#[cfg(all(windows, target_arch = "x86_64"))]
	{
		Some("8664")
	}
	#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
	{
		Some("ma64")
	}
	#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
	{
		Some("mi64")
	}
	#[cfg(not(any(
		all(windows, target_arch = "x86_64"),
		all(target_os = "macos", target_arch = "aarch64"),
		all(target_os = "macos", target_arch = "x86_64"),
	)))]
	{
		None
	}
}

/// Resource type `PIPL`, id 16000, from a PE image. Absent from Mach-O binaries.
fn pe_pipl_resource(binary: &[u8]) -> Option<&[u8]> {
	let pe = u32::from_le_bytes(binary.get(0x3c..0x40)?.try_into().ok()?) as usize;
	if binary.get(pe..pe + 4) != Some(b"PE\0\0") {
		return None;
	}
	let sections = u16::from_le_bytes(binary.get(pe + 6..pe + 8)?.try_into().ok()?) as usize;
	if sections == 0 || sections > 96 {
		return None;
	}
	let optional = u16::from_le_bytes(binary.get(pe + 20..pe + 22)?.try_into().ok()?) as usize;
	let opt = pe + 24;
	if binary.get(opt..opt + 2) != Some(&0x020bu16.to_le_bytes()) || optional < 128 + 24 {
		return None;
	}
	let dirs = u32::from_le_bytes(binary.get(opt + 108..opt + 112)?.try_into().ok()?) as usize;
	if dirs <= 2 {
		return None;
	}
	let resource_rva = u32::from_le_bytes(binary.get(opt + 128..opt + 132)?.try_into().ok()?);
	let section_off = opt + optional;
	let sections = read_sections(binary, section_off, sections)?;
	rva_to_offset(&sections, resource_rva)?;
	let type_dir = find_entry(binary, &sections, resource_rva, 0, EntryKey::Name("PIPL"))?;
	let id_dir = find_entry(binary, &sections, resource_rva, type_dir, EntryKey::Id(16000))?;
	let data = find_entry(binary, &sections, resource_rva, id_dir, EntryKey::First)?;
	let data_off = rva_to_offset(&sections, resource_rva.checked_add(data)?)?;
	let leaf_rva = u32::from_le_bytes(binary.get(data_off..data_off + 4)?.try_into().ok()?);
	let size = u32::from_le_bytes(binary.get(data_off + 4..data_off + 8)?.try_into().ok()?) as usize;
	let file_off = rva_to_offset(&sections, leaf_rva)?;
	binary.get(file_off..file_off + size)
}

struct Section {
	va: u32,
	raw: u32,
	raw_size: u32,
	span: u32,
}

enum EntryKey {
	Name(&'static str),
	Id(u32),
	First,
}

fn read_sections(binary: &[u8], offset: usize, count: usize) -> Option<Vec<Section>> {
	let mut sections = Vec::with_capacity(count);
	for index in 0..count {
		let at = offset + index * 40;
		let virt = u32::from_le_bytes(binary.get(at + 8..at + 12)?.try_into().ok()?);
		let va = u32::from_le_bytes(binary.get(at + 12..at + 16)?.try_into().ok()?);
		let raw_size = u32::from_le_bytes(binary.get(at + 16..at + 20)?.try_into().ok()?);
		let raw = u32::from_le_bytes(binary.get(at + 20..at + 24)?.try_into().ok()?);
		sections.push(Section {
			va,
			raw,
			raw_size,
			span: virt.max(raw_size),
		});
	}
	Some(sections)
}

fn rva_to_offset(sections: &[Section], rva: u32) -> Option<usize> {
	let section = sections
		.iter()
		.find(|s| rva >= s.va && rva.wrapping_sub(s.va) < s.span)?;
	let delta = rva - section.va;
	if delta >= section.raw_size {
		return None;
	}
	Some((section.raw + delta) as usize)
}

/// Returns the next directory's offset from the resource root, or the data-entry offset.
fn find_entry(binary: &[u8], sections: &[Section], root_rva: u32, dir: u32, want: EntryKey) -> Option<u32> {
	let at = rva_to_offset(sections, root_rva.checked_add(dir)?)?;
	let named = u16::from_le_bytes(binary.get(at + 12..at + 14)?.try_into().ok()?) as usize;
	let ids = u16::from_le_bytes(binary.get(at + 14..at + 16)?.try_into().ok()?) as usize;
	let mut cursor = at + 16;
	for index in 0..named + ids {
		let name = u32::from_le_bytes(binary.get(cursor..cursor + 4)?.try_into().ok()?);
		let offset = u32::from_le_bytes(binary.get(cursor + 4..cursor + 8)?.try_into().ok()?);
		cursor += 8;
		let matched = match want {
			EntryKey::First => index == 0,
			EntryKey::Id(id) => name == id,
			EntryKey::Name(expected) => {
				name & 0x8000_0000 != 0 && resource_name(binary, sections, root_rva, name & 0x7fff_ffff)? == expected
			}
		};
		if matched {
			return Some(offset & 0x7fff_ffff);
		}
	}
	None
}

fn resource_name(binary: &[u8], sections: &[Section], root_rva: u32, offset: u32) -> Option<String> {
	let at = rva_to_offset(sections, root_rva.checked_add(offset)?)?;
	let len = u16::from_le_bytes(binary.get(at..at + 2)?.try_into().ok()?) as usize;
	let raw = binary.get(at + 2..at + 2 + len * 2)?;
	let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
	Some(String::from_utf16_lossy(&units).to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn windows_pipl_is_little_endian_after_a_two_byte_prefix() {
		let blob = pipl_le(&[
			("8664", b"main\0".to_vec()),
			("eVER", 0x002b_0001u32.to_le_bytes().to_vec()),
		]);
		let pipl = parse(Path::new("x.aex"), &blob, false).unwrap();
		assert_eq!(pipl.properties().len(), 2);
		assert_eq!(pipl.get("8664").unwrap().value(), PiplValue::Text("main".into()));
		assert_eq!(pipl.effect_version().unwrap().to_string(), "5.6.0 develop build 1");
	}

	#[test]
	fn mac_rsrc_pipl_is_big_endian() {
		let pipl = pipl_be(&[
			("kind", b"eFKT".to_vec()),
			("name", pascal("SDK_Noise")),
			("ma64", pascal("EffectMain")),
			("eGLO", 0x0200_0404u32.to_be_bytes().to_vec()),
			("ePVR", vec![0, 2, 0, 0]),
		]);
		let file = rsrc(&pipl);
		let resource = rsrc_pipl_resource(&file).unwrap().unwrap();
		let pipl = parse(Path::new("x.rsrc"), resource, true).unwrap();
		assert_eq!(pipl.kind().as_deref(), Some("eFKT"));
		assert_eq!(pipl.name().as_deref(), Some("SDK_Noise"));
		assert_eq!(pipl.get("ePVR").unwrap().value(), PiplValue::Pair(2, 0));
		assert_eq!(pipl.out_flags().unwrap(), OutFlags::from_bits(0x0200_0404));
		if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
			assert_eq!(pipl.entry_point().as_deref(), Some("EffectMain"));
		}
	}

	#[test]
	fn a_bundle_finds_its_rsrc_by_any_name() {
		let dir = std::env::temp_dir().join(format!("aexlo-pipl-{}", std::process::id()));
		let resources = dir.join("Odd.plugin").join("Contents").join("Resources");
		std::fs::create_dir_all(&resources).unwrap();
		std::fs::write(
			resources.join("OddPiPL.rsrc"),
			rsrc(&pipl_be(&[("name", pascal("Odd"))])),
		)
		.unwrap();
		let pipl = Pipl::read(dir.join("Odd.plugin")).unwrap().unwrap();
		assert_eq!(pipl.name().as_deref(), Some("Odd"));
		assert!(pipl.source().ends_with("OddPiPL.rsrc"));
		// Beside a binary outside a bundle, only a `.rsrc` of its name counts.
		let flat = dir.join("flat");
		std::fs::create_dir_all(&flat).unwrap();
		std::fs::write(flat.join("libother.dylib"), b"\xcf\xfa\xed\xfe").unwrap();
		std::fs::write(flat.join("Other.rsrc"), rsrc(&pipl_be(&[("name", pascal("Other"))]))).unwrap();
		assert_eq!(Pipl::read(flat.join("libother.dylib")).unwrap(), None);
		let _ = std::fs::remove_dir_all(&dir);
	}

	#[test]
	fn a_truncated_pipl_is_an_error() {
		let mut blob = pipl_be(&[("name", pascal("Cut"))]);
		blob.truncate(blob.len() - 6);
		assert!(matches!(
			parse(Path::new("x.rsrc"), &blob, true),
			Err(AexloError::InvalidPipl { .. })
		));
	}

	#[test]
	fn effect_versions_round_trip() {
		for bits in [0x002b_0001, 0x0408_0e00 | 0x3 << 9, 0] {
			assert_eq!(EffectVersion::from_bits(bits).bits(), bits);
		}
		let version = EffectVersion::from_bits(0x002b_0001);
		assert_eq!((version.major, version.minor, version.stage), (5, 6, Stage::Develop));
	}

	#[test]
	fn mach_o_pipl_section_is_read_from_thin_and_universal_binaries() {
		let pipl = pipl_be(&[("name", pascal("Sectioned"))]);
		let thin = mach_o_with_pipl(&rsrc(&pipl));
		let mut fat = 0xcafe_babeu32.to_be_bytes().to_vec();
		fat.extend_from_slice(&1u32.to_be_bytes());
		fat.extend_from_slice(&native_cpu_type().unwrap_or(0).to_be_bytes());
		fat.extend_from_slice(&0u32.to_be_bytes());
		fat.extend_from_slice(&64u32.to_be_bytes());
		fat.extend_from_slice(&(thin.len() as u32).to_be_bytes());
		fat.extend_from_slice(&0u32.to_be_bytes());
		fat.resize(64, 0);
		fat.extend_from_slice(&thin);
		let bare = mach_o_with_pipl(&pipl);
		for binary in [&thin, &fat, &bare] {
			let parsed = read_mach_o(Path::new("x"), binary).unwrap().unwrap();
			assert_eq!(parsed.name().as_deref(), Some("Sectioned"));
		}
		assert!(mach_o_pipl_section(&thin[..40]).is_none());
	}

	/// A thin Mach-O with one `__TEXT` segment holding `section` as `__pipl`.
	fn mach_o_with_pipl(pipl: &[u8]) -> Vec<u8> {
		let commands = 72 + 80;
		let data_at = 32 + commands;
		let mut image = Vec::new();
		image.extend_from_slice(&0xfeed_facfu32.to_le_bytes());
		image.extend_from_slice(&[0; 12]);
		image.extend_from_slice(&1u32.to_le_bytes());
		image.extend_from_slice(&(commands as u32).to_le_bytes());
		image.extend_from_slice(&[0; 8]);
		let name = |text: &[u8]| {
			let mut field = text.to_vec();
			field.resize(16, 0);
			field
		};
		image.extend_from_slice(&0x19u32.to_le_bytes());
		image.extend_from_slice(&(commands as u32).to_le_bytes());
		image.extend_from_slice(&name(b"__TEXT"));
		image.extend_from_slice(&[0; 32]);
		image.extend_from_slice(&[0; 8]);
		image.extend_from_slice(&1u32.to_le_bytes());
		image.extend_from_slice(&0u32.to_le_bytes());
		image.extend_from_slice(&name(b"__pipl"));
		image.extend_from_slice(&name(b"__TEXT"));
		image.extend_from_slice(&0u64.to_le_bytes());
		image.extend_from_slice(&(pipl.len() as u64).to_le_bytes());
		image.extend_from_slice(&(data_at as u32).to_le_bytes());
		image.extend_from_slice(&[0; 28]);
		image.extend_from_slice(pipl);
		image
	}

	#[test]
	fn pe_resource_yields_the_pipl() {
		let pe = pe_with_pipl(&pipl_le(&[("8664", b"main\0".to_vec())]));
		let resource = pe_pipl_resource(&pe).unwrap();
		let pipl = parse(Path::new("x.aex"), resource, false).unwrap();
		assert_eq!(pipl.get("8664").unwrap().value(), PiplValue::Text("main".into()));
		assert!(pe_pipl_resource(b"not a plugin").is_none());
	}

	fn pascal(text: &str) -> Vec<u8> {
		let mut bytes = vec![text.len() as u8];
		bytes.extend_from_slice(text.as_bytes());
		bytes
	}

	fn pipl_le(properties: &[(&str, Vec<u8>)]) -> Vec<u8> {
		let mut bytes = vec![1, 0];
		bytes.extend_from_slice(&0u32.to_le_bytes());
		bytes.extend_from_slice(&(properties.len() as u32).to_le_bytes());
		for (key, payload) in properties {
			bytes.extend_from_slice(&fourcc_le("8BIM"));
			bytes.extend_from_slice(&fourcc_le(key));
			bytes.extend_from_slice(&0u32.to_le_bytes());
			bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
			bytes.extend_from_slice(payload);
			while bytes.len() % 4 != 2 {
				bytes.push(0);
			}
		}
		bytes
	}

	fn pipl_be(properties: &[(&str, Vec<u8>)]) -> Vec<u8> {
		let mut bytes = 0u32.to_be_bytes().to_vec();
		bytes.extend_from_slice(&(properties.len() as u32).to_be_bytes());
		for (key, payload) in properties {
			bytes.extend_from_slice(b"8BIM");
			bytes.extend_from_slice(key.as_bytes());
			bytes.extend_from_slice(&0u32.to_be_bytes());
			bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
			bytes.extend_from_slice(payload);
			while !bytes.len().is_multiple_of(4) {
				bytes.push(0);
			}
		}
		bytes
	}

	/// A classic resource file holding `pipl` as `PiPL` 16000.
	fn rsrc(pipl: &[u8]) -> Vec<u8> {
		let data_off = 256u32;
		let data_len = 4 + pipl.len() as u32;
		let map_off = data_off + data_len;
		let mut file = vec![0u8; 256];
		file.extend_from_slice(&(pipl.len() as u32).to_be_bytes());
		file.extend_from_slice(pipl);
		// map: header copy (16), next map (4), file ref (2), attributes (2),
		// type list offset (2), name list offset (2), then the type list.
		let mut map = vec![0u8; 28];
		map[24..26].copy_from_slice(&28u16.to_be_bytes());
		map[26..28].copy_from_slice(&50u16.to_be_bytes());
		map.extend_from_slice(&0u16.to_be_bytes()); // one type
		map.extend_from_slice(b"PiPL");
		map.extend_from_slice(&0u16.to_be_bytes()); // one resource
		map.extend_from_slice(&10u16.to_be_bytes()); // its references, from the type list
		map.extend_from_slice(&16000i16.to_be_bytes());
		map.extend_from_slice(&0xffffu16.to_be_bytes());
		map.extend_from_slice(&0u32.to_be_bytes()); // attributes 0, data offset 0
		map.extend_from_slice(&0u32.to_be_bytes());
		let map_len = map.len() as u32;
		file[0..4].copy_from_slice(&data_off.to_be_bytes());
		file[4..8].copy_from_slice(&map_off.to_be_bytes());
		file[8..12].copy_from_slice(&data_len.to_be_bytes());
		file[12..16].copy_from_slice(&map_len.to_be_bytes());
		file.extend_from_slice(&map);
		file
	}

	fn fourcc_le(text: &str) -> [u8; 4] {
		u32::from_be_bytes(text.as_bytes().try_into().unwrap()).to_le_bytes()
	}

	fn pe_with_pipl(pipl: &[u8]) -> Vec<u8> {
		let resource = resource_tree(pipl);
		let mut file = vec![0u8; 0x200];
		file[0] = b'M';
		file[1] = b'Z';
		file[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
		file[0x40..0x44].copy_from_slice(b"PE\0\0");
		file[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());
		file[0x54..0x56].copy_from_slice(&240u16.to_le_bytes());
		file[0x58..0x5a].copy_from_slice(&0x020bu16.to_le_bytes());
		file[0x58 + 108..0x58 + 112].copy_from_slice(&16u32.to_le_bytes());
		file[0x58 + 128..0x58 + 132].copy_from_slice(&0x1000u32.to_le_bytes());
		file[0x58 + 132..0x58 + 136].copy_from_slice(&(resource.len() as u32).to_le_bytes());
		let section = 0x40 + 24 + 240;
		file[section + 8..section + 12].copy_from_slice(&(resource.len() as u32).to_le_bytes());
		file[section + 12..section + 16].copy_from_slice(&0x1000u32.to_le_bytes());
		file[section + 16..section + 20].copy_from_slice(&(resource.len() as u32).to_le_bytes());
		file[section + 20..section + 24].copy_from_slice(&0x200u32.to_le_bytes());
		file.extend_from_slice(&resource);
		file
	}

	fn resource_tree(pipl: &[u8]) -> Vec<u8> {
		// type directory, one named entry, then the UTF-16 name, then the id
		// directory, the language directory, the data entry, and the PiPL.
		let mut name = vec![4, 0];
		for unit in "PIPL".encode_utf16() {
			name.extend_from_slice(&unit.to_le_bytes());
		}
		while !name.len().is_multiple_of(4) {
			name.push(0);
		}
		let name_at = 24u32;
		let id_at = name_at + name.len() as u32;
		let lang_at = id_at + 24;
		let data_at = lang_at + 24;
		let pipl_at = data_at + 16;

		let mut blob = vec![0u8; 16];
		blob[12..14].copy_from_slice(&1u16.to_le_bytes());
		blob.extend_from_slice(&(0x8000_0000 | name_at).to_le_bytes());
		blob.extend_from_slice(&(0x8000_0000 | id_at).to_le_bytes());
		blob.extend_from_slice(&name);

		let mut id_dir = vec![0u8; 16];
		id_dir[14..16].copy_from_slice(&1u16.to_le_bytes());
		id_dir.extend_from_slice(&16000u32.to_le_bytes());
		id_dir.extend_from_slice(&(0x8000_0000 | lang_at).to_le_bytes());
		blob.extend_from_slice(&id_dir);

		let mut lang = vec![0u8; 16];
		lang[14..16].copy_from_slice(&1u16.to_le_bytes());
		lang.extend_from_slice(&0x409u32.to_le_bytes());
		lang.extend_from_slice(&data_at.to_le_bytes());
		blob.extend_from_slice(&lang);

		blob.extend_from_slice(&(0x1000 + pipl_at).to_le_bytes());
		blob.extend_from_slice(&(pipl.len() as u32).to_le_bytes());
		blob.extend_from_slice(&0u32.to_le_bytes());
		blob.extend_from_slice(&0u32.to_le_bytes());
		blob.extend_from_slice(pipl);
		blob
	}
}
