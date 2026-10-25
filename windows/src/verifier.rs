//! Structural verification of merged packages against their sources.
use crate::package::{checksum, extract, package_files, read_file, TempDir};
use crate::scaler;
use crate::xml::{read_xml, semantic, Element, A_NS};
use crate::{fail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Normalizes "." and ".." in a '/'-separated path.
pub fn standardize(path: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if path.starts_with('/') { format!("/{joined}") } else { joined }
}

pub fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

pub fn resolve_part(owner: &str, target: &str) -> String {
    let raw = target.split('#').next().unwrap_or("");
    let clean = percent_decode(raw).unwrap_or_else(|| target.to_string());
    let path = if clean.starts_with('/') { clean } else { format!("/{}/{}", dirname(owner), clean) };
    standardize(&path).trim_start_matches('/').to_string()
}

pub fn ordered_parts(root: &Path) -> Result<Vec<String>> {
    let d = read_xml(root, "ppt/presentation.xml")?;
    let r = read_xml(root, "ppt/_rels/presentation.xml.rels")?;
    let rels: HashMap<String, String> = r.root.named("Relationship").map(|e| (e.attr("Id").to_string(), e.attr("Target").to_string())).collect();
    let Some(ids) = d.root.child("sldIdLst") else { return fail("Falta la llista de diapositives.") };
    ids.named("sldId")
        .map(|e| match rels.get(e.rid()) {
            Some(t) => Ok(resolve_part("ppt/presentation.xml", t)),
            None => fail("Falta una relació de diapositiva."),
        })
        .collect()
}

pub fn slide_size(root: &Element) -> Option<(i64, i64)> {
    let sz = root.child("sldSz")?;
    let w: i64 = sz.attr("cx").parse().ok()?;
    let h: i64 = sz.attr("cy").parse().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

#[derive(Clone)]
pub struct PartFingerprint {
    pub timing: String,
    timing_node: Option<Element>,
    pub transition: String,
    text: Vec<String>,
    shape_identity: Vec<String>,
    shape_ids: HashSet<String>,
    timing_target_ids: HashSet<String>,
}

impl PartFingerprint {
    pub fn new(element: &Element) -> PartFingerprint {
        let t = element.child("timing");
        let mut texts = vec![];
        let mut shapes = vec![];
        let mut ids = HashSet::new();
        let mut targets = HashSet::new();
        element.visit(&mut |e| {
            if e.local() == "t" && e.ns == A_NS {
                texts.push(e.text());
            }
            if e.local() == "cNvPr" {
                shapes.push(semantic(Some(e)));
                if !e.attr("id").is_empty() {
                    ids.insert(e.attr("id").to_string());
                }
            }
            if e.local() == "spTgt" && !e.attr("spid").is_empty() {
                targets.insert(e.attr("spid").to_string());
            }
        });
        PartFingerprint {
            timing: semantic(t),
            timing_node: t.cloned(),
            transition: semantic(element.child("transition")),
            text: texts,
            shape_identity: shapes,
            shape_ids: ids,
            timing_target_ids: targets,
        }
    }

    pub fn expected_timing(&self, source: Option<(i64, i64)>, target: Option<(i64, i64)>) -> String {
        let (Some(node), Some(s), Some(t)) = (&self.timing_node, source, target) else { return self.timing.clone() };
        if s == t {
            return self.timing.clone();
        }
        let fit = !scaler::is_widescreen(s);
        let raw = (t.0 as f64 / s.0 as f64).min(t.1 as f64 / s.1 as f64);
        let factor = if fit { raw.min(1.0) } else { raw };
        let rx = s.0 as f64 * factor / t.0 as f64;
        let ry = s.1 as f64 * factor / t.1 as f64;
        if (rx - 1.0).abs() <= 1e-9 && (ry - 1.0).abs() <= 1e-9 {
            return self.timing.clone();
        }
        let sx = (t.0 as f64 - s.0 as f64 * factor) / 2.0 / t.0 as f64;
        let sy = (t.1 as f64 - s.1 as f64 * factor) / 2.0 / t.1 as f64;
        let mut copy = node.clone();
        scaler::scale_timing_animations(&mut copy, rx, ry, sx, sy);
        semantic(Some(&copy))
    }
}

#[derive(Clone)]
pub struct ResourceLink {
    id: String,
    kind: String,
    target: String,
    external: bool,
}

pub fn owner_of_rels(file: &str) -> String {
    if file == "_rels/.rels" {
        return String::new();
    }
    let directory = dirname(file);
    let name = file.rsplit('/').next().unwrap_or("");
    let base = &name[..name.len().saturating_sub(5)];
    let parent = dirname(directory);
    if parent.is_empty() { base.to_string() } else { format!("{parent}/{base}") }
}

pub struct PackageSnapshot {
    pub slides: Vec<String>,
    parts: BTreeMap<String, PartFingerprint>,
    notes: BTreeMap<String, String>,
    binaries: BTreeMap<String, String>,
    has_vba: bool,
    links: BTreeMap<String, Vec<ResourceLink>>,
    slide_size: Option<(i64, i64)>,
}

impl PackageSnapshot {
    pub fn capture(root: &Path) -> Result<PackageSnapshot> {
        let mut parts = BTreeMap::new();
        let mut notes = BTreeMap::new();
        let mut binaries = BTreeMap::new();
        let mut links = BTreeMap::new();
        let size = read_xml(root, "ppt/presentation.xml").ok().and_then(|d| slide_size(&d.root));
        for file in package_files(root) {
            if file.ends_with(".rels") && ["ppt/slides/", "ppt/slideLayouts/", "ppt/slideMasters/", "ppt/notesSlides/"].iter().any(|p| file.starts_with(p)) {
                let d = read_xml(root, &file)?;
                links.insert(file.clone(), d.root.named("Relationship").map(|e| ResourceLink { id: e.attr("Id").into(), kind: e.attr("Type").into(), target: e.attr("Target").into(), external: e.attr("TargetMode") == "External" }).collect());
            }
            if file.ends_with(".xml") && ["ppt/slides/", "ppt/slideLayouts/", "ppt/slideMasters/"].iter().any(|p| file.starts_with(p)) {
                parts.insert(file.clone(), PartFingerprint::new(&read_xml(root, &file)?.root));
            } else if file.starts_with("ppt/notesSlides/") && file.ends_with(".xml") {
                notes.insert(file.clone(), semantic(Some(&read_xml(root, &file)?.root)));
            } else if file.starts_with("ppt/media/") || file.starts_with("ppt/embeddings/") || file.starts_with("ppt/fonts/") {
                binaries.insert(file.clone(), checksum(&read_file(&crate::xml::part_path(root, &file))?));
            }
        }
        Ok(PackageSnapshot {
            slides: ordered_parts(root)?,
            parts,
            notes,
            binaries,
            has_vba: crate::xml::part_path(root, "ppt/vbaProject.bin").exists(),
            links,
            slide_size: size,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileValidation {
    pub file: String,
    pub status: String,
    pub checked_slides: usize,
    pub slides_with_animation: usize,
    pub binary_resources_checked: usize,
    pub checks: Vec<String>,
    pub issues: Vec<String>,
    pub needs_review: Vec<String>,
}

pub fn verify(sources: &[PackageSnapshot], inputs: &[PathBuf], merged: &Path) -> Result<Vec<FileValidation>> {
    let ordered = ordered_parts(merged)?;
    let all_files = package_files(merged);
    let names: HashSet<String> = all_files.iter().cloned().collect();
    let merged_pres = read_xml(merged, "ppt/presentation.xml")?;
    let merged_size = slide_size(&merged_pres.root);
    let mut global = vec![];
    if let Ok(ct) = read_xml(merged, "[Content_Types].xml") {
        let defaults: HashSet<String> = ct.root.named("Default").map(|e| e.attr("Extension").to_lowercase()).collect();
        let overrides: HashSet<String> = ct.root.named("Override").map(|e| e.attr("PartName").trim_start_matches('/').to_string()).collect();
        for file in all_files.iter().filter(|f| *f != "[Content_Types].xml") {
            let ext = if file.ends_with(".rels") { "rels".to_string() } else { file.rsplit_once('.').map(|(_, e)| e.to_lowercase()).filter(|_| file.rsplit('/').next().unwrap_or("").contains('.')).unwrap_or_default() };
            if !overrides.contains(file) && !defaults.contains(&ext) {
                global.push(format!("Falta ContentType al paquet fusionat: {file}"));
            }
        }
    }
    if let Some(ids) = merged_pres.root.child("sldIdLst") {
        let v: Vec<&str> = ids.named("sldId").map(|e| e.attr("id")).collect();
        if v.iter().collect::<HashSet<_>>().len() != v.len() {
            global.push("IDs de diapositiva (sldId) duplicats a presentation.xml.".into());
        }
    }
    let mut master_ids: Vec<String> = merged_pres.root.child("sldMasterIdLst").map(|m| m.named("sldMasterId").map(|e| e.attr("id").to_string()).collect()).unwrap_or_default();
    for file in all_files.iter().filter(|f| f.ends_with(".xml") && f.contains("/slideMasters/") || f.ends_with(".xml") && f.starts_with("slideMasters/")) {
        if let Ok(d) = read_xml(merged, file) {
            master_ids.extend(d.root.descendants("sldLayoutId").iter().map(|e| e.attr("id").to_string()));
        }
    }
    if master_ids.iter().collect::<HashSet<_>>().len() != master_ids.len() {
        global.push("IDs de patró o disseny (sldMasterId/sldLayoutId) duplicats al paquet fusionat.".into());
    }
    let mut broken = vec![];
    for file in &all_files {
        if file.ends_with(".xml") || file.ends_with(".rels") {
            read_xml(merged, file)?;
        }
        if !file.ends_with(".rels") {
            continue;
        }
        let owner = owner_of_rels(file);
        let doc = read_xml(merged, file)?;
        for rel in doc.root.named("Relationship").filter(|r| r.attr("TargetMode") != "External") {
            let target = resolve_part(&owner, rel.attr("Target"));
            if !names.contains(&target) {
                broken.push(format!("{file} → {target}"));
            }
        }
    }
    let mut results = vec![];
    let mut offset = 0;
    for (i, source) in sources.iter().enumerate() {
        let prefix = if i == 0 { String::new() } else { format!("pptMerger/source{i}/") };
        let mut issues = vec![];
        if i == 0 {
            issues.extend(global.iter().cloned());
        }
        let expected: Vec<String> = source.slides.iter().map(|s| format!("{prefix}{s}")).collect();
        let actual: Vec<String> = ordered.iter().skip(offset).take(expected.len()).cloned().collect();
        if actual != expected {
            issues.push("El nombre o l’ordre de les diapositives no coincideix amb l’original.".into());
        }
        for (part, original) in &source.parts {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) {
                issues.push(format!("Falta la diapositiva, el disseny o el patró: {part}"));
                continue;
            }
            let imported = PartFingerprint::new(&read_xml(merged, &destination)?.root);
            let number = source.slides.iter().position(|s| s == part).map(|n| format!("diapositiva {}", n + 1)).unwrap_or_else(|| part.clone());
            if original.expected_timing(source.slide_size, merged_size) != imported.timing {
                issues.push(format!("{number}: l’animació/temporització no coincideix amb l’original (absent o modificada)."));
            }
            if original.transition != imported.transition {
                issues.push(format!("{number}: transició o avançament automàtic absent/modificat."));
            }
            if original.text != imported.text {
                issues.push(format!("{number}: text absent o modificat."));
            }
            if original.shape_identity != imported.shape_identity {
                issues.push(format!("{number}: objectes, identificadors o accions interactives absents/modificats."));
            }
            let valid: HashSet<&String> = original.timing_target_ids.intersection(&original.shape_ids).collect();
            if !valid.iter().all(|id| imported.shape_ids.contains(*id)) {
                issues.push(format!("{number}: referència d’animació (spid) òrfena cap a un objecte inexistent."));
            }
        }
        for (part, links) in &source.links {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) {
                issues.push(format!("Falten relacions de recursos: {part}"));
                continue;
            }
            let doc = read_xml(merged, &destination)?;
            let imported: Vec<&Element> = doc.root.named("Relationship").collect();
            for link in links.iter().filter(|l| !l.kind.ends_with("/notesMaster")) {
                let Some(rel) = imported.iter().find(|r| r.attr("Id") == link.id) else {
                    issues.push(format!("Falta la relació {} a {part}", link.id));
                    continue;
                };
                let expected = if link.external { link.target.clone() } else { format!("{prefix}{}", resolve_part(&owner_of_rels(part), &link.target)) };
                let actual = if link.external { rel.attr("Target").to_string() } else { resolve_part(&owner_of_rels(&destination), rel.attr("Target")) };
                if actual != expected || rel.attr("Type") != link.kind || (rel.attr("TargetMode") == "External") != link.external {
                    issues.push(format!("Recurs enllaçat incorrectament: {part}, relació {}", link.id));
                }
            }
        }
        for (part, original) in &source.notes {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) || semantic(Some(&read_xml(merged, &destination)?.root)) != *original {
                issues.push(format!("Notes absents o modificades: {part}"));
            }
        }
        for (part, original) in &source.binaries {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) || checksum(&read_file(&crate::xml::part_path(merged, &destination))?) != *original {
                issues.push(format!("Recurs multimèdia/font/objecte incrustat absent o modificat: {part}"));
            }
        }
        issues.extend(broken.iter().filter(|b| if i == 0 { !b.starts_with("pptMerger/") } else { b.starts_with(&prefix) }).map(|b| format!("Relació interna trencada: {b}")));
        if i == sources.len() - 1 && ordered.len() != sources.iter().map(|s| s.slides.len()).sum::<usize>() {
            issues.push("El recompte total de diapositives no coincideix.".into());
        }
        let mut review = vec!["No comprovat: aspecte visual, geometria final, fonts heretades, reproducció d’animacions/àudio/vídeo, edició i enllaços externs.".to_string()];
        if source.has_vba {
            review.push(if i == 0 { "VBA del primer fitxer conservat, però funcionament no comprovat." } else { "El projecte VBA d’aquest fitxer no s’ha integrat: importació incompleta si depèn de macros." }.into());
        }
        results.push(FileValidation {
            file: crate::package::file_name(&inputs[i]),
            status: if issues.is_empty() { "structural_checks_passed_needs_review" } else { "import_failed_validation" }.into(),
            checked_slides: source.slides.len(),
            slides_with_animation: source.slides.iter().filter(|s| source.parts.get(*s).map(|p| p.timing != "ABSENT").unwrap_or(false)).count(),
            binary_resources_checked: source.binaries.len(),
            checks: [
                "Ordre i nombre de diapositives",
                "Arbres complets d’animacions i temporitzacions (també patrons i dissenys) i integritat de spid",
                "Transicions i avançaments automàtics",
                "Text, objectes i accions",
                "Contingut i patró de notes",
                "Hash SHA-256 dels recursos multimèdia, fonts i objectes incrustats",
                "XML, ContentTypes, unicitat d’IDs i relacions internes",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            issues,
            needs_review: review,
        });
        offset += source.slides.len();
    }
    Ok(results)
}

pub fn inspect(inputs: &[PathBuf], output: &Path) -> Result<Vec<FileValidation>> {
    let temp = TempDir::new("PPTCheck")?;
    let mut snapshots = vec![];
    for (i, input) in inputs.iter().enumerate() {
        let dir = temp.path().join(format!("s{i}"));
        std::fs::create_dir_all(&dir).or_else(|e| fail(e.to_string()))?;
        extract(input, &dir)?;
        snapshots.push(PackageSnapshot::capture(&dir)?);
    }
    let out = temp.path().join("out");
    std::fs::create_dir_all(&out).or_else(|e| fail(e.to_string()))?;
    extract(output, &out)?;
    verify(&snapshots, inputs, &out)
}
