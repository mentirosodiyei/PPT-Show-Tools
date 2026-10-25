//! ZIP packages, temporary folders and file helpers.
use crate::{fail, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_PACKAGE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_PACKAGE_ENTRIES: usize = 100_000;

/// Temporary folder removed when dropped.
pub struct TempDir(pub PathBuf);
impl TempDir {
    pub fn new(prefix: &str) -> Result<TempDir> {
        let p = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).or_else(|e| fail(format!("No es pot crear la carpeta temporal: {e}")))?;
        Ok(TempDir(p))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}
pub fn extension(p: &Path) -> String {
    p.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}
pub fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Untrusted packages: reject absolute or parent paths, links, encryption,
/// duplicates (also case-insensitive, as on Windows) and archives that would
/// expand beyond a sane size, before and while writing anything.
pub fn extract(input: &Path, dir: &Path) -> Result<()> {
    let name = file_name(input);
    let invalid = || crate::Error::Fail(format!("«{name}» no és un paquet ZIP vàlid de PowerPoint."));
    let file = fs::File::open(input).or_else(|e| fail(format!("No es pot obrir «{name}»: {e}")))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| invalid())?;
    if zip.len() > MAX_PACKAGE_ENTRIES {
        return fail(format!("«{name}» és massa gran o té massa entrades per processar-lo amb seguretat."));
    }
    let mut declared: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(|_| invalid())?;
        declared = declared.saturating_add(entry.size());
    }
    if declared > MAX_PACKAGE_BYTES {
        return fail(format!("«{name}» és massa gran o té massa entrades per processar-lo amb seguretat."));
    }
    let mut seen = std::collections::HashSet::new();
    let mut written: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| crate::Error::Fail(format!("«{name}»: entrada ZIP no llegible ({e}).")))?;
        let entry_name = entry.name().to_string();
        let special = entry.unix_mode().map(|m| m & 0o170000 != 0o100000 && m & 0o170000 != 0o040000 && m & 0o170000 != 0).unwrap_or(false);
        if special {
            return fail(format!("«{name}» conté enllaços simbòlics o entrades especials, no admesos en un PowerPoint: {entry_name}"));
        }
        let segments: Vec<&str> = entry_name.split('/').filter(|s| !s.is_empty()).collect();
        let bad = entry_name.starts_with('/') || entry_name.contains('\\') || entry_name.contains(':') || segments.iter().any(|s| *s == ".." || *s == ".") || segments.is_empty();
        if bad {
            return fail(format!("«{name}»: ruta d’entrada ZIP no admesa: {entry_name}"));
        }
        let target = segments.iter().fold(dir.to_path_buf(), |p, s| p.join(s));
        if entry.is_dir() {
            fs::create_dir_all(&target).or_else(|e| fail(format!("No es pot extreure «{name}»: {e}")))?;
            continue;
        }
        if !seen.insert(entry_name.to_lowercase()) {
            return fail(format!("«{name}»: entrada ZIP duplicada: {entry_name}"));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).or_else(|e| fail(format!("No es pot extreure «{name}»: {e}")))?;
        }
        let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&target).or_else(|e| fail(format!("No es pot extreure «{name}» ({entry_name}): {e}")))?;
        // Declared sizes can lie; count what is really written.
        let mut limited = (&mut entry).take(MAX_PACKAGE_BYTES - written + 1);
        let copied = std::io::copy(&mut limited, &mut out).or_else(|e| fail(format!("«{name}»: no es pot llegir {entry_name} ({e}).")))?;
        written += copied;
        if written > MAX_PACKAGE_BYTES {
            return fail(format!("«{name}» és massa gran o té massa entrades per processar-lo amb seguretat."));
        }
    }
    Ok(())
}

/// Relative part names ('/'-separated) of every regular file, sorted.
pub fn package_files(root: &Path) -> Vec<String> {
    let mut out = vec![];
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(items) = fs::read_dir(dir) else { return };
        for item in items.flatten() {
            let Ok(kind) = item.file_type() else { continue };
            let name = item.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            if kind.is_dir() {
                walk(&item.path(), &rel, out);
            } else if kind.is_file() {
                out.push(rel);
            }
        }
    }
    walk(root, "", &mut out);
    out.sort();
    out
}

/// Zips a package folder; [Content_Types].xml goes first as Office expects.
/// The output must not exist yet.
pub fn zip_dir(root: &Path, output: &Path) -> Result<()> {
    let file = fs::OpenOptions::new().write(true).create_new(true).open(output).or_else(|e| fail(format!("No es pot crear {}: {e}", output.display())))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(false);
    let mut files = package_files(root);
    files.sort_by_key(|f| f != "[Content_Types].xml");
    for part in files {
        let data = fs::read(crate::xml::part_path(root, &part)).or_else(|e| fail(format!("No es pot llegir {part}: {e}")))?;
        let opts = if data.len() as u64 >= 0xFFFF_FFFF { options.large_file(true) } else { options };
        zip.start_file(part.as_str(), opts).or_else(|e| fail(format!("No es pot empaquetar {part}: {e}")))?;
        zip.write_all(&data).or_else(|e| fail(format!("No es pot empaquetar {part}: {e}")))?;
    }
    zip.finish().or_else(|e| fail(format!("No es pot tancar el paquet: {e}")))?.flush().or_else(|e| fail(format!("No es pot desar el paquet: {e}")))?;
    Ok(())
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = with_suffix(path, &format!(".tmp-{}", uuid::Uuid::new_v4()));
    fs::write(&tmp, data).or_else(|e| fail(format!("No es pot escriure {}: {e}", path.display())))?;
    fs::rename(&tmp, path).or_else(|e| {
        let _ = fs::remove_file(&tmp);
        fail(format!("No es pot escriure {}: {e}", path.display()))
    })
}

/// Writes a new file and refuses to replace an existing one.
pub fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(path).or_else(|e| fail(format!("No es pot crear {}: {e}", path.display())))?;
    f.write_all(data).or_else(|e| fail(format!("No es pot escriure {}: {e}", path.display())))
}

/// Copies a file to a path that must not exist yet.
pub fn copy_new(from: &Path, to: &Path) -> Result<()> {
    let mut src = fs::File::open(from).or_else(|e| fail(format!("No es pot llegir {}: {e}", from.display())))?;
    let mut dst = fs::OpenOptions::new().write(true).create_new(true).open(to).or_else(|e| fail(format!("No es pot crear {}: {e}", to.display())))?;
    std::io::copy(&mut src, &mut dst).or_else(|e| {
        let _ = fs::remove_file(to);
        fail(format!("No es pot copiar a {}: {e}", to.display()))
    })?;
    Ok(())
}

pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).or_else(|e| fail(format!("No es pot copiar el paquet: {e}")))?;
    for item in fs::read_dir(from).or_else(|e| fail(format!("No es pot llegir el paquet: {e}")))?.flatten() {
        let kind = item.file_type().or_else(|e| fail(format!("No es pot llegir el paquet: {e}")))?;
        let dest = to.join(item.file_name());
        if kind.is_dir() {
            copy_tree(&item.path(), &dest)?;
        } else if kind.is_file() {
            fs::copy(item.path(), &dest).or_else(|e| fail(format!("No es pot copiar el paquet: {e}")))?;
        }
    }
    Ok(())
}

pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    let mut f = fs::File::open(path).or_else(|e| fail(format!("No es pot llegir {}: {e}", path.display())))?;
    let mut v = vec![];
    f.read_to_end(&mut v).or_else(|e| fail(format!("No es pot llegir {}: {e}", path.display())))?;
    Ok(v)
}

pub fn checksum(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b) || a == b
}
