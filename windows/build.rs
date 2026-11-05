//! Embeds the application icon and version information in the Windows .exe.
//! The .res file is written here directly, so no resource compiler is needed
//! when cross-compiling; lld-link accepts .res inputs.
use std::env;
use std::fs;
use std::path::PathBuf;

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()).collect()
}
fn pad4(v: &mut Vec<u8>) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
}

/// One resource entry with ordinal type and name.
fn resource(out: &mut Vec<u8>, kind: u16, name: u16, data: &[u8], flags: u16, language: u16) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(name.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // DataVersion
    out.extend(flags.to_le_bytes());
    out.extend(language.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // Version
    out.extend(0u32.to_le_bytes()); // Characteristics
    out.extend(data);
    pad4(out);
}

/// VS_VERSIONINFO-style node: length, value length, type, key, value, children.
fn node(key: &str, value: &[u8], value_length: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut v = vec![0, 0];
    v.extend(value_length.to_le_bytes());
    v.extend((text as u16).to_le_bytes());
    v.extend(utf16z(key));
    pad4(&mut v);
    v.extend(value);
    for child in children {
        pad4(&mut v);
        v.extend(child);
    }
    let len = v.len() as u16;
    v[0..2].copy_from_slice(&len.to_le_bytes());
    v
}

fn version_info(version: &str) -> Vec<u8> {
    let parts: Vec<u16> = version.split('.').map(|p| p.parse().unwrap_or(0)).chain([0, 0, 0, 0]).take(4).collect();
    let ms = (parts[0] as u32) << 16 | parts[1] as u32;
    let ls = (parts[2] as u32) << 16 | parts[3] as u32;
    let mut fixed = vec![];
    for x in [0xFEEF04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend(x.to_le_bytes());
    }
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", "Deckwright"),
        ("FileDescription", "Deckwright"),
        ("FileVersion", version),
        ("InternalName", "Deckwright"),
        ("OriginalFilename", "Deckwright.exe"),
        ("ProductName", "Deckwright"),
        ("ProductVersion", version),
    ]
    .iter()
    .map(|(k, val)| {
        let value = utf16z(val);
        node(k, &value, (value.len() / 2) as u16, true, &[])
    })
    .collect();
    // US English (0x0409), Unicode code page (1200).
    let table = node("040904B0", &[], 0, true, &strings);
    let string_info = node("StringFileInfo", &[], 0, true, &[table]);
    let translation = node("Translation", &0x04B0_0409u32.to_le_bytes(), 4, false, &[]);
    let var_info = node("VarFileInfo", &[], 0, true, &[translation]);
    node("VS_VERSION_INFO", &fixed, 52, false, &[string_info, var_info])
}

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=build.rs");
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows-msvc") {
        return;
    }
    let ico = fs::read("assets/app.ico").expect("assets/app.ico");
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut res = vec![];
    resource(&mut res, 0, 0, &[], 0, 0); // empty leading entry required by the format
    let mut group = vec![0, 0, 1, 0];
    group.extend((count as u16).to_le_bytes());
    for i in 0..count {
        let e = &ico[6 + i * 16..6 + (i + 1) * 16];
        let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let offset = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        let id = (i + 1) as u16;
        resource(&mut res, 3, id, &ico[offset..offset + size], 0x1010, 0x0409); // RT_ICON
        group.extend(&e[0..12]); // width, height, colours, reserved, planes, bit count, bytes
        group.extend(id.to_le_bytes());
    }
    resource(&mut res, 14, 1, &group, 0x1030, 0x0409); // RT_GROUP_ICON
    resource(&mut res, 16, 1, &version_info(&env::var("CARGO_PKG_VERSION").unwrap()), 0x0030, 0x0409); // RT_VERSION
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("deckwright.res");
    fs::write(&out, res).unwrap();
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
