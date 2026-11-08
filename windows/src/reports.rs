//! Where reports and error logs go. Nothing is written next to the files the
//! user converts.
//!
//! - Installed (the installer leaves `PPTShowTools.installed` next to the exe):
//!   %LOCALAPPDATA%\PPT Show Tools\Logs, and files there older than 30 days are deleted.
//! - Portable (no marker): no reports, no logs and no settings file are written.
//!
//! PPTSHOWTOOLS_LOG_DIR replaces the logs folder (and counts as installed; tests).
//! PPTSHOWTOOLS_REPORTS_NEXT_TO_OUTPUT=1 keeps the old `<output>.report.json`
//! beside the output, for scripts and the test suite.
use crate::package::{with_suffix, write_new};
use crate::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub const KEEP_DAYS: u64 = 30;
pub const INSTALLED_MARKER: &str = "PPTShowTools.installed";
static BASES: Mutex<Option<HashMap<PathBuf, PathBuf>>> = Mutex::new(None);

pub fn next_to_output() -> bool {
    std::env::var("PPTSHOWTOOLS_REPORTS_NEXT_TO_OUTPUT").as_deref() == Ok("1")
}

/// True for the installed app; false for the portable one.
pub fn installed() -> bool {
    if std::env::var_os("PPTSHOWTOOLS_LOG_DIR").is_some_and(|d| !d.is_empty()) {
        return true;
    }
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join(INSTALLED_MARKER).is_file())).unwrap_or(false)
}

fn folder() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PPTSHOWTOOLS_LOG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    if !installed() {
        return None;
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("PPT Show Tools").join("Logs"))
}

/// Creates the logs folder and deletes what is older than 30 days.
/// None when nothing may be written (portable).
pub fn prepare() -> Option<PathBuf> {
    let dir = folder()?;
    std::fs::create_dir_all(&dir).ok()?;
    let limit = SystemTime::now().checked_sub(Duration::from_secs(KEEP_DAYS * 86_400))?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_file() && meta.modified().is_ok_and(|m| m < limit) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Some(dir)
}

fn key(output: &Path) -> PathBuf {
    std::path::absolute(output).unwrap_or_else(|_| output.to_path_buf())
}

fn new_base(output: &Path) -> Option<PathBuf> {
    let dir = prepare()?;
    let stem = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "PPT Show Tools".into());
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let id = uuid::Uuid::new_v4().simple().to_string();
    Some(dir.join(format!("{stem}-{secs}-{}", &id[..6])))
}

/// Path without suffix shared by every report file of one output.
fn base(output: &Path, fresh: bool) -> Option<PathBuf> {
    if next_to_output() {
        return Some(output.to_path_buf());
    }
    let mut guard = BASES.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if !fresh {
        if let Some(known) = map.get(&key(output)) {
            return Some(known.clone());
        }
    }
    let made = new_base(output)?;
    map.insert(key(output), made.clone());
    Some(made)
}

/// The report file of an output written earlier in this process, if any.
pub fn file(output: &Path, suffix: &str) -> Option<PathBuf> {
    let path = with_suffix(&base(output, false)?, suffix);
    path.is_file().then_some(path)
}

pub fn write(output: &Path, suffix: &str, data: &[u8]) -> Result<()> {
    let Some(mut b) = base(output, false) else { return Ok(()) };
    // A new conversion to a name used earlier in this session starts a new report.
    if !next_to_output() && with_suffix(&b, suffix).exists() {
        match base(output, true) {
            Some(fresh) => b = fresh,
            None => return Ok(()),
        }
    }
    write_new(&with_suffix(&b, suffix), data)
}

pub fn remove(output: &Path, suffixes: &[&str]) {
    if let Some(b) = base(output, false) {
        for s in suffixes {
            let _ = std::fs::remove_file(with_suffix(&b, s));
        }
    }
}

/// Only when reports go beside the output can they collide with an existing file.
pub fn collides(output: &Path, suffixes: &[&str]) -> bool {
    next_to_output() && suffixes.iter().any(|s| with_suffix(output, s).exists())
}

/// Error log in the logs folder; returns its path, or a note when none is written.
pub fn log_failure(output: &Path, error: &str) -> String {
    let dir = if next_to_output() { Some(output.parent().unwrap_or(Path::new(".")).to_path_buf()) } else { prepare() };
    let Some(dir) = dir else { return "none (portable version)".into() };
    let log = dir.join(format!("PPTShowTools-ERROR-{}.log", uuid::Uuid::new_v4()));
    match write_new(&log, error.as_bytes()) {
        Ok(()) => log.display().to_string(),
        Err(_) => "No log could be written.".into(),
    }
}
