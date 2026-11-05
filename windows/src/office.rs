//! Semantic verification for packages re-saved by PowerPoint, which renames
//! parts and relationship IDs: compare ordered slide features, not ZIP paths.
use crate::package::{checksum, extension, extract, file_name, read_file, zip_dir, TempDir};
use crate::verifier::{ordered_parts, resolve_part, slide_size, FileValidation, PartFingerprint};
use crate::xml::{read_xml, Document, Element};
use crate::{fail, scaler, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct OfficePreparedFile {
    pub name: String,
    pub path: String,
    pub slides: usize,
    #[serde(rename = "macroEnabled")]
    pub macro_enabled: bool,
}
#[derive(Serialize, Deserialize)]
pub struct OfficePreparation {
    pub width: i64,
    pub height: i64,
    pub files: Vec<OfficePreparedFile>,
}

pub fn prepare(inputs: &[PathBuf], destination: &Path, fit: bool) -> Result<OfficePreparation> {
    if inputs.len() < 2 || destination.exists() {
        return fail("Two inputs and a new preparation folder are required.");
    }
    let temp = TempDir::new("Deckwright-Office")?;
    let mut roots = vec![];
    let mut docs = vec![];
    let mut sizes = vec![];
    for (i, input) in inputs.iter().enumerate() {
        let e = extension(input);
        if e != "pptx" && e != "pptm" {
            return fail("Unsupported input format.");
        }
        let root = temp.path().join(format!("s{i}"));
        std::fs::create_dir_all(&root).or_else(|e| fail(e.to_string()))?;
        extract(input, &root)?;
        let doc = read_xml(&root, "ppt/presentation.xml")?;
        let Some(size) = slide_size(&doc.root).filter(|_| doc.root.ns == crate::xml::P_NS) else { return fail(format!("Cannot read the size of {}", file_name(input))) };
        if !scaler::is_widescreen(size) && !fit {
            return Err(Error::FormatChoice(format!("{} is not 16:9. Choose to fit it without enlarging, or change it yourself.", file_name(input))));
        }
        roots.push(root);
        docs.push(doc);
        sizes.push(size);
    }
    let unit = sizes.iter().map(|s| ((s.0 + 15) / 16).max((s.1 + 8) / 9)).max().unwrap_or(0);
    let target = (unit * 16, unit * 9);
    std::fs::create_dir_all(destination).or_else(|e| fail(e.to_string()))?;
    let mut files = vec![];
    let result = (|| -> Result<()> {
        for i in 0..inputs.len() {
            scaler::normalize(&roots[i], &mut docs[i], sizes[i], target, !scaler::is_widescreen(sizes[i]))?;
            let path = destination.join(format!("source{i}.{}", extension(&inputs[i])));
            zip_dir(&roots[i], &path)?;
            files.push(OfficePreparedFile { name: file_name(&inputs[i]), path: path.display().to_string(), slides: ordered_parts(&roots[i])?.len(), macro_enabled: roots[i].join("ppt").join("vbaProject.bin").exists() });
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(destination);
        return Err(e);
    }
    Ok(OfficePreparation { width: target.0, height: target.1, files })
}

fn rels_of(owner: &str) -> String {
    let (dir, name) = owner.rsplit_once('/').unwrap_or(("", owner));
    if dir.is_empty() { format!("_rels/{name}.rels") } else { format!("{dir}/_rels/{name}.rels") }
}

fn related(root: &Path, owner: &str, suffix: &str) -> Result<Option<String>> {
    let rel = rels_of(owner);
    if !crate::xml::part_path(root, &rel).exists() {
        return Ok(None);
    }
    let d = read_xml(root, &rel)?;
    let found = d.root.named("Relationship").find(|r| r.attr("Type").ends_with(suffix) && r.attr("TargetMode") != "External").map(|r| resolve_part(owner, r.attr("Target")));
    Ok(found)
}

// Office can merge/split runs while leaving the paragraph text unchanged.
// Slide-number placeholders are computed fields, not presenter notes.
fn semantic_text(document: &Document, notes: bool) -> Vec<String> {
    let mut result = vec![];
    fn walk(e: &Element, computed: bool, notes: bool, result: &mut Vec<String>) {
        let mut computed = computed;
        if notes && e.local() == "sp" {
            let mut slide_number = false;
            e.visit(&mut |x| if x.local() == "ph" && x.attr("type") == "sldNum" { slide_number = true });
            computed = computed || slide_number;
        }
        if e.local() == "p" && !computed {
            let mut text = String::new();
            e.visit(&mut |x| if x.local() == "t" { text.push_str(&x.text()) });
            if !text.is_empty() {
                result.push(text);
            }
        }
        for c in e.elems() {
            walk(c, computed, notes, result);
        }
    }
    walk(&document.root, false, notes, &mut result);
    result
}

fn notes_text(root: &Path, slide: &str) -> Result<Vec<String>> {
    match related(root, slide, "/notesSlide")? {
        Some(part) => Ok(semantic_text(&read_xml(root, &part)?, true)),
        None => Ok(vec![]),
    }
}

fn media(root: &Path, slides: &[String]) -> Result<HashSet<String>> {
    let mut seen = HashSet::new();
    let mut pending: Vec<String> = slides.to_vec();
    let mut hashes = HashSet::new();
    while let Some(owner) = pending.pop() {
        if !seen.insert(owner.clone()) {
            continue;
        }
        let path = crate::xml::part_path(root, &owner);
        if !path.exists() {
            return fail(format!("Broken internal relationship: {owner}"));
        }
        let lower = owner.to_lowercase();
        if lower.contains("/media/") || lower.contains("/embeddings/") {
            hashes.insert(checksum(&read_file(&path)?));
        }
        let rel = rels_of(&owner);
        if crate::xml::part_path(root, &rel).exists() {
            // A notes slide points back to its slide; `seen` handles cycles.
            for r in read_xml(root, &rel)?.root.named("Relationship").filter(|r| r.attr("TargetMode") != "External") {
                pending.push(resolve_part(&owner, r.attr("Target")));
            }
        }
    }
    Ok(hashes)
}

pub fn verify(inputs: &[PathBuf], output: &Path) -> Result<Vec<FileValidation>> {
    let temp = TempDir::new("Deckwright-OfficeCheck")?;
    let merged = temp.path().join("merged");
    std::fs::create_dir_all(&merged).or_else(|e| fail(e.to_string()))?;
    extract(output, &merged)?;
    let output_slides = ordered_parts(&merged)?;
    let mut results = vec![];
    let mut offset = 0;
    for (i, input) in inputs.iter().enumerate() {
        let source = temp.path().join(format!("s{i}"));
        std::fs::create_dir_all(&source).or_else(|e| fail(e.to_string()))?;
        extract(input, &source)?;
        let originals = ordered_parts(&source)?;
        let imported: Vec<String> = output_slides.iter().skip(offset).take(originals.len()).cloned().collect();
        let mut issues = vec![];
        let mut animated = 0;
        if imported.len() != originals.len() {
            issues.push("Wrong slide count.".to_string());
        }
        for j in 0..originals.len().min(imported.len()) {
            let a_doc = read_xml(&source, &originals[j])?;
            let b_doc = read_xml(&merged, &imported[j])?;
            let before = PartFingerprint::new(&a_doc.root);
            let after = PartFingerprint::new(&b_doc.root);
            if before.timing != "ABSENT" {
                animated += 1;
            }
            let n = j + 1;
            if semantic_text(&a_doc, false) != semantic_text(&b_doc, false) {
                issues.push(format!("Slide {n}: text/order differs."));
            }
            if before.timing != after.timing {
                issues.push(format!("Slide {n}: animation/timing missing or different. PowerPoint may also have reassigned identifiers; review needed."));
            }
            if before.transition != after.transition {
                issues.push(format!("Slide {n}: transition/advance missing or different."));
            }
            if notes_text(&source, &originals[j])? != notes_text(&merged, &imported[j])? {
                issues.push(format!("Slide {n}: notes text differs."));
            }
            for suffix in ["/slideLayout", "/slideMaster"] {
                let mut a = related(&source, &originals[j], "/slideLayout")?;
                let mut b = related(&merged, &imported[j], "/slideLayout")?;
                if suffix == "/slideMaster" {
                    a = match a { Some(x) => related(&source, &x, suffix)?, None => None };
                    b = match b { Some(x) => related(&merged, &x, suffix)?, None => None };
                }
                match (a, b) {
                    (Some(a), Some(b)) => {
                        let af = PartFingerprint::new(&read_xml(&source, &a)?.root);
                        let bf = PartFingerprint::new(&read_xml(&merged, &b)?.root);
                        if af.timing != bf.timing || af.transition != bf.transition {
                            issues.push(format!("Slide {n}: master or layout animation/transition differs."));
                        }
                    }
                    (Some(_), None) => issues.push(format!("Slide {n}: source master/layout missing.")),
                    _ => {}
                }
            }
        }
        let original_media = media(&source, &originals)?;
        let imported_media = media(&merged, &imported)?;
        if !original_media.is_subset(&imported_media) {
            issues.push("Media/embedded objects identical to the originals are missing (or PowerPoint transformed them).".into());
        }
        let size = read_xml(&source, "ppt/presentation.xml")?;
        let out_size = read_xml(&merged, "ppt/presentation.xml")?;
        if let (Some(a), Some(b)) = (size.root.child("sldSz"), out_size.root.child("sldSz")) {
            if a.attr("cx") != b.attr("cx") || a.attr("cy") != b.attr("cy") {
                issues.push("The output size does not match the prepared copies.".into());
            }
        }
        results.push(FileValidation {
            file: file_name(input),
            status: if issues.is_empty() { "structural_checks_passed_needs_review" } else { "import_failed_validation" }.into(),
            checked_slides: imported.len(),
            slides_with_animation: animated,
            binary_resources_checked: original_media.len(),
            checks: vec!["Slides in order, text, animations, transitions, notes, size and reachable resources".into()],
            issues,
            needs_review: vec!["Import with PowerPoint source formatting. Appearance, fonts, editing and playback are not certified; macros from added files are not merged.".into(), "PowerPoint identifiers can change and cause differences that need review. This is not binary equivalence.".into()],
        });
        offset += originals.len();
    }
    if offset != output_slides.len() {
        if let Some(last) = results.last_mut() {
            last.status = "import_failed_validation".into();
            last.issues.push("Wrong total count.".into());
        }
    }
    Ok(results)
}
