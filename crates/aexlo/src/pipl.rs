//! The PiPL resource After Effects reads when a plugin does not export
//! `PluginDataEntryFunction2`.
//!
//! The Win64 entry-point property (`8664`) is a NUL-terminated symbol name.
//! The macOS ones (`mi64`, `ma64`) are Pascal strings. Plugins such as Magic
//! Bullet Looks export that symbol as `main` rather than `EffectMain`.

/// Entry-point symbol declared for this process's platform, if the binary has one.
pub(crate) fn entry_point_name(binary: &[u8]) -> Option<String> {
	let key = platform_entry_key()?;
	let resource = pe_pipl_resource(binary)?;
	entry_point_in_pipl(resource, key)
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

fn entry_point_in_pipl(bytes: &[u8], key: &str) -> Option<String> {
	if bytes.len() < 10 {
		return None;
	}
	let count = u32::from_le_bytes(bytes[6..10].try_into().ok()?) as usize;
	let mut offset = 10;
	for _ in 0..count {
		if bytes.len() < offset + 16 {
			return None;
		}
		let property = fourcc(&bytes[offset + 4..offset + 8]);
		let length = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().ok()?) as usize;
		offset += 16;
		if bytes.len() < offset + length {
			return None;
		}
		let payload = &bytes[offset..offset + length];
		offset += length + (4 - (length % 4)) % 4;
		if property == key {
			return decode_entry(key, payload);
		}
	}
	None
}

fn decode_entry(key: &str, payload: &[u8]) -> Option<String> {
	let bytes = if key == "8664" {
		let end = payload.iter().position(|&b| b == 0).unwrap_or(payload.len());
		&payload[..end]
	} else {
		let len = (*payload.first()?) as usize;
		payload.get(1..1 + len)?
	};
	let name = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
	if name.is_empty() || name.len() > 128 || !name.bytes().all(|b| b.is_ascii_graphic()) {
		return None;
	}
	Some(name.to_string())
}

fn fourcc(bytes: &[u8]) -> String {
	let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
	value.to_be_bytes().iter().map(|b| *b as char).collect()
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
	fn pipl_reads_c_string_and_pascal_entry_points() {
		let win = pipl_blob("8664", b"main\0");
		assert_eq!(entry_point_in_pipl(&win, "8664").as_deref(), Some("main"));
		let mac = pipl_blob("ma64", &[4, b'm', b'a', b'i', b'n']);
		assert_eq!(entry_point_in_pipl(&mac, "ma64").as_deref(), Some("main"));
		assert!(entry_point_in_pipl(&win, "ma64").is_none());
	}

	#[test]
	fn pe_resource_yields_the_platform_entry_point() {
		let Some(key) = platform_entry_key() else {
			return;
		};
		let payload: Vec<u8> = if key == "8664" {
			b"main\0".to_vec()
		} else {
			vec![4, b'm', b'a', b'i', b'n']
		};
		let pe = pe_with_pipl(&pipl_blob(key, &payload));
		assert_eq!(entry_point_name(&pe).as_deref(), Some("main"));
		assert!(entry_point_name(b"not a plugin").is_none());
	}

	fn pipl_blob(key: &str, payload: &[u8]) -> Vec<u8> {
		let mut bytes = vec![1, 0, 0, 0, 0, 0, 1, 0, 0, 0];
		bytes.extend_from_slice(&fourcc_le("8BIM"));
		bytes.extend_from_slice(&fourcc_le(key));
		bytes.extend_from_slice(&0u32.to_le_bytes());
		bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
		bytes.extend_from_slice(payload);
		while bytes.len() % 4 != 0 {
			bytes.push(0);
		}
		bytes
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
		while name.len() % 4 != 0 {
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
