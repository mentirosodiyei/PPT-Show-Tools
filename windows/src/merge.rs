//! Open XML merge: each imported package is namespaced intact under
//! pptMerger/sourceN/, so its masters, themes and relative links survive.
use crate::package::{copy_new, copy_tree, extension, extract, file_name, package_files, with_suffix, write_new, zip_dir, TempDir};
use crate::verifier::{slide_size, standardize, verify, FileValidation, PackageSnapshot};
use crate::xml::{read_xml, write_xml, Element, CT_NS, PKG_NS, P_NS, R_NS};
use crate::{fail, scaler, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const MIN_SLIDE_EMU: i64 = 914400;
pub const MAX_SLIDE_EMU: i64 = 51206400;

pub fn require_valid_slide_size(size: (i64, i64)) -> Result<()> {
    let ok = |v: i64| (MIN_SLIDE_EMU..=MAX_SLIDE_EMU).contains(&v);
    if ok(size.0) && ok(size.1) {
        Ok(())
    } else {
        fail(format!("The resulting slide size ({} × {} EMU) is outside PowerPoint's limits (1–56 in per side).", size.0, size.1))
    }
}

/// A source deck's own size, checked before any arithmetic on it.
pub fn require_source_size(size: (i64, i64), name: &str) -> Result<()> {
    let ok = |v: i64| (MIN_SLIDE_EMU..=MAX_SLIDE_EMU).contains(&v);
    if ok(size.0) && ok(size.1) {
        Ok(())
    } else {
        fail(format!("“{name}” has a slide size outside PowerPoint's limits (1–56 in per side)."))
    }
}

/// PowerPoint measures pixels at 96 per inch: 1 px = 9525 EMU.
pub const EMU_PER_PIXEL: i64 = 9525;

/// "1280 × 720 px (33.87 × 19.05 cm)"; Open XML stores 360,000 EMU per centimetre.
pub fn describe_size(size: (i64, i64)) -> String {
    format!("{:.0} × {:.0} px ({:.2} × {:.2} cm)", size.0 as f64 / EMU_PER_PIXEL as f64, size.1 as f64 / EMU_PER_PIXEL as f64, size.0 as f64 / 360000.0, size.1 as f64 / 360000.0)
}

/// Output canvas and, per source, whether it may only shrink (fit with margins)
/// or is scaled proportionally to the canvas.
pub struct SlidePlan {
    pub target: (i64, i64),
    pub fit_without_enlarging: Vec<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MergeReport {
    pub engine: String,
    pub input_files: Vec<String>,
    pub slide_counts: Vec<usize>,
    pub total_slides: usize,
    pub warnings: Vec<String>,
    pub power_point_playback_validated: bool,
    pub validation: Vec<FileValidation>,
    pub overall_status: String,
    pub validation_scope: String,
}
impl MergeReport {
    pub fn structural_validation_passed(&self) -> bool {
        self.validation.iter().all(|v| v.issues.is_empty())
    }
}

/// `fixed` is the size forced in Settings; None means automatic: a size shared by
/// every source (or their common proportion, at the largest size) is kept;
/// mixed proportions fall back to 16:9, fitting the others only when allowed.
pub fn plan_slide_size(sizes: &[(i64, i64)], fixed: Option<(i64, i64)>, fit_non_matching: bool, names: &[String]) -> Result<SlidePlan> {
    let plan = if let Some(target) = fixed {
        SlidePlan { target, fit_without_enlarging: vec![false; sizes.len()] }
    } else if !sizes.is_empty() && sizes.iter().all(|s| scaler::same_aspect(*s, sizes[0])) {
        let largest = *sizes.iter().max_by_key(|s| s.0 as i128 * s.1 as i128).unwrap();
        SlidePlan { target: largest, fit_without_enlarging: vec![false; sizes.len()] }
    } else {
        let invalid: Vec<usize> = (0..sizes.len()).filter(|&i| !scaler::is_widescreen(sizes[i])).collect();
        if !invalid.is_empty() && !fit_non_matching {
            let details: Vec<String> = invalid.iter().map(|&i| format!("• File {}: “{}” — {}.", i + 1, names[i], describe_size(sizes[i]))).collect();
            return Err(Error::FormatChoice(format!("These files are not 16:9:\n\n{}\n\nYou can fit them centred without enlarging or cropping, choose an output size in Settings, or stop and change them yourself. The originals are not modified.", details.join("\n"))));
        }
        // Smallest exact 16:9 canvas at least as wide AND as high as every source.
        let unit = sizes.iter().map(|s| ((s.0 + 15) / 16).max((s.1 + 8) / 9)).max().unwrap_or(0);
        SlidePlan { target: (unit * 16, unit * 9), fit_without_enlarging: sizes.iter().map(|s| !scaler::is_widescreen(*s)).collect() }
    };
    require_valid_slide_size(plan.target)?;
    Ok(plan)
}

// Sequence order of children in CT_Presentation (ISO/IEC 29500-1 §19.2.1.26)
const PRESENTATION_CHILD_ORDER: [&str; 15] = ["sldMasterIdLst", "notesMasterIdLst", "handoutMasterIdLst", "sldIdLst", "sldSz", "notesSz", "smartTags", "embeddedFontLst", "custShowLst", "photoAlbum", "custDataLst", "kinsoku", "defaultTextStyle", "modifyVerifier", "extLst"];

fn insert_in_presentation_order(root: &mut Element, node: Element) {
    let rank = |l: &str| PRESENTATION_CHILD_ORDER.iter().position(|x| *x == l);
    let Some(target) = rank(node.local()) else {
        root.add(node);
        return;
    };
    let position = root.children.iter().position(|c| matches!(c, crate::xml::Node::Elem(e) if rank(e.local()).map(|r| r > target).unwrap_or(false)));
    match position {
        Some(i) => root.children.insert(i, crate::xml::Node::Elem(node)),
        None => root.add(node),
    }
}

fn mkdir(p: &Path) -> Result<()> {
    std::fs::create_dir_all(p).or_else(|e| fail(format!("Cannot create {}: {e}", p.display())))
}

fn part_of(target: &str) -> String {
    if let Some(rest) = target.strip_prefix('/') {
        rest.to_string()
    } else {
        standardize(&format!("/ppt/{target}")).trim_start_matches('/').to_string()
    }
}

pub fn merge(inputs: &[PathBuf], output: &Path, fit_non_widescreen: bool, fixed: Option<(i64, i64)>) -> Result<MergeReport> {
    if inputs.len() < 2 {
        return fail("Add at least two presentations.");
    }
    let out_ext = extension(output);
    if out_ext != "pptx" && out_ext != "pptm" {
        return fail("The output must be PPTX or PPTM.");
    }
    if output.exists() || with_suffix(output, ".report.json").exists() || with_suffix(output, ".report.txt").exists() {
        return fail("The output file or its report already exists. Choose another name.");
    }
    for input in inputs {
        let e = extension(input);
        if (e != "pptx" && e != "pptm") || crate::package::same_file(input, output) {
            return fail("Invalid input.");
        }
    }
    let temp = TempDir::new("Deckwright")?;
    let mut roots = vec![];
    for (i, input) in inputs.iter().enumerate() {
        let dir = temp.path().join(format!("input{i}"));
        mkdir(&dir)?;
        extract(input, &dir)?;
        roots.push(dir);
    }
    for dir in &roots {
        let trash = dir.join("[trash]");
        if trash.exists() {
            std::fs::remove_dir_all(&trash).or_else(|e| fail(e.to_string()))?;
        }
    }
    let mut docs = roots.iter().map(|r| read_xml(r, "ppt/presentation.xml")).collect::<Result<Vec<_>>>()?;
    if !docs.iter().all(|d| d.root.ns == P_NS) {
        return fail("Only PowerPoint Open XML (Transitional) is supported.");
    }
    let mut sizes = vec![];
    for (i, d) in docs.iter().enumerate() {
        match slide_size(&d.root) {
            Some(s) => {
                require_source_size(s, &file_name(&inputs[i]))?;
                sizes.push(s)
            }
            None => return fail(format!("Cannot read the slide size of file {}: “{}”.", i + 1, file_name(&inputs[i]))),
        }
    }
    let originals = roots.iter().map(|r| PackageSnapshot::capture(r)).collect::<Result<Vec<_>>>()?;
    let names: Vec<String> = inputs.iter().map(|p| file_name(p)).collect();
    let plan = plan_slide_size(&sizes, fixed, fit_non_widescreen, &names)?;
    let target = plan.target;
    let mut scaling_warnings = vec![];
    for i in 0..sizes.len() {
        if sizes[i] != target {
            let fit = plan.fit_without_enlarging[i];
            scaler::normalize(&roots[i], &mut docs[i], sizes[i], target, fit)?;
            scaling_warnings.push(format!("{}: {} → {}; {}.", names[i], describe_size(sizes[i]), describe_size(target), if fit { "centred without enlarging or cropping" } else { "scaled proportionally and centred" }));
        }
    }
    let counts: Vec<usize> = docs.iter().map(|d| d.root.child("sldIdLst").map(|l| l.named("sldId").count()).unwrap_or(0)).collect();
    if !counts.iter().all(|c| *c > 0) {
        return fail("One of the presentations has no slides.");
    }
    let base = roots[0].clone();
    let mut rels = read_xml(&base, "ppt/_rels/presentation.xml.rels")?;
    let mut ct = read_xml(&base, "[Content_Types].xml")?;
    let mut warnings = vec!["Direct Open XML engine (PowerPoint is not opened).".to_string(), "Check playback, editing, notes, fonts, external links and OLE objects in PowerPoint.".to_string()];
    warnings.extend(scaling_warnings);
    // ST_SlideId allows 256…2147483647; reject anything that would overflow.
    let highest_slide = docs[0].root.child("sldIdLst").and_then(|l| l.named("sldId").filter_map(|e| e.attr("id").parse::<u64>().ok()).max()).unwrap_or(255);
    if highest_slide >= 2147483647 {
        return fail("Too many slide identifiers to create a valid presentation.");
    }
    let mut next_slide: u64 = highest_slide + 1;
    let mut next_master: u64 = 2147483648;
    for d in &docs {
        if let Some(m) = d.root.child("sldMasterIdLst") {
            for e in m.named("sldMasterId") {
                if let Ok(id) = e.attr("id").parse::<u64>() {
                    next_master = next_master.max(id.min(u32::MAX as u64) + 1);
                }
            }
        }
    }
    // Master and layout IDs share a presentation-wide identifier space.
    for dir in &roots {
        for part in package_files(&dir.join("ppt").join("slideMasters")).iter().filter(|p| p.ends_with(".xml")) {
            let d = read_xml(&dir.join("ppt").join("slideMasters"), part)?;
            for e in d.root.descendants("sldLayoutId") {
                if let Ok(id) = e.attr("id").parse::<u64>() {
                    next_master = next_master.max(id.min(u32::MAX as u64) + 1);
                }
            }
        }
    }
    let mut rel_number = 0;
    let mut add_rel = |rels: &mut crate::xml::Document, kind: &str, target: &str| -> String {
        let mut id;
        loop {
            rel_number += 1;
            id = format!("pptMerger{rel_number}");
            if !rels.root.named("Relationship").any(|e| e.attr("Id") == id) {
                break;
            }
        }
        let mut e = Element::new("Relationship", PKG_NS);
        if let Some(prefix) = rels.root.qname.split_once(':').map(|(p, _)| p.to_string()) {
            e.qname = format!("{prefix}:Relationship");
        }
        e.set_attr("Id", &id);
        e.set_attr("Type", kind);
        e.set_attr("Target", target);
        rels.root.add(e);
        id
    };
    let mut notes_target: Option<String> = rels.root.named("Relationship").find(|e| e.attr("Type").ends_with("/notesMaster")).map(|e| {
        let t = e.attr("Target");
        if t.starts_with('/') { t.to_string() } else { format!("/ppt/{t}") }
    });
    warnings.push("All notes use the first notes master available. Each slide's notes content is kept; inherited notes formatting may change.".into());
    let (base_doc, other_docs) = docs.split_at_mut(1);
    let pres = &mut base_doc[0];
    let p_prefix = pres.prefix_for(P_NS, "p");
    let r_prefix = pres.prefix_for(R_NS, "r");
    let ct_prefix = ct.root.qname.split_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
    for i in 1..roots.len() {
        let source = &roots[i];
        let source_doc = &other_docs[i - 1];
        let prefix = format!("pptMerger/source{i}/");
        if notes_target.is_none() {
            let sr = read_xml(source, "ppt/_rels/presentation.xml.rels")?;
            let found = sr.root.named("Relationship").find(|e| e.attr("Type").ends_with("/notesMaster")).cloned();
            if let Some(e) = found {
                let old = e.attr("Target");
                let part = if let Some(r) = old.strip_prefix('/') { r.to_string() } else { format!("ppt/{old}") };
                let target = format!("/{prefix}{part}");
                let new_id = add_rel(&mut rels, e.attr("Type"), &target);
                notes_target = Some(target);
                let mut list = Element::new(&format!("{p_prefix}:notesMasterIdLst"), P_NS);
                let mut item = Element::new(&format!("{p_prefix}:notesMasterId"), P_NS);
                item.set_attr_ns(R_NS, "id", &r_prefix, &new_id);
                list.add(item);
                insert_in_presentation_order(&mut pres.root, list);
            }
        }
        let dest = crate::xml::part_path(&base, &prefix);
        copy_tree(source, &dest)?;
        for part in package_files(&dest).iter().filter(|p| p.ends_with(".rels")) {
            let mut d = read_xml(&dest, part)?;
            for e in d.root.elems_mut().filter(|e| e.local() == "Relationship") {
                if e.attr("TargetMode") == "External" {
                    continue;
                }
                if e.attr("Type").ends_with("/notesMaster") && notes_target.is_some() {
                    e.set_attr("Target", notes_target.as_deref().unwrap_or(""));
                } else if let Some(rest) = e.attr("Target").strip_prefix('/').map(str::to_string) {
                    e.set_attr("Target", &format!("/{prefix}{rest}"));
                }
            }
            write_xml(&d, &dest, part)?;
        }
        let masters = dest.join("ppt").join("slideMasters");
        for part in package_files(&masters).iter().filter(|p| p.ends_with(".xml")) {
            let mut d = read_xml(&masters, part)?;
            d.root.visit_mut(&mut |e| {
                if e.local() == "sldLayoutId" {
                    e.set_attr("id", &next_master.to_string());
                    next_master += 1;
                }
            });
            write_xml(&d, &masters, part)?;
        }
        let src_ct = read_xml(source, "[Content_Types].xml")?;
        // OPC default extensions are case-insensitive; part names keep their case.
        let mut defaults: HashMap<String, String> = HashMap::new();
        for entry in src_ct.root.named("Default") {
            let ext = entry.attr("Extension").to_lowercase();
            let kind = entry.attr("ContentType").to_string();
            if let Some(existing) = defaults.get(&ext) {
                if *existing != kind {
                    return fail(format!("Conflicting content types for extension .{ext} in “{}”.", file_name(&inputs[i])));
                }
            }
            defaults.insert(ext, kind);
        }
        // Explicit types prevent conflicting default extensions across packages.
        let overrides: HashMap<String, String> = src_ct.root.named("Override").map(|e| (e.attr("PartName").trim_start_matches('/').to_string(), e.attr("ContentType").to_string())).collect();
        for part in package_files(source) {
            if part == "[Content_Types].xml" || part.ends_with(".rels") {
                continue;
            }
            let name = part.rsplit('/').next().unwrap_or("");
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
            let Some(kind) = overrides.get(&part).or_else(|| defaults.get(&ext)) else { return fail(format!("Missing content type: {part}")) };
            let mut e = Element::new(&format!("{ct_prefix}Override"), CT_NS);
            e.set_attr("PartName", &format!("/{prefix}{part}"));
            e.set_attr("ContentType", kind);
            ct.root.add(e);
        }
        // The imported package's root metadata is not a second OPC root.
        std::fs::remove_file(dest.join("[Content_Types].xml")).or_else(|e| fail(e.to_string()))?;
        let src_rels = read_xml(source, "ppt/_rels/presentation.xml.rels")?;
        let by_id: HashMap<String, &Element> = src_rels.root.named("Relationship").map(|e| (e.attr("Id").to_string(), e)).collect();
        for (list_name, child_name) in [("sldMasterIdLst", "sldMasterId"), ("sldIdLst", "sldId")] {
            let Some(source_list) = source_doc.root.child(list_name) else { continue };
            if pres.root.child(list_name).is_none() {
                insert_in_presentation_order(&mut pres.root, Element::new(&format!("{p_prefix}:{list_name}"), P_NS));
            }
            let mut imported = vec![];
            for e in source_list.named(child_name) {
                let Some(rel) = by_id.get(e.rid()) else { return fail("Missing presentation relationship") };
                let new_rid = add_rel(&mut rels, rel.attr("Type"), &format!("/{prefix}{}", part_of(rel.attr("Target"))));
                let mut node = e.clone();
                node.set_attr_ns(R_NS, "id", &r_prefix, &new_rid);
                if child_name == "sldId" {
                    node.set_attr("id", &next_slide.to_string());
                    next_slide += 1;
                } else {
                    node.set_attr("id", &next_master.to_string());
                    next_master += 1;
                }
                imported.push(node);
            }
            let target_list = pres.root.child_mut(list_name).expect("list inserted above");
            for node in imported {
                target_list.add(node);
            }
        }
        // Embedded fonts belong to the presentation; copying their bytes alone
        // does not make them available to PowerPoint.
        if let Some(source_fonts) = source_doc.root.child("embeddedFontLst") {
            if pres.root.child("embeddedFontLst").is_none() {
                insert_in_presentation_order(&mut pres.root, Element::new(&format!("{p_prefix}:embeddedFontLst"), P_NS));
            }
            let mut fonts = vec![];
            for font in source_fonts.named("embeddedFont") {
                let mut node = font.clone();
                for style in node.elems_mut() {
                    let id = style.rid().to_string();
                    if id.is_empty() {
                        continue;
                    }
                    let rel = by_id.get(&id).filter(|r| r.attr("Type").ends_with("/font") && r.attr("TargetMode") != "External");
                    let Some(rel) = rel else { return fail(format!("Missing or invalid embedded font relationship in “{}”.", file_name(&inputs[i]))) };
                    let new_id = add_rel(&mut rels, rel.attr("Type"), &format!("/{prefix}{}", part_of(rel.attr("Target"))));
                    style.set_attr_ns(R_NS, "id", &r_prefix, &new_id);
                }
                fonts.push(node);
            }
            let target = pres.root.child_mut("embeddedFontLst").expect("list inserted above");
            for f in fonts {
                target.add(f);
            }
            warnings.push(format!("{}: embedded fonts registered in the result. Check substitutions, usage rights and different variants with the same name.", file_name(&inputs[i])));
        }
        if source.join("ppt").join("vbaProject.bin").exists() {
            warnings.push(format!("{}: VBA kept as an inactive resource; not merged into the output VBA project.", file_name(&inputs[i])));
        }
    }
    let has_macros = base.join("ppt").join("vbaProject.bin").exists();
    if out_ext == "pptx" && has_macros {
        return fail("The first file contains VBA: save as PPTM to keep it.");
    }
    for e in ct.root.elems_mut().filter(|e| e.local() == "Override" && e.attr("PartName") == "/ppt/presentation.xml") {
        e.set_attr("ContentType", if out_ext == "pptm" { "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml" } else { "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml" });
    }
    // Only one presentation root is valid; imported presentation metadata is unused.
    for i in 1..roots.len() {
        let prefix = format!("pptMerger/source{i}/");
        for part in ["ppt/presentation.xml", "ppt/_rels/presentation.xml.rels", "_rels/.rels"] {
            let path = crate::xml::part_path(&base, &format!("{prefix}{part}"));
            if path.exists() {
                std::fs::remove_file(&path).or_else(|e| fail(e.to_string()))?;
            }
            let name = format!("/{prefix}{part}");
            ct.root.children.retain(|n| !matches!(n, crate::xml::Node::Elem(e) if e.local() == "Override" && e.attr("PartName") == name));
        }
    }
    // ST_SlideId allows 256…2147483647.
    if next_slide > 2147483648 || next_master > 4294967296 {
        return fail("Too many slide identifiers to create a valid presentation.");
    }
    write_xml(pres, &base, "ppt/presentation.xml")?;
    write_xml(&rels, &base, "ppt/_rels/presentation.xml.rels")?;
    write_xml(&ct, &base, "[Content_Types].xml")?;
    let archive = temp.path().join("result.zip");
    zip_dir(&base, &archive)?;
    let verification = temp.path().join("verify");
    mkdir(&verification)?;
    extract(&archive, &verification)?;
    let validation = verify(&originals, inputs, &verification, Some(&plan.fit_without_enlarging))?;
    let passed = validation.iter().all(|v| v.issues.is_empty());
    let report = MergeReport {
        engine: "Deckwright-OpenXML-v0.7.4-windows".into(),
        input_files: inputs.iter().map(|p| p.display().to_string()).collect(),
        total_slides: counts.iter().sum(),
        slide_counts: counts,
        warnings: warnings.clone(),
        power_point_playback_validated: false,
        validation,
        overall_status: if passed { "structural_checks_passed_needs_review" } else { "import_failed_validation" }.into(),
        validation_scope: "Structural comparisons against each original; visual rendering, playback and editing not validated.".into(),
    };
    copy_new(&archive, output)?;
    let written = (|| -> Result<()> {
        write_new(&with_suffix(output, ".report.json"), &crate::to_json(&report)?)?;
        let mut lines = vec!["DECKWRIGHT — CHECKS PER FILE".to_string(), if passed { "Structural checks passed. Visual and playback review pending." } else { "IMPORT NOT CORRECT: differences found." }.into(), String::new(), format!("Output size: {}", describe_size(target)), String::new()];
        for (i, check) in report.validation.iter().enumerate() {
            lines.push(format!("{}. {}", i + 1, check.file));
            lines.push(if check.issues.is_empty() { "STRUCTURE: checks passed; this is not full fidelity." } else { "IMPORT NOT CORRECT" }.into());
            lines.push(format!("{} slides; {} with animation; {} resources checked.", check.checked_slides, check.slides_with_animation, check.binary_resources_checked));
            lines.extend(check.checks.iter().map(|c| format!("Checked: {c}")));
            lines.extend(check.issues.iter().map(|c| format!("ERROR: {c}")));
            lines.extend(check.needs_review.iter().map(|c| format!("TO REVIEW: {c}")));
            lines.push(String::new());
        }
        lines.push("WARNINGS".into());
        lines.extend(warnings);
        write_new(&with_suffix(output, ".report.txt"), lines.join("\r\n").as_bytes())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_file(with_suffix(output, ".report.json"));
        return Err(e);
    }
    Ok(report)
}
