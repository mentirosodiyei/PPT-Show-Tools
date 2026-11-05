//! Structural verification of merged packages against their sources.
use crate::package::{checksum_file, extract, package_files, TempDir};
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
    let Some(ids) = d.root.child("sldIdLst") else { return fail("The slide list is missing.") };
    ids.named("sldId")
        .map(|e| match rels.get(e.rid()) {
            Some(t) => Ok(resolve_part("ppt/presentation.xml", t)),
            None => fail("A slide relationship is missing."),
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
            transition: semantic(element.child("transition")),
            text: texts,
            shape_identity: shapes,
            shape_ids: ids,
            timing_target_ids: targets,
        }
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
    embedded_fonts: Vec<String>,
    links: BTreeMap<String, Vec<ResourceLink>>,
}

impl PackageSnapshot {
    pub fn capture(root: &Path) -> Result<PackageSnapshot> {
        let mut parts = BTreeMap::new();
        let mut notes = BTreeMap::new();
        let mut binaries = BTreeMap::new();
        let mut links = BTreeMap::new();
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
                binaries.insert(file.clone(), checksum_file(&crate::xml::part_path(root, &file))?);
            }
        }
        Ok(PackageSnapshot {
            slides: ordered_parts(root)?,
            parts,
            notes,
            binaries,
            has_vba: crate::xml::part_path(root, "ppt/vbaProject.bin").exists(),
            embedded_fonts: embedded_typefaces(root),
            links,
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
    let mut global = vec![];
    if read_xml(merged, "[Content_Types].xml").is_err() {
        global.push("[Content_Types].xml is missing or unreadable in the merged package.".into());
    }
    // Every embedded font of every source must still be registered.
    let mut merged_fonts = embedded_typefaces(merged);
    for source in sources {
        for font in &source.embedded_fonts {
            match merged_fonts.iter().position(|f| f == font) {
                Some(i) => {
                    merged_fonts.remove(i);
                }
                None => global.push(format!("Embedded font “{font}” is not registered in the merged presentation.")),
            }
        }
    }
    if let Ok(ct) = read_xml(merged, "[Content_Types].xml") {
        let defaults: HashSet<String> = ct.root.named("Default").map(|e| e.attr("Extension").to_lowercase()).collect();
        let overrides: HashSet<String> = ct.root.named("Override").map(|e| e.attr("PartName").trim_start_matches('/').to_string()).collect();
        for file in all_files.iter().filter(|f| *f != "[Content_Types].xml") {
            let ext = if file.ends_with(".rels") { "rels".to_string() } else { file.rsplit_once('.').map(|(_, e)| e.to_lowercase()).filter(|_| file.rsplit('/').next().unwrap_or("").contains('.')).unwrap_or_default() };
            if !overrides.contains(file) && !defaults.contains(&ext) {
                global.push(format!("Missing content type in the merged package: {file}"));
            }
        }
    }
    if let Some(ids) = merged_pres.root.child("sldIdLst") {
        let v: Vec<&str> = ids.named("sldId").map(|e| e.attr("id")).collect();
        if v.iter().collect::<HashSet<_>>().len() != v.len() {
            global.push("Duplicate slide IDs (sldId) in presentation.xml.".into());
        }
    }
    let mut master_ids: Vec<String> = merged_pres.root.child("sldMasterIdLst").map(|m| m.named("sldMasterId").map(|e| e.attr("id").to_string()).collect()).unwrap_or_default();
    for file in all_files.iter().filter(|f| f.ends_with(".xml") && f.contains("/slideMasters/") || f.ends_with(".xml") && f.starts_with("slideMasters/")) {
        if let Ok(d) = read_xml(merged, file) {
            master_ids.extend(d.root.descendants("sldLayoutId").iter().map(|e| e.attr("id").to_string()));
        }
    }
    if master_ids.iter().collect::<HashSet<_>>().len() != master_ids.len() {
        global.push("Duplicate master or layout IDs (sldMasterId/sldLayoutId) in the merged package.".into());
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
            issues.push("The number or order of slides does not match the original.".into());
        }
        for (part, original) in &source.parts {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) {
                issues.push(format!("Missing slide, layout or master: {part}"));
                continue;
            }
            let imported = PartFingerprint::new(&read_xml(merged, &destination)?.root);
            let number = source.slides.iter().position(|s| s == part).map(|n| format!("slide {}", n + 1)).unwrap_or_else(|| part.clone());
            // Animation coordinates are fractions of the slide: resizing keeps them.
            if original.timing != imported.timing {
                issues.push(format!("{number}: animation/timing does not match the original (missing or changed)."));
            }
            if original.transition != imported.transition {
                issues.push(format!("{number}: transition or automatic advance missing/changed."));
            }
            if original.text != imported.text {
                issues.push(format!("{number}: text missing or changed."));
            }
            if original.shape_identity != imported.shape_identity {
                issues.push(format!("{number}: objects, identifiers or interactive actions missing/changed."));
            }
            let valid: HashSet<&String> = original.timing_target_ids.intersection(&original.shape_ids).collect();
            if !valid.iter().all(|id| imported.shape_ids.contains(*id)) {
                issues.push(format!("{number}: orphan animation reference (spid) to a missing object."));
            }
        }
        for (part, links) in &source.links {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) {
                issues.push(format!("Missing resource relationships: {part}"));
                continue;
            }
            let doc = read_xml(merged, &destination)?;
            let imported: Vec<&Element> = doc.root.named("Relationship").collect();
            for link in links.iter().filter(|l| !l.kind.ends_with("/notesMaster")) {
                let Some(rel) = imported.iter().find(|r| r.attr("Id") == link.id) else {
                    issues.push(format!("Missing relationship {} in {part}", link.id));
                    continue;
                };
                let expected = if link.external { link.target.clone() } else { format!("{prefix}{}", resolve_part(&owner_of_rels(part), &link.target)) };
                let actual = if link.external { rel.attr("Target").to_string() } else { resolve_part(&owner_of_rels(&destination), rel.attr("Target")) };
                if actual != expected || rel.attr("Type") != link.kind || (rel.attr("TargetMode") == "External") != link.external {
                    issues.push(format!("Wrongly linked resource: {part}, relationship {}", link.id));
                }
            }
        }
        for (part, original) in &source.notes {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) || semantic(Some(&read_xml(merged, &destination)?.root)) != *original {
                issues.push(format!("Notes missing or changed: {part}"));
            }
        }
        for (part, original) in &source.binaries {
            let destination = format!("{prefix}{part}");
            if !names.contains(&destination) || checksum_file(&crate::xml::part_path(merged, &destination))? != *original {
                issues.push(format!("Media, font or embedded object missing or changed: {part}"));
            }
        }
        issues.extend(broken.iter().filter(|b| if i == 0 { !b.starts_with("pptMerger/") } else { b.starts_with(&prefix) }).map(|b| format!("Broken internal relationship: {b}")));
        if i == sources.len() - 1 && ordered.len() != sources.iter().map(|s| s.slides.len()).sum::<usize>() {
            issues.push("The total slide count does not match.".into());
        }
        let mut review = vec!["Not checked: visual appearance, final geometry, inherited fonts, animation/audio/video playback, editing and external links.".to_string()];
        if source.has_vba {
            review.push(if i == 0 { "The first file's VBA is kept, but not tested." } else { "This file's VBA project was not merged: the import is incomplete if it relies on macros." }.into());
        }
        results.push(FileValidation {
            file: crate::package::file_name(&inputs[i]),
            status: if issues.is_empty() { "structural_checks_passed_needs_review" } else { "import_failed_validation" }.into(),
            checked_slides: source.slides.len(),
            slides_with_animation: source.slides.iter().filter(|s| source.parts.get(*s).map(|p| p.timing != "ABSENT").unwrap_or(false)).count(),
            binary_resources_checked: source.binaries.len(),
            checks: [
                "Slide order and count",
                "Full animation and timing trees (also masters and layouts) and spid integrity",
                "Transitions and automatic advance",
                "Text, objects and actions",
                "Notes content and master",
                "SHA-256 of media, fonts and embedded objects",
                "XML, content types, unique IDs and internal relationships",
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

/// Typefaces registered in a presentation's embeddedFontLst.
fn embedded_typefaces(root: &Path) -> Vec<String> {
    let Ok(pres) = read_xml(root, "ppt/presentation.xml") else { return vec![] };
    let Some(list) = pres.root.child("embeddedFontLst") else { return vec![] };
    list.named("embeddedFont").filter_map(|f| f.child("font").map(|x| x.attr("typeface").to_string())).collect()
}

/// `fixed` and `fit` must match the options the output was merged with, so
/// animations are checked against the scaling actually applied.
pub fn inspect(inputs: &[PathBuf], output: &Path) -> Result<Vec<FileValidation>> {
    let temp = TempDir::new("Deckwright-Check")?;
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
