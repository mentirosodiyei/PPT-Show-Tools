//! Converts physical DrawingML dimensions, never percentages, IDs, rotations,
//! image crops, animation durations or custom-geometry coordinate systems.
use crate::xml::{read_xml, write_xml, Document, Element, Node, A_NS, P_NS};
use crate::Result;
use std::path::Path;

pub fn is_widescreen(size: (i64, i64)) -> bool {
    (size.0 as f64 / size.1 as f64 / (16.0 / 9.0) - 1.0).abs() <= 0.001
}

fn xml_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(items) = std::fs::read_dir(dir) else { return };
    for item in items.flatten() {
        let p = item.path();
        if p.is_dir() {
            xml_files(&p, out);
        } else if p.extension().map(|e| e == "xml").unwrap_or(false) {
            out.push(p);
        }
    }
}

pub fn normalize(package: &Path, presentation: &mut Document, from: (i64, i64), to: (i64, i64), fit_without_enlarging: bool) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let raw = (to.0 as f64 / from.0 as f64).min(to.1 as f64 / from.1 as f64);
    let factor = if fit_without_enlarging { raw.min(1.0) } else { raw };
    let shift_x = (to.0 as f64 - from.0 as f64 * factor) / 2.0;
    let shift_y = (to.1 as f64 - from.1 as f64 * factor) / 2.0;
    let unit = (factor - 1.0).abs() < 1e-9;
    let rx = factor * from.0 as f64 / to.0 as f64;
    let ry = factor * from.1 as f64 / to.1 as f64;
    // Notes and handouts use their own page coordinates; media bytes and
    // embedded workbooks are left untouched.
    for folder in ["slides", "slideLayouts", "slideMasters", "theme", "charts"] {
        let mut files = vec![];
        xml_files(&package.join("ppt").join(folder), &mut files);
        files.sort();
        for file in files {
            let rel = file.strip_prefix(package).unwrap_or(&file).to_string_lossy().replace('\\', "/");
            let mut doc = read_xml(package, &rel)?;
            let is_master = folder == "slideMasters" || folder == "theme";
            if !unit {
                scale_node(&mut doc.root, factor, is_master, "");
            }
            if ["slides", "slideLayouts", "slideMasters"].contains(&folder) {
                if let Some(tree) = doc.root.first_descendant_mut("spTree") {
                    translate_container(tree, shift_x, shift_y);
                }
                scale_timing_animations(&mut doc.root, rx, ry, shift_x / to.0 as f64, shift_y / to.1 as f64);
            }
            write_xml(&doc, package, &rel)?;
        }
    }
    if !unit {
        if let Some(style) = presentation.root.child_mut("defaultTextStyle") {
            scale_node(style, factor, true, "presentation");
        }
    }
    if let Some(size) = presentation.root.child_mut("sldSz") {
        size.set_attr("cx", &to.0.to_string());
        size.set_attr("cy", &to.1.to_string());
        // A type such as screen4x3 would contradict the new size.
        size.remove_attr("type");
    }
    write_xml(presentation, package, "ppt/presentation.xml")
}

fn translate_container(container: &mut Element, dx: f64, dy: f64) {
    for shape in container.elems_mut() {
        let local = shape.local().to_string();
        if local == "AlternateContent" {
            for branch in shape.elems_mut() {
                if branch.local() == "Choice" || branch.local() == "Fallback" {
                    translate_container(branch, dx, dy);
                }
            }
            continue;
        }
        if !["sp", "pic", "grpSp", "cxnSp", "graphicFrame", "contentPart"].contains(&local.as_str()) {
            continue;
        }
        let holder: Option<&mut Element> = if local == "graphicFrame" || local == "contentPart" {
            Some(shape)
        } else if shape.child("spPr").is_some() {
            shape.child_mut("spPr")
        } else {
            shape.child_mut("grpSpPr")
        };
        if let Some(offset) = holder.and_then(|h| h.child_mut("xfrm")).and_then(|t| t.child_mut("off")) {
            for (key, delta) in [("x", dx), ("y", dy)] {
                if let Some(v) = offset.attr_mut(key) {
                    if let Ok(n) = v.parse::<f64>() {
                        *v = ((n + delta).round() as i64).to_string();
                    }
                }
            }
        }
    }
}

pub fn scale_timing_animations(root: &mut Element, rx: f64, ry: f64, sx: f64, sy: f64) {
    if (rx - 1.0).abs() <= 1e-9 && (ry - 1.0).abs() <= 1e-9 {
        return;
    }
    // Motion paths (origin="parent" absolute commands also shift).
    for_each_descendant_mut(root, "animMotion", &mut |motion| {
        let origin_parent = motion.attr("origin") == "parent";
        if let Some(path) = motion.attr_mut("path") {
            if !path.is_empty() {
                *path = scale_motion_path(path, rx, ry, sx, sy, origin_parent);
            }
        }
    });
    for_each_descendant_mut(root, "anim", &mut |anim| {
        let names: Vec<String> = anim.descendants("attrName").iter().map(|e| e.text().trim().to_string()).collect();
        let has = |a: &str, b: &str| names.iter().any(|n| n == a || n == b);
        let (is_x, is_y, is_w, is_h) = (has("ppt_x", "style.left"), has("ppt_y", "style.top"), has("ppt_w", "style.width"), has("ppt_h", "style.height"));
        for_each_descendant_mut(anim, "tav", &mut |tav| {
            let has_formula = tav.has_attr("fmla");
            if let Some(f) = tav.attr_mut("fmla") {
                let fmla = f.clone();
                if (is_x || fmla.contains("ppt_x")) && (rx - 1.0).abs() > 1e-9 {
                    *f = scale_formula_offsets(&fmla, "ppt_x", rx, sx);
                } else if (is_y || fmla.contains("ppt_y")) && (ry - 1.0).abs() > 1e-9 {
                    *f = scale_formula_offsets(&fmla, "ppt_y", ry, sy);
                } else if (is_w || fmla.contains("ppt_w")) && (rx - 1.0).abs() > 1e-9 {
                    *f = scale_formula_offsets(&fmla, "ppt_w", rx, 0.0);
                } else if (is_h || fmla.contains("ppt_h")) && (ry - 1.0).abs() > 1e-9 {
                    *f = scale_formula_offsets(&fmla, "ppt_h", ry, 0.0);
                }
            }
            if !has_formula {
                for_each_descendant_mut(tav, "fltVal", &mut |flt| {
                    if let Some(v) = flt.attr_mut("val") {
                        if let Ok(n) = v.parse::<f64>() {
                            if is_x {
                                *v = format_coord(n * rx + sx)
                            } else if is_y {
                                *v = format_coord(n * ry + sy)
                            } else if is_w {
                                *v = format_coord(n * rx)
                            } else if is_h {
                                *v = format_coord(n * ry)
                            }
                        }
                    }
                });
            }
        });
    });
}

/// Calls `f` on every descendant (not `e` itself) named `local`, outermost first,
/// without descending into a matched element (its own matches are handled by `f`).
fn for_each_descendant_mut(e: &mut Element, local: &str, f: &mut dyn FnMut(&mut Element)) {
    for c in e.elems_mut() {
        if c.local() == local {
            f(c);
        } else {
            for_each_descendant_mut(c, local, f);
        }
    }
}

pub fn scale_formula_offsets(fmla: &str, variable: &str, factor: f64, shift: f64) -> String {
    let trimmed = fmla.trim_matches(|c| c == ' ' || c == '\t');
    if !trimmed.contains(variable) {
        return if shift.abs() > 1e-9 { format!("({trimmed})*{}+{}", format_coord(factor), format_coord(shift)) } else { format!("({trimmed})*{}", format_coord(factor)) };
    }
    let pattern = format!(r"^({})\s*([+-])\s*([0-9]+(?:\.[0-9]+)?)$", regex::escape(variable));
    if let Some(c) = regex::Regex::new(&pattern).ok().and_then(|r| r.captures(trimmed)) {
        if let Ok(n) = c[3].parse::<f64>() {
            return format!("{variable}{}{}", &c[2], format_coord(n * factor));
        }
    }
    fmla.to_string()
}

pub fn scale_motion_path(path: &str, rx: f64, ry: f64, sx: f64, sy: f64, origin_parent: bool) -> String {
    let mut tokens: Vec<String> = vec![];
    let mut current = String::new();
    for ch in path.chars() {
        if ch.is_whitespace() || ch == ',' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else if ch.is_alphabetic() && ch != 'e' && ch != 'E' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push(ch.to_string());
        } else if ch == 'e' || ch == 'E' {
            if current.chars().last().map(|l| l.is_numeric() || l == '.').unwrap_or(false) {
                current.push(ch);
            } else {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(ch.to_string());
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    let mut out = vec![];
    let mut command = 'M';
    let mut index = 0;
    for token in tokens {
        let mut chars = token.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if c.is_alphabetic() {
                command = c;
                index = 0;
                out.push(token);
                continue;
            }
        }
        if let Ok(v) = token.parse::<f64>() {
            let absolute = origin_parent && command.is_uppercase();
            let scaled = if index % 2 == 0 { v * rx + if absolute { sx } else { 0.0 } } else { v * ry + if absolute { sy } else { 0.0 } };
            out.push(format_coord(scaled));
            index += 1;
        } else {
            out.push(token);
        }
    }
    out.join(" ")
}

pub fn format_coord(value: f64) -> String {
    if value.abs() < 1e-12 {
        return "0".into();
    }
    let rounded = (value * 1_000_000.0).round() / 1_000_000.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        return (rounded.round() as i64).to_string();
    }
    let mut s = format!("{rounded:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

fn length(node: &mut Element, attribute: &str, factor: f64) {
    if let Some(v) = node.attr_mut(attribute) {
        if let Ok(n) = v.parse::<f64>() {
            if n.is_finite() {
                *v = ((n * factor).round() as i64).to_string();
            }
        }
    }
}

fn scale_node(node: &mut Element, factor: f64, is_master: bool, parent: &str) {
    if (factor - 1.0).abs() <= 1e-9 {
        return;
    }
    let local = node.local().to_string();
    if node.ns == A_NS {
        let l = local.as_str();
        if (l == "off" || l == "chOff") && parent == "xfrm" {
            length(node, "x", factor);
            length(node, "y", factor);
        }
        if (l == "ext" || l == "chExt") && parent == "xfrm" {
            length(node, "cx", factor);
            length(node, "cy", factor);
        }
        if ["rPr", "defRPr", "endParaRPr"].contains(&l) {
            for key in ["sz", "kern", "spc"] {
                length(node, key, factor);
            }
            if let Some(v) = node.attr_mut("sz") {
                if let Ok(n) = v.parse::<i64>() {
                    *v = n.clamp(100, 400000).to_string();
                }
            }
        }
        if l == "bodyPr" {
            for (key, value) in [("lIns", 91440), ("rIns", 91440), ("tIns", 45720), ("bIns", 45720)] {
                if is_master && !node.has_attr(key) {
                    node.set_attr(key, &value.to_string());
                }
                length(node, key, factor);
            }
            length(node, "spcCol", factor);
        }
        if l == "tcPr" {
            for key in ["marL", "marR", "marT", "marB"] {
                length(node, key, factor);
            }
        }
        let lvl = l.len() == 7 && l.starts_with("lvl") && l.ends_with("pPr") && l.as_bytes()[3].is_ascii_digit() && l.as_bytes()[3] != b'0';
        if l == "pPr" || lvl {
            for key in ["marL", "marR", "indent", "defTabSz"] {
                length(node, key, factor);
            }
        }
        if l == "tab" {
            length(node, "pos", factor);
        }
        if l == "spcPts" || l == "buSzPts" {
            length(node, "val", factor);
        }
        if l == "gridCol" {
            length(node, "w", factor);
        }
        if l == "tr" {
            length(node, "h", factor);
        }
        if l == "ln" || l.starts_with("lnB") || ["lnL", "lnR", "lnT"].contains(&l) {
            length(node, "w", factor);
        }
        if ["outerShdw", "innerShdw", "prstShdw"].contains(&l) {
            length(node, "dist", factor);
            length(node, "blurRad", factor);
        }
        if l == "glow" || l == "softEdge" {
            length(node, "rad", factor);
        }
        if l == "blur" || l == "reflection" {
            length(node, "rad", factor);
            length(node, "blurRad", factor);
            length(node, "dist", factor);
        }
        if l == "bevelT" || l == "bevelB" {
            length(node, "w", factor);
            length(node, "h", factor);
        }
        if l == "sp3d" {
            length(node, "z", factor);
            length(node, "extrusionH", factor);
            length(node, "contourW", factor);
        }
    }
    if node.ns == P_NS && local == "oleObj" {
        length(node, "imgW", factor);
        length(node, "imgH", factor);
    }
    for child in node.children.iter_mut() {
        if let Node::Elem(e) = child {
            scale_node(e, factor, is_master, &local);
        }
    }
}
