//! Updates from the project's GitHub releases. A release is installed only if
//! its SHA256SUMS.txt carries a valid Ed25519 signature from the author's key
//! (tools/release-sign.swift) and the download matches its checksum, so a file
//! replaced on GitHub, or by anyone in between, is never installed.
//! Same rules as Sources/Updater.swift.
use crate::{fail, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const REPOSITORY: &str = "mentirosodiyei/PPT-Show-Tools";
/// Public half of the release signing key.
const RELEASE_KEY: &str = "3gayXZS9YvENGojbepLokzqvcMrleqUdcIw/+8+ReaM=";
pub const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(12 * 3600);
const MAX_DOWNLOAD: u64 = 500 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Vec<u64>,
    pub notes: String,
    pub page: String,
    assets: HashMap<String, String>,
}
impl Release {
    pub fn version_text(&self) -> String {
        self.version.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(".")
    }
}

/// A release shown by the screenshot mode, never installed.
pub fn preview(version: &str, notes: &str) -> Release {
    Release { version: parse(version).unwrap_or_default(), notes: notes.into(), page: String::new(), assets: HashMap::new() }
}

/// The test suite points the updater at a local server with its own key;
/// that is only accepted for 127.0.0.1.
fn test_server() -> Option<String> {
    std::env::var("PPTSHOWTOOLS_UPDATE_URL").ok().filter(|u| u.starts_with("http://127.0.0.1:") || u.starts_with("http://127.0.0.1/"))
}
fn api_url() -> String {
    test_server().unwrap_or_else(|| format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=10"))
}
fn public_key() -> String {
    if test_server().is_some() {
        if let Ok(k) = std::env::var("PPTSHOWTOOLS_UPDATE_KEY") {
            return k;
        }
    }
    RELEASE_KEY.into()
}
pub fn current_version() -> String {
    if test_server().is_some() {
        if let Ok(v) = std::env::var("PPTSHOWTOOLS_VERSION") {
            return v;
        }
    }
    env!("CARGO_PKG_VERSION").into()
}

/// "v0.10.1" → [0, 10, 1].
pub fn parse(tag: &str) -> Option<Vec<u64>> {
    let parts: Vec<Option<u64>> = tag.trim_start_matches(['v', 'V']).split('.').map(|p| p.parse().ok()).collect();
    ((2..=4).contains(&parts.len()) && parts.iter().all(Option::is_some)).then(|| parts.into_iter().flatten().collect())
}
pub fn newer(a: &[u64], b: &[u64]) -> bool {
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Only HTTPS from GitHub, or the local test server.
fn allowed(url: &str) -> bool {
    if test_server().is_some() {
        return url.starts_with("http://127.0.0.1:") || url.starts_with("http://127.0.0.1/");
    }
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', ':']).next().unwrap_or("").to_ascii_lowercase();
    ["github.com", "api.github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"].contains(&host.as_str())
}

/// GET with redirects followed by hand, each one checked against `allowed`.
fn fetch(url: &str, limit: u64) -> Result<Vec<u8>> {
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(120)).build();
    let mut url = url.to_string();
    for _ in 0..6 {
        if !allowed(&url) {
            return fail("Refused to download from an unexpected address.");
        }
        let response = match agent.get(&url).set("User-Agent", &format!("PPTShowTools/{}", current_version())).set("Accept", "application/vnd.github+json").call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return fail(format!("GitHub answered with error {code}.")),
            Err(e) => return fail(format!("Cannot reach GitHub: {e}")),
        };
        if (300..400).contains(&response.status()) {
            url = response.header("Location").map(str::to_string).ok_or_else(|| crate::Error::Fail("Redirect without a destination.".into()))?;
            continue;
        }
        let mut data = vec![];
        response.into_reader().take(limit + 1).read_to_end(&mut data).or_else(|e| fail(format!("Download interrupted: {e}")))?;
        if data.len() as u64 > limit {
            return fail("The download is larger than expected.");
        }
        return Ok(data);
    }
    fail("Too many redirects.")
}

/// The newest published release newer than this app, or None.
pub fn latest() -> Result<Option<Release>> {
    let list: serde_json::Value = serde_json::from_slice(&fetch(&api_url(), 4 * 1024 * 1024)?).or_else(|_| fail("Unexpected answer from GitHub."))?;
    let current = parse(&current_version()).unwrap_or_default();
    let mut best: Option<Release> = None;
    for item in list.as_array().into_iter().flatten() {
        if item["draft"].as_bool() == Some(true) {
            continue;
        }
        let Some(version) = item["tag_name"].as_str().and_then(parse) else { continue };
        if !newer(&version, &current) || best.as_ref().is_some_and(|b| !newer(&version, &b.version)) {
            continue;
        }
        let assets = item["assets"].as_array().into_iter().flatten().filter_map(|a| Some((a["name"].as_str()?.to_string(), a["browser_download_url"].as_str()?.to_string()))).collect();
        best = Some(Release { version, notes: item["body"].as_str().unwrap_or("").into(), page: item["html_url"].as_str().unwrap_or("").into(), assets });
    }
    Ok(best)
}

/// Installed copies update with the installer; the portable one with its zip.
pub fn asset_name(release: &Release) -> String {
    let v = release.version_text();
    if crate::reports::installed() {
        format!("PPTShowTools-Setup-{v}.exe")
    } else {
        format!("PPTShowTools-{v}-Windows-portable.zip")
    }
}

fn verified_checksums(release: &Release) -> Result<HashMap<String, String>> {
    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let (Some(sums_url), Some(sig_url)) = (release.assets.get("SHA256SUMS.txt"), release.assets.get("SHA256SUMS.txt.sig")) else {
        return fail("This release is not signed, so it is not installed automatically.");
    };
    let sums = fetch(sums_url, 1024 * 1024)?;
    let sig_text = String::from_utf8_lossy(&fetch(sig_url, 4096)?).trim().to_string();
    let b64 = base64::engine::general_purpose::STANDARD;
    let key: Option<[u8; 32]> = b64.decode(public_key()).ok().and_then(|k| k.try_into().ok());
    let sig: Option<[u8; 64]> = b64.decode(sig_text).ok().and_then(|s| s.try_into().ok());
    let valid = match (key.and_then(|k| VerifyingKey::from_bytes(&k).ok()), sig) {
        (Some(k), Some(s)) => k.verify(&sums, &Signature::from_bytes(&s)).is_ok(),
        _ => false,
    };
    if !valid {
        return fail("The release's signature is not valid. The update was not installed.");
    }
    Ok(String::from_utf8_lossy(&sums)
        .lines()
        .filter_map(|line| {
            let (hash, name) = line.split_once(' ')?;
            (hash.len() == 64).then(|| (name.trim_start_matches([' ', '*']).trim().to_string(), hash.to_ascii_lowercase()))
        })
        .collect())
}

/// Downloads the right file for this copy and checks it; returns its path in a new temporary folder.
pub fn download(release: &Release, progress: &dyn Fn(String)) -> Result<PathBuf> {
    let name = asset_name(release);
    progress("Checking the release signature…".into());
    let sums = verified_checksums(release)?;
    let (Some(expected), Some(url)) = (sums.get(&name), release.assets.get(&name)) else {
        return fail(format!("This release has no {name}."));
    };
    progress(format!("Downloading {name}…"));
    let data = fetch(url, MAX_DOWNLOAD)?;
    if crate::package::checksum(&data) != *expected {
        return fail("The download does not match the release checksum. The update was not installed.");
    }
    let folder = std::env::temp_dir().join(format!("PPTShowTools-Update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).or_else(|e| fail(e.to_string()))?;
    let file = folder.join(&name);
    std::fs::write(&file, data).or_else(|e| fail(e.to_string()))?;
    Ok(file)
}

/// Portable copy: the files of the new zip replace those in `folder`. The
/// running exe is renamed aside first (Windows allows renaming it) and the old
/// copy is removed on the next launch.
pub fn install_portable(zip_path: &Path, folder: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path).or_else(|e| fail(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(file).or_else(|_| fail("The update is not a valid zip file."))?;
    let wanted = ["PPTShowTools.exe", "ffmpeg.exe", "FFMPEG-LICENSE.txt", "FFMPEG-NOTICE.txt"];
    let mut staged = vec![];
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).or_else(|_| fail("The update is not a valid zip file."))?;
        let name = entry.name().map_err(|_| crate::Error::Fail("Invalid name in the update.".into()))?.to_string();
        let Some(base) = name.strip_prefix("PPT Show Tools/").filter(|b| wanted.contains(b)) else { continue };
        let target = folder.join(format!("{base}.new"));
        let mut out = std::fs::File::create(&target).or_else(|e| fail(format!("Cannot write in {}: {e}", folder.display())))?;
        std::io::copy(&mut (&mut entry).take(MAX_DOWNLOAD), &mut out).or_else(|e| fail(e.to_string()))?;
        staged.push(base.to_string());
    }
    if !staged.iter().any(|s| s == "PPTShowTools.exe") {
        for s in &staged {
            let _ = std::fs::remove_file(folder.join(format!("{s}.new")));
        }
        return fail("The update does not contain PPTShowTools.exe.");
    }
    for name in &staged {
        let (current, new, old) = (folder.join(name), folder.join(format!("{name}.new")), folder.join(format!("{name}.old")));
        let _ = std::fs::remove_file(&old);
        if current.exists() {
            std::fs::rename(&current, &old).or_else(|e| fail(format!("Cannot replace {name}: {e}")))?;
        }
        std::fs::rename(&new, &current).or_else(|e| fail(format!("Cannot replace {name}: {e}")))?;
    }
    Ok(())
}

/// Installed copy: the installer runs silently and opens the new version.
pub fn start_installer(setup: &Path) -> Result<()> {
    let mut command = std::process::Command::new(setup);
    command.arg("/S");
    command.spawn().or_else(|e| fail(format!("Cannot start the installer: {e}")))?;
    Ok(())
}

/// Removes the copies left by an update, once the new version runs.
pub fn clean_up() {
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().ends_with(".old") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Short release notes for the update window: Markdown simplified.
pub fn plain_notes(notes: &str) -> String {
    let link = regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)").expect("valid pattern");
    let text = link.replace_all(&notes.replace("**", "").replace('`', ""), "$1").to_string();
    let text: Vec<String> = text.lines().map(|l| if l.starts_with('#') { l.trim_start_matches(['#', ' ']).to_uppercase() } else { l.to_string() }).collect();
    let text = text.join("\n");
    if text.chars().count() > 4000 {
        format!("{}…", text.chars().take(4000).collect::<String>())
    } else {
        text
    }
}
