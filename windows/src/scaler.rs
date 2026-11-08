//! Adapts slides to a new size. Converts physical DrawingML dimensions, never
//! percentages, IDs, rotations, animation timings or custom-geometry coordinates.
use crate::xml::{read_xml, write_xml, Document, Element, Node, A_NS, P_NS};
use crate::Result;
use std::path::Path;

pub fn is_widescreen(size: (i64, i64)) -> bool {
    (size.0 as f64 / size.1 as f64 / (16.0 / 9.0) - 1.0).abs() <= 0.001
}

pub fn same_aspect(a: (i64, i64), b: (i64, i64)) -> bool {
    ((a.0 as f64 / a.1 as f64) / (b.0 as f64 / b.1 as f64) - 1.0).abs() <= 0.001
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

/// Resizes every slide, layout and master without changing the design: the
/// whole page is scaled by s = min(sx, sy), the factor that fits the old page
/// in the new one, and centred; nothing moves relative to anything else and
/// nothing is stretched or cropped. Only the background grows into the new
/// margins: plain and gradient backgrounds cover them by themselves, picture
/// backgrounds and pictures filling the page are extended with their own edge
/// colours (see `padding`), and videos filling the page get a blurred copy
/// behind them (see `video`).
/// When the proportion does not change this is plain proportional scaling.
/// Animation coordinates are mapped into the centred content (see `scale_timing_animations`).
/// Returns notes for the report (videos that got a still instead of a moving background).
pub fn normalize(package: &Path, presentation: &mut Document, from: (i64, i64), to: (i64, i64)) -> Result<Vec<String>> {
    if from == to {
        return Ok(vec![]);
    }
    let mut notes = vec![];
    let page = Adaptation::new(from, to);
    let mut pad = crate::padding::Pad::default();
    // Notes and handouts use their own page coordinates; media bytes and
    // embedded workbooks are left untouched.
    for folder in ["slides", "slideLayouts", "slideMasters", "theme", "charts"] {
        let mut files = vec![];
        xml_files(&package.join("ppt").join(folder), &mut files);
        files.sort();
        for file in files {
            let rel = file.strip_prefix(package).unwrap_or(&file).to_string_lossy().replace('\\', "/");
            let mut doc = read_xml(package, &rel)?;
            let mut part = crate::padding::PartPad::new(package, &rel);
            let is_master = folder == "slideMasters" || folder == "theme";
            let layout_part = ["slides", "slideLayouts", "slideMasters"].contains(&folder);
            // Frames are read before the uniform pass rescales them.
            let mut frames = vec![];
            let mut videos = vec![];
            if layout_part {
                if let Some(tree) = doc.root.first_descendant_mut("spTree") {
                    collect_frames(tree, &mut frames);
                    if !same_aspect(from, to) {
                        videos = crate::video::spanning_videos(tree, page.width, page.height);
                    }
                }
            }
            if !page.is_unit() {
                scale_node(&mut doc.root, page.s, is_master, "");
            }
            if layout_part {
                if let Some(tree) = doc.root.first_descendant_mut("spTree") {
                    let mut next = frames.into_iter();
                    place_frames(tree, &page, &mut next, &mut |fill, sides| part.pad(&mut pad, fill, sides));
                }
                // A picture background always fills the slide: show it whole in the new proportion.
                if let Some(bg) = doc.root.first_descendant_mut("bgPr").and_then(|b| b.child_mut("blipFill")) {
                    if !part.pad(&mut pad, bg, crate::padding::centred(page.sx, page.sy)) {
                        crate::padding::inset_fill(bg, page.sx, page.sy);
                    }
                }
            }
            if layout_part {
                let (dx, dy) = page.offset();
                let (rx, ry) = (page.width * page.s / page.new_width, page.height * page.s / page.new_height);
                scale_timing_animations(&mut doc.root, rx, ry, dx / page.new_width, dy / page.new_height);
            }
            if !videos.is_empty() {
                notes.extend(crate::video::add_backdrops(package, &mut doc, &videos, (page.new_width, page.new_height), &mut part, &mut pad));
            }
            part.save()?;
            write_xml(&doc, package, &rel)?;
        }
    }
    crate::padding::register(package, &pad)?;
    if !page.is_unit() {
        if let Some(style) = presentation.root.child_mut("defaultTextStyle") {
            scale_node(style, page.s, true, "presentation");
        }
    }
    if let Some(size) = presentation.root.child_mut("sldSz") {
        size.set_attr("cx", &to.0.to_string());
        size.set_attr("cy", &to.1.to_string());
        // A type such as screen4x3 would contradict the new size.
        size.remove_attr("type");
    }
    write_xml(presentation, package, "ppt/presentation.xml")?;
    notes.sort();
    notes.dedup();
    Ok(notes)
}

/// A top-level object's frame on the original page.
#[derive(Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    cx: f64,
    cy: f64,
}

const SHAPES: [&str; 6] = ["sp", "pic", "grpSp", "cxnSp", "graphicFrame", "contentPart"];

/// The off/ext pair of a top-level object, when it has one.
fn frame_of(shape: &Element) -> Option<Frame> {
    let local = shape.local();
    let holder = if local == "graphicFrame" || local == "contentPart" { Some(shape) } else { shape.child("spPr").or_else(|| shape.child("grpSpPr")) };
    let t = holder?.child("xfrm")?;
    let (off, ext) = (t.child("off")?, t.child("ext")?);
    Some(Frame { x: off.attr("x").parse().ok()?, y: off.attr("y").parse().ok()?, cx: ext.attr("cx").parse().ok()?, cy: ext.attr("cy").parse().ok()? })
}

fn collect_frames(container: &Element, frames: &mut Vec<Option<Frame>>) {
    for shape in container.elems() {
        if shape.local() == "AlternateContent" {
            for branch in shape.elems().filter(|b| b.local() == "Choice" || b.local() == "Fallback") {
                collect_frames(branch, frames);
            }
        } else if SHAPES.contains(&shape.local()) {
            frames.push(frame_of(shape));
        }
    }
}

/// Visits the same objects in the same order as `collect_frames`.
type PadFn<'a> = dyn FnMut(&mut Element, [f64; 4]) -> bool + 'a;

fn place_frames(container: &mut Element, page: &Adaptation, frames: &mut std::vec::IntoIter<Option<Frame>>, pad: &mut PadFn) {
    for shape in container.elems_mut() {
        if shape.local() == "AlternateContent" {
            for branch in shape.elems_mut().filter(|b| b.local() == "Choice" || b.local() == "Fallback") {
                place_frames(branch, page, frames, pad);
            }
        } else if SHAPES.contains(&shape.local()) {
            if let Some(Some(frame)) = frames.next() {
                page.place(shape, frame, pad);
            }
        }
    }
}

/// Geometry of one resize; see `normalize`.
struct Adaptation {
    width: f64,
    height: f64,
    new_width: f64,
    new_height: f64,
    sx: f64,
    sy: f64,
    s: f64,
}

impl Adaptation {
    fn new(from: (i64, i64), to: (i64, i64)) -> Adaptation {
        let (width, height, new_width, new_height) = (from.0 as f64, from.1 as f64, to.0 as f64, to.1 as f64);
        let (sx, sy) = (new_width / width, new_height / height);
        Adaptation { width, height, new_width, new_height, sx, sy, s: sx.min(sy) }
    }
    fn is_unit(&self) -> bool {
        (self.s - 1.0).abs() < 1e-9
    }
    /// Offset that centres the scaled page in the new one (one of them is 0).
    fn offset(&self) -> (f64, f64) {
        ((self.new_width - self.width * self.s) / 2.0, (self.new_height - self.height * self.s) / 2.0)
    }
    /// Every object keeps its place on the page: the whole page is scaled by s
    /// and centred, so nothing moves relative to anything else. Only pictures
    /// that fill the page are extended into the new margins (with their own
    /// colours, see `padding`); videos stay as they are (see `video`).
    fn place(&self, shape: &mut Element, f: Frame, pad: &mut PadFn) {
        let kind = shape.local().to_string();
        let (dx, dy) = self.offset();
        let (mut x, mut y, mut w, mut h) = (f.x * self.s + dx, f.y * self.s + dy, f.cx * self.s, f.cy * self.s);
        let picture = kind == "pic" || (kind == "sp" && shape.child("spPr").and_then(|p| p.child("blipFill")).is_some());
        let fills_page = f.cx >= 0.9 * self.width && f.cy >= 0.9 * self.height;
        if picture && fills_page && !crate::video::is_video(shape) && w > 0.0 && h > 0.0 && (dx > 0.5 || dy > 0.5) {
            // Extend to the edges of the new page on the axis that gained margins.
            let (x0, x1) = if dx > 0.5 { (x.min(0.0), (x + w).max(self.new_width)) } else { (x, x + w) };
            let (y0, y1) = if dy > 0.5 { (y.min(0.0), (y + h).max(self.new_height)) } else { (y, y + h) };
            let sides = [(x - x0) / w, (x1 - x - w) / w, (y - y0) / h, (y1 - y - h) / h];
            let fill = if kind == "pic" { shape.child_mut("blipFill") } else { shape.child_mut("spPr").and_then(|p| p.child_mut("blipFill")) };
            if fill.is_some_and(|fill| pad(fill, sides)) {
                (x, y, w, h) = (x0, y0, x1 - x0, y1 - y0);
            }
        }
        let holder = if kind == "graphicFrame" || kind == "contentPart" {
            Some(&mut *shape)
        } else if shape.child("spPr").is_some() {
            shape.child_mut("spPr")
        } else {
            shape.child_mut("grpSpPr")
        };
        if let Some(t) = holder.and_then(|h| h.child_mut("xfrm")) {
            if let Some(off) = t.child_mut("off") {
                off.set_attr("x", &(x.round() as i64).to_string());
                off.set_attr("y", &(y.round() as i64).to_string());
            }
            if let Some(ext) = t.child_mut("ext") {
                ext.set_attr("cx", &(w.round() as i64).to_string());
                ext.set_attr("cy", &(h.round() as i64).to_string());
            }
        }
    }
}

/// Animation coordinates are fractions of the slide. The content now fills a
/// centred block of the new slide, so they are mapped into that block: scaled
/// by (rx, ry) = block size / slide size and moved by (sx, sy) = block offset /
/// slide size, so every movement covers the same part of the content as before.
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
