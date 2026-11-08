//! Open XML merge re-saved by Microsoft PowerPoint through COM automation
//! (PowerShell), with no keyboard, mouse, clipboard or window focus.
use crate::merge::MergeReport;
use crate::package::{extension, TempDir};
use crate::verifier::FileValidation;
use crate::{fail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PowerPointMergeReport {
    pub engine: String,
    pub input_files: Vec<String>,
    pub output: String,
    #[serde(rename = "openXML")]
    pub open_xml: MergeReport,
    pub power_point_slides: usize,
    pub power_point_validation: Vec<FileValidation>,
    pub overall_status: String,
    pub warnings: Vec<String>,
}
impl PowerPointMergeReport {
    pub fn passed(&self) -> bool {
        self.open_xml.structural_validation_passed() && self.power_point_validation.iter().all(|v| v.issues.is_empty())
    }
    pub fn validation(&self) -> Vec<FileValidation> {
        self.open_xml.validation.iter().chain(&self.power_point_validation).cloned().collect()
    }
}

#[cfg(windows)]
const SCRIPT: &str = r#"param([string]$Source, [string]$Target, [int]$Expected)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$wasRunning = @(Get-Process -Name POWERPNT -ErrorAction SilentlyContinue).Count -gt 0
try { $app = New-Object -ComObject PowerPoint.Application } catch { [Console]::Error.WriteLine('NO_POWERPOINT'); exit 3 }
$p = $null
try {
  # ReadOnly = msoTrue, Untitled = msoFalse, WithWindow = msoFalse
  $p = $app.Presentations.Open($Source, -1, 0, 0)
  $n = $p.Slides.Count
  if ($n -ne $Expected) { throw "PowerPoint opened $n slides instead of $Expected." }
  # 24 = ppSaveAsOpenXMLPresentation
  $p.SaveAs($Target, 24)
  Write-Output "$n|closed"
} finally {
  if ($p) { try { $p.Close() } catch {} }
  if (-not $wasRunning) { try { if ($app.Presentations.Count -eq 0) { $app.Quit() } } catch {} }
  [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($app)
}
"#;

#[cfg(windows)]
pub fn available() -> bool {
    std::process::Command::new("reg").args(["query", r"HKCR\PowerPoint.Application"]).creation_flags(0x0800_0000).output().map(|o| o.status.success()).unwrap_or(false)
}
#[cfg(not(windows))]
pub fn available() -> bool {
    false
}

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
fn resave(source: &Path, target: &Path, expected: usize) -> Result<(usize, bool)> {
    use std::io::Read;
    let temp = TempDir::new("PPTShowTools-PowerPointScript")?;
    let script = temp.path().join("desa.ps1");
    // BOM: Windows PowerShell 5 reads scripts without it as ANSI.
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(SCRIPT.as_bytes());
    std::fs::write(&script, bytes).or_else(|e| fail(e.to_string()))?;
    // PowerPoint resolves relative paths against its own folder; absolute paths
    // also never start with "-", which PowerShell would read as a parameter.
    let source = std::path::absolute(source).or_else(|e| fail(e.to_string()))?;
    let target = std::path::absolute(target).or_else(|e| fail(e.to_string()))?;
    let mut child = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Source")
        .arg(&source)
        .arg("-Target")
        .arg(&target)
        .arg("-Expected")
        .arg(expected.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .or_else(|e| fail(format!("Cannot start PowerShell: {e}")))?;
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out_reader = std::thread::spawn(move || {
        let mut s = vec![];
        let _ = out.read_to_end(&mut s);
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = vec![];
        let _ = err.read_to_end(&mut s);
        s
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1800);
    let status = loop {
        if let Some(status) = child.try_wait().or_else(|e| fail(e.to_string()))? {
            break status;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            return fail("PowerPoint did not respond in time. It may be showing a dialog (file repair, sign-in or licence). Check PowerPoint; the result was not validated.");
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    };
    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).trim().to_string();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).trim().to_string();
    if !status.success() {
        if stderr.contains("NO_POWERPOINT") {
            return fail("Microsoft PowerPoint for Windows (desktop) was not found.");
        }
        return fail(format!("PowerPoint could not save the merge: {}", if stderr.is_empty() { "unknown error" } else { &stderr }));
    }
    let line = stdout.lines().last().unwrap_or("");
    let mut parts = line.split('|');
    match (parts.next().and_then(|n| n.trim().parse::<usize>().ok()), parts.next()) {
        (Some(n), Some(state)) => Ok((n, state.trim() == "closed")),
        _ => fail("Unexpected reply from PowerPoint."),
    }
}

#[cfg(not(windows))]
fn resave(_: &Path, _: &Path, _: usize) -> Result<(usize, bool)> {
    fail("This build saves with PowerPoint for Windows.")
}

pub fn merge(inputs: &[PathBuf], output: &Path, fixed: Option<(i64, i64)>, progress: &dyn Fn(String)) -> Result<PowerPointMergeReport> {
    if extension(output) != "pptx" {
        return fail("Saving with PowerPoint produces PPTX. To keep macros, use Merge with a PPTM output.");
    }
    if output.exists() || crate::reports::collides(output, &[".report.json", ".report.txt"]) {
        return fail("The output file or its report already exists. Choose another name.");
    }
    let temp = TempDir::new("PPTShowTools-PowerPoint")?;
    let intermediate = temp.path().join("open-xml-merge.pptx");
    progress("Merging and checking…".into());
    let open_xml = crate::merge::merge_with(inputs, &intermediate, fixed, false)?;
    if !open_xml.structural_validation_passed() {
        let issues: Vec<String> = open_xml.validation.iter().flat_map(|v| v.issues.iter().take(5).map(move |i| format!("{}: {i}", v.file))).collect();
        return fail(format!("The Open XML merge failed its checks; it is not sent to PowerPoint.\n{}", issues.join("\n")));
    }
    progress("Opening and saving with PowerPoint…".into());
    let (slides, closed) = resave(&intermediate, output, open_xml.total_slides)?;
    progress("Checking the file saved by PowerPoint…".into());
    let mut warnings = vec!["Open XML merge saved with Microsoft PowerPoint through COM automation.".to_string()];
    warnings.extend(open_xml.warnings.iter().filter(|w| !w.starts_with("Direct Open XML engine")).cloned());
    warnings.push("Each presentation keeps its master and theme, like “Keep Source Formatting”, but PowerPoint's paste command was not run.".into());
    if !closed {
        warnings.push("PowerPoint could not close the result; it is still open. Close it without saving changes.".into());
    }
    let mut issues = vec![];
    if slides != open_xml.total_slides {
        issues.push(format!("PowerPoint saved {slides} slides; expected {}.", open_xml.total_slides));
    }
    // A result that cannot even be checked is not left behind as if it were valid.
    let verified = crate::office::verify(&[intermediate.clone()], output).map_err(|e| {
        let _ = std::fs::remove_file(output);
        e
    })?;
    let checks: Vec<FileValidation> = verified
        .into_iter()
        .map(|c| FileValidation {
            file: "Open XML merge → saved by PowerPoint".into(),
            status: if issues.is_empty() { c.status } else { "import_failed_validation".into() },
            checked_slides: c.checked_slides,
            slides_with_animation: c.slides_with_animation,
            binary_resources_checked: c.binary_resources_checked,
            checks: c.checks,
            issues: issues.iter().cloned().chain(c.issues).collect(),
            needs_review: vec!["PowerPoint opened and saved the merge; nothing was pasted. Appearance, fonts, editing and playback are not certified.".into(), "PowerPoint reorganises parts and identifiers when saving; the content is compared, not the bytes.".into()],
        })
        .collect();
    let passed = checks.iter().all(|c| c.issues.is_empty());
    let report = PowerPointMergeReport {
        engine: "PPTShowTools-OpenXML+PowerPoint-COM-v0.9.0-windows".into(),
        input_files: inputs.iter().map(|p| p.display().to_string()).collect(),
        output: output.display().to_string(),
        open_xml,
        power_point_slides: slides,
        power_point_validation: checks,
        overall_status: if passed { "structural_checks_passed_needs_review" } else { "import_failed_validation" }.into(),
        warnings,
    };
    let written = (|| -> Result<()> {
        crate::reports::write(output, ".report.json", &crate::to_json(&report)?)?;
        let mut lines = vec!["PPTSHOWTOOLS — OPEN XML MERGE SAVED WITH POWERPOINT".to_string(), if report.passed() { "Structural checks passed. Visual and playback review pending." } else { "IMPORT NOT CORRECT: differences found." }.into(), String::new()];
        for (i, check) in report.validation().iter().enumerate() {
            lines.push(format!("{}. {}", i + 1, check.file));
            lines.push(if check.issues.is_empty() { "STRUCTURE: checks passed; this is not full fidelity." } else { "IMPORT NOT CORRECT" }.into());
            lines.extend(check.issues.iter().map(|c| format!("ERROR: {c}")));
            lines.extend(check.needs_review.iter().map(|c| format!("TO REVIEW: {c}")));
            lines.push(String::new());
        }
        lines.push("WARNINGS".into());
        lines.extend(report.warnings.iter().cloned());
        crate::reports::write(output, ".report.txt", lines.join("\r\n").as_bytes())
    })();
    if let Err(e) = written {
        crate::reports::remove(output, &[".report.json", ".report.txt"]);
        return Err(e);
    }
    Ok(report)
}
