//! Open XML merge re-saved by Microsoft PowerPoint through COM automation
//! (PowerShell), with no keyboard, mouse, clipboard or window focus.
use crate::merge::MergeReport;
use crate::package::{extension, with_suffix, write_new, TempDir};
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
  if ($n -ne $Expected) { throw "PowerPoint ha obert $n diapositives en lloc de $Expected." }
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
    let temp = TempDir::new("PPTPowerPoint")?;
    let script = temp.path().join("desa.ps1");
    // BOM: Windows PowerShell 5 reads scripts without it as ANSI.
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(SCRIPT.as_bytes());
    std::fs::write(&script, bytes).or_else(|e| fail(e.to_string()))?;
    let mut child = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Source")
        .arg(source)
        .arg("-Target")
        .arg(target)
        .arg("-Expected")
        .arg(expected.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .or_else(|e| fail(format!("No es pot iniciar PowerShell: {e}")))?;
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
            return fail("PowerPoint no ha respost a temps. Pot ser que mostri un diàleg (reparació del fitxer, inici de sessió o llicència). Revisa PowerPoint; el resultat no s’ha validat.");
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    };
    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).trim().to_string();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).trim().to_string();
    if !status.success() {
        if stderr.contains("NO_POWERPOINT") {
            return fail("No s’ha trobat Microsoft PowerPoint per a Windows (cal la versió d’escriptori).");
        }
        return fail(format!("PowerPoint no ha pogut desar la fusió: {}", if stderr.is_empty() { "error desconegut" } else { &stderr }));
    }
    let line = stdout.lines().last().unwrap_or("");
    let mut parts = line.split('|');
    match (parts.next().and_then(|n| n.trim().parse::<usize>().ok()), parts.next()) {
        (Some(n), Some(state)) => Ok((n, state.trim() == "closed")),
        _ => fail("Resposta inesperada de PowerPoint."),
    }
}

#[cfg(not(windows))]
fn resave(_: &Path, _: &Path, _: usize) -> Result<(usize, bool)> {
    fail("El motor amb PowerPoint d’aquesta versió fa servir PowerPoint per a Windows.")
}

pub fn merge(inputs: &[PathBuf], output: &Path, fit: bool, progress: &dyn Fn(String)) -> Result<PowerPointMergeReport> {
    if extension(output) != "pptx" {
        return fail("El motor amb PowerPoint desa en PPTX. Per conservar macros, fes servir la fusió Open XML en PPTM.");
    }
    if output.exists() || with_suffix(output, ".report.json").exists() || with_suffix(output, ".report.txt").exists() {
        return fail("El fitxer de sortida o l’informe ja existeix. Tria un altre nom.");
    }
    let temp = TempDir::new("PPTMergerPowerPoint")?;
    let intermediate = temp.path().join("fusio-open-xml.pptx");
    progress("Fusionant i comprovant amb Open XML…".into());
    let open_xml = crate::merge::merge(inputs, &intermediate, fit)?;
    if !open_xml.structural_validation_passed() {
        let issues: Vec<String> = open_xml.validation.iter().flat_map(|v| v.issues.iter().take(5).map(move |i| format!("{}: {i}", v.file))).collect();
        return fail(format!("La fusió Open XML no supera la comprovació; no s’envia a PowerPoint.\n{}", issues.join("\n")));
    }
    progress("Obrint i desant amb PowerPoint…".into());
    let (slides, closed) = resave(&intermediate, output, open_xml.total_slides)?;
    progress("Comprovant el fitxer desat per PowerPoint…".into());
    let mut warnings = vec!["Fusió Open XML desada amb Microsoft PowerPoint per automatització COM.".to_string()];
    warnings.extend(open_xml.warnings.iter().filter(|w| !w.starts_with("Motor Open XML directe")).cloned());
    warnings.push("Cada presentació conserva el seu patró i tema, com «Mantenir el format d’origen», però no s’ha executat l’ordre d’enganxar de PowerPoint.".into());
    if !closed {
        warnings.push("PowerPoint no ha pogut tancar el resultat; continua obert. Tanca’l sense desar canvis.".into());
    }
    let mut issues = vec![];
    if slides != open_xml.total_slides {
        issues.push(format!("PowerPoint ha desat {slides} diapositives; se n’esperaven {}.", open_xml.total_slides));
    }
    let checks: Vec<FileValidation> = crate::office::verify(&[intermediate.clone()], output)?
        .into_iter()
        .map(|c| FileValidation {
            file: "Fusió Open XML → desat per PowerPoint".into(),
            status: if issues.is_empty() { c.status } else { "import_failed_validation".into() },
            checked_slides: c.checked_slides,
            slides_with_animation: c.slides_with_animation,
            binary_resources_checked: c.binary_resources_checked,
            checks: c.checks,
            issues: issues.iter().cloned().chain(c.issues).collect(),
            needs_review: vec!["PowerPoint ha obert i desat la fusió; no s’ha executat cap enganxament. No es certifica l’aspecte, les fonts, l’edició ni la reproducció.".into(), "PowerPoint reorganitza parts i identificadors en desar; es compara el contingut, no els bytes.".into()],
        })
        .collect();
    let passed = checks.iter().all(|c| c.issues.is_empty());
    let report = PowerPointMergeReport {
        engine: "OpenXML+PowerPoint-COM-v0.6.0-windows".into(),
        input_files: inputs.iter().map(|p| p.display().to_string()).collect(),
        output: output.display().to_string(),
        open_xml,
        power_point_slides: slides,
        power_point_validation: checks,
        overall_status: if passed { "structural_checks_passed_needs_review" } else { "import_failed_validation" }.into(),
        warnings,
    };
    let written = (|| -> Result<()> {
        write_new(&with_suffix(output, ".report.json"), &crate::to_json(&report)?)?;
        let mut lines = vec!["PPT MERGER — FUSIÓ OPEN XML DESADA AMB POWERPOINT".to_string(), if report.passed() { "Comprovació estructural superada. Revisió visual i reproducció pendents." } else { "IMPORTACIÓ NO CORRECTA: diferències detectades." }.into(), String::new()];
        for (i, check) in report.validation().iter().enumerate() {
            lines.push(format!("{}. {}", i + 1, check.file));
            lines.push(if check.issues.is_empty() { "ESTRUCTURA: comprovacions superades; no equival a fidelitat total." } else { "IMPORTACIÓ NO CORRECTA" }.into());
            lines.extend(check.issues.iter().map(|c| format!("ERROR: {c}")));
            lines.extend(check.needs_review.iter().map(|c| format!("REVISIÓ PENDENT: {c}")));
            lines.push(String::new());
        }
        lines.push("AVISOS".into());
        lines.extend(report.warnings.iter().cloned());
        write_new(&with_suffix(output, ".report.txt"), lines.join("\r\n").as_bytes())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(with_suffix(output, ".report.json"));
        return Err(e);
    }
    Ok(report)
}
