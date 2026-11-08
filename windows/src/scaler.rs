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

/// Adapts every slide, layout and master to a new size without distorting
/// anything and without leaving empty bands:
/// - text, pictures, charts, tables and groups keep their proportions, scaled
///   by s = min(sx, sy), the factor that fits the old page in the new one;
/// - text boxes, placeholders and connectors take the new proportion (their
///   text reflows at the scaled font size);
/// - shapes spanning the page keep spanning it; pictures spanning it and picture
///   backgrounds are never cropped or stretched: they are shown whole, and the
///   new margins take the colours of the picture's edges (see `padding`);
/// - other objects keep their place relative to the nearest edge, or to the
///   centre when they sit in the middle third.
/// When the proportion does not change this is plain proportional scaling.
/// Animation coordinates are fractions of the slide, so they stay valid.
pub fn normalize(package: &Path, presentation: &mut Document, from: (i64, i64), to: (i64, i64)) -> Result<()> {
    if from == to {
        return Ok(());
    }
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
            if layout_part {
                if let Some(tree) = doc.root.first_descendant_mut("spTree") {
                    collect_frames(tree, &mut frames);
                }
            }
            if !page.is_unit() {
                scale_node(&mut doc.root, page.s, is_master, "");
            }
            if layout_part {
                if let Some(tree) = doc.root.first_descendant_mut("spTree") {
                    let mut next = frames.into_iter();
                    place_frames(tree, &page, &mut next, &mut |fill, gx, gy| part.pad(&mut pad, fill, gx, gy));
                }
                // A picture background always fills the slide: show it whole in the new proportion.
                if let Some(bg) = doc.root.first_descendant_mut("bgPr").and_then(|b| b.child_mut("blipFill")) {
                    if !part.pad(&mut pad, bg, page.sx, page.sy) {
                        crate::padding::inset_fill(bg, page.sx, page.sy);
                    }
                }
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
    write_xml(presentation, package, "ppt/presentation.xml")
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
type PadFn<'a> = dyn FnMut(&mut Element, f64, f64) -> bool + 'a;

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
    /// New start and length on one axis.
    fn axis(&self, start: f64, length: f64, page: f64, new_page: f64, factor: f64, stretch: bool) -> (f64, f64) {
        if stretch {
            return (start * factor, length * factor);
        }
        let size = length * self.s;
        let centre = (start + length / 2.0) / page;
        let mut position = if centre < 1.0 / 3.0 {
            start * self.s // near the start edge
        } else if centre > 2.0 / 3.0 {
            new_page - (page - start - length) * self.s - size // near the end edge
        } else {
            centre * new_page - size / 2.0 // middle third: keep centred
        };
        if start >= 0.0 && start + length <= page {
            position = position.max(0.0).min((new_page - size).max(0.0));
        }
        (position, size)
    }
    fn place(&self, shape: &mut Element, f: Frame, pad: &mut PadFn) {
        let kind = shape.local().to_string();
        let picture = kind == "pic" || (kind == "sp" && shape.child("spPr").and_then(|p| p.child("blipFill")).is_some());
        let text = kind == "sp" && !picture && (!shape.descendants("ph").is_empty() || shape.descendants("t").iter().any(|t| !t.text().is_empty()));
        let (spans_x, spans_y) = (f.cx >= 0.9 * self.width, f.cy >= 0.9 * self.height);
        let (mut stretch_x, mut stretch_y) = match kind.as_str() {
            "cxnSp" => (true, true),
            "sp" if text => (true, true),
            "sp" | "pic" => (spans_x, spans_y),
            _ => (false, false), // groups, charts, tables, SmartArt: rigid
        };
        // A picture that would change shape is padded to its new proportion; if
        // that is not possible it keeps its own proportion instead (never cropped).
        if picture && (stretch_x || stretch_y) {
            let gx = if stretch_x { self.sx } else { self.s };
            let gy = if stretch_y { self.sy } else { self.s };
            let fill = if kind == "pic" { shape.child_mut("blipFill") } else { shape.child_mut("spPr").and_then(|p| p.child_mut("blipFill")) };
            if !(f.cx > 0.0 && f.cy > 0.0 && fill.is_some_and(|fill| pad(fill, gx, gy))) {
                (stretch_x, stretch_y) = (false, false);
            }
        }
        let (x, w) = self.axis(f.x, f.cx, self.width, self.new_width, self.sx, stretch_x);
        let (y, h) = self.axis(f.y, f.cy, self.height, self.new_height, self.sy, stretch_y);
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
