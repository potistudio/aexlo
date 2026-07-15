//! Reads the PiPL resource back out of the built probe DLL and prints its
//! parsed properties — a preflight check that real After Effects will accept
//! what `playground/probe/build.rs` generated.

use std::path::Path;

use anyhow::Context;
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::System::LibraryLoader::{
	FindResourceA, LOAD_LIBRARY_AS_DATAFILE, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
};

pub fn dump(dll: &Path) -> anyhow::Result<()> {
	let bytes = read_pipl_resource(dll)?;
	println!("PiPL resource: {} bytes\n", bytes.len());
	parse(&bytes)
}

fn read_pipl_resource(dll: &Path) -> anyhow::Result<Vec<u8>> {
	let mut wide: Vec<u16> = dll
		.as_os_str()
		.to_str()
		.context("non-UTF8 path")?
		.encode_utf16()
		.collect();
	wide.push(0);

	unsafe {
		// windows-sys 0.52 models HMODULE/HRSRC/HGLOBAL as isize; 0 is null.
		let module = LoadLibraryExW(wide.as_ptr(), 0, LOAD_LIBRARY_AS_DATAFILE);
		anyhow::ensure!(module != 0, "LoadLibraryExW failed for {}", dll.display());

		let result = (|| {
			// Resource id 16000, custom type "PiPL" — what AE looks for.
			let resource = FindResourceA(module, 16000 as _, c"PiPL".as_ptr() as *const u8);
			anyhow::ensure!(resource != 0, "no PiPL resource (id 16000) found");

			let size = SizeofResource(module, resource);
			let data = LoadResource(module, resource);
			anyhow::ensure!(!data.is_null() && size > 0, "failed to load PiPL resource data");

			let ptr = LockResource(data) as *const u8;
			anyhow::ensure!(!ptr.is_null(), "failed to lock PiPL resource data");

			Ok(std::slice::from_raw_parts(ptr, size as usize).to_vec())
		})();

		FreeLibrary(module);
		result
	}
}

fn fourcc(bytes: &[u8]) -> String {
	let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
	let chars = value.to_be_bytes();
	if chars.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
		chars.iter().map(|b| *b as char).collect()
	} else {
		format!("0x{value:08X}")
	}
}

fn parse(bytes: &[u8]) -> anyhow::Result<()> {
	anyhow::ensure!(bytes.len() >= 10, "PiPL too short for header");

	let version = u16::from_le_bytes([bytes[0], bytes[1]]);
	let count = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
	println!("header: version={version} properties={count}");

	let mut offset = 10;
	for index in 0..count {
		anyhow::ensure!(bytes.len() >= offset + 16, "property {index}: truncated header");

		let vendor = fourcc(&bytes[offset..]);
		let key = fourcc(&bytes[offset + 4..]);
		let length = u32::from_le_bytes([
			bytes[offset + 12],
			bytes[offset + 13],
			bytes[offset + 14],
			bytes[offset + 15],
		]) as usize;

		offset += 16;
		anyhow::ensure!(
			bytes.len() >= offset + length,
			"property {index} ({key}): truncated payload"
		);
		let payload = &bytes[offset..offset + length];
		offset += length;

		println!(
			"  [{index:2}] {vendor}/{key} ({length:3} bytes) = {}",
			describe(&key, payload)
		);
	}

	println!("\nPiPL parses cleanly — safe to hand to After Effects.");
	Ok(())
}

fn describe(key: &str, payload: &[u8]) -> String {
	match key {
		// Pascal strings (length-prefixed).
		"name" | "catg" | "eMNA" | "eURL" => {
			let len = payload.first().copied().unwrap_or(0) as usize;
			format!(
				"\"{}\"",
				String::from_utf8_lossy(&payload[1..1 + len.min(payload.len() - 1)])
			)
		}
		// NUL-terminated entry point name.
		"8664" => {
			let end = payload.iter().position(|&b| b == 0).unwrap_or(payload.len());
			format!("\"{}\"", String::from_utf8_lossy(&payload[..end]))
		}
		"kind" => fourcc(payload),
		"ePVR" | "eSVR" => {
			let major = u16::from_le_bytes([payload[0], payload[1]]);
			let minor = u16::from_le_bytes([payload[2], payload[3]]);
			format!("{major}.{minor}")
		}
		_ if payload.len() == 4 => {
			let value = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
			format!("0x{value:08X} ({value})")
		}
		_ => format!("{payload:02X?}"),
	}
}
