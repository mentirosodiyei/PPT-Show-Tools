//! Pictures that fill the slide (backgrounds, full-bleed photos) are never
//! cropped or stretched when the proportion changes. The whole picture is
//! placed, centred, on a new canvas of the new proportion; the margins take
//! the colour of the nearest edge of the picture, row by row (or column by
//! column) and softened, so the join is hard to see. The new image is a new
//! media part; the original stays untouched for anything else that uses it.
use crate::verifier::resolve_part;
use crate::xml::{read_xml, write_xml, Document, Element, PKG_NS, R_NS};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
/// Canvas limit: big enough for 8K walls, small enough for memory.
const MAX_PIXELS: f64 = 36_000_000.0;

/// State shared by every part of one package.
#[derive(Default)]
pub struct Pad {
    made: HashMap<String, String>,
    pub extensions: BTreeSet<String>,
    next: usize,
}

/// One part (slide, layout or master) and its relationships.
pub struct PartPad<'a> {
    package: &'a Path,
    part: String,
    rels: Option<Document>,
    dirty: bool,
}

impl<'a> PartPad<'a> {
    pub fn new(package: &'a Path, part: &str) -> PartPad<'a> {
        PartPad { package, part: part.into(), rels: None, dirty: false }
    }
    fn rels_part(&self) -> String {
        let (dir, name) = self.part.rsplit_once('/').unwrap_or(("", &self.part));
        format!("{dir}/_rels/{name}.rels")
    }
    /// Replaces the picture of `blip_fill` by a copy padded so a box whose
    /// width grew by `gx` and height by `gy` shows it whole and undistorted.
    /// False when that is not possible (other formats, existing crops, tiles).
    pub fn pad(&mut self, state: &mut Pad, blip_fill: &mut Element, gx: f64, gy: f64) -> bool {
        let k = gx / gy;
        if (k - 1.0).abs() <= 1e-6 {
            return true;
        }
        if blip_fill.child("tile").is_some() || has_offsets(blip_fill.child("srcRect")) || has_offsets(blip_fill.child("stretch").and_then(|s| s.child("fillRect"))) {
            return false;
        }
        let Some(embed) = blip_fill.child("blip").map(|b| b.attr_ns(R_NS, "embed").to_string()).filter(|e| !e.is_empty()) else { return false };
        if self.rels.is_none() {
            let Ok(doc) = read_xml(self.package, &self.rels_part()) else { return false };
            self.rels = Some(doc);
        }
        let rels = self.rels.as_mut().expect("loaded above");
        let Some(rel) = rels.root.named("Relationship").find(|r| r.attr("Id") == embed && r.attr("TargetMode") != "External") else { return false };
        let media = resolve_part(&self.part, rel.attr("Target"));
        let ext = media.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        if !["png", "jpg", "jpeg"].contains(&ext.as_str()) {
            return false;
        }
        let key = format!("{media}|{k:.6}");
        let name = match state.made.get(&key) {
            Some(name) => name.clone(),
            None => {
                let Ok(data) = std::fs::read(crate::xml::part_path(self.package, &media)) else { return false };
                let Some(bytes) = padded(&data, ext != "png", gx, gy) else { return false };
                let name = format!("ppt/media/pst-pad-{}.{ext}", uuid::Uuid::new_v4().simple().to_string().get(..8).unwrap_or("0"));
                if std::fs::write(crate::xml::part_path(self.package, &name), bytes).is_err() {
                    return false;
                }
                state.extensions.insert(ext.clone());
                state.made.insert(key, name.clone());
                name
            }
        };
        let id = loop {
            state.next += 1;
            let id = format!("rIdPad{}", state.next);
            if !rels.root.named("Relationship").any(|r| r.attr("Id") == id) {
                break id;
            }
        };
        let prefix = rels.root.qname.split_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
        let mut e = Element::new(&format!("{prefix}Relationship"), PKG_NS);
        e.set_attr("Id", &id);
        e.set_attr("Type", IMAGE_REL);
        e.set_attr("Target", &format!("/{name}"));
        rels.root.add(e);
        self.dirty = true;
        if let Some(blip) = blip_fill.child_mut("blip") {
            blip.set_attr_ns(R_NS, "embed", "r", &id);
        }
        true
    }
    /// Target part of a relationship of this part (internal ones only).
    pub fn target(&mut self, id: &str) -> Option<String> {
        if self.rels.is_none() {
            self.rels = read_xml(self.package, &self.rels_part()).ok();
        }
        let rel = self.rels.as_ref()?.root.named("Relationship").find(|r| r.attr("Id") == id && r.attr("TargetMode") != "External")?;
        Some(resolve_part(&self.part, rel.attr("Target")))
    }
    /// Adds a relationship and returns its new id.
    pub fn add_relationship(&mut self, state: &mut Pad, kind: &str, target: &str) -> Option<String> {
        if self.rels.is_none() {
            self.rels = read_xml(self.package, &self.rels_part()).ok();
        }
        let rels = self.rels.as_mut()?;
        let id = loop {
            state.next += 1;
            let id = format!("rIdPad{}", state.next);
            if !rels.root.named("Relationship").any(|r| r.attr("Id") == id) {
                break id;
            }
        };
        let prefix = rels.root.qname.split_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
        let mut e = Element::new(&format!("{prefix}Relationship"), PKG_NS);
        e.set_attr("Id", &id);
        e.set_attr("Type", kind);
        e.set_attr("Target", target);
        rels.root.add(e);
        self.dirty = true;
        Some(id)
    }
    pub fn save(&self) -> crate::Result<()> {
        match (&self.rels, self.dirty) {
            (Some(doc), true) => write_xml(doc, self.package, &self.rels_part()),
            _ => Ok(()),
        }
    }
}

fn has_offsets(rect: Option<&Element>) -> bool {
    rect.is_some_and(|r| ["l", "t", "r", "b"].iter().any(|k| r.attr(k).parse::<f64>().unwrap_or(0.0).abs() > 0.0))
}

/// When a picture background cannot be padded, it is shown whole (inset) rather than cropped.
pub fn inset_fill(blip_fill: &mut Element, gx: f64, gy: f64) {
    let k = gx / gy;
    if (k - 1.0).abs() <= 1e-6 {
        return;
    }
    let Some(stretch) = blip_fill.child_mut("stretch") else { return };
    if stretch.child("fillRect").is_none() {
        let prefix = stretch.qname.split_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
        stretch.add(Element::new(&format!("{prefix}fillRect"), crate::xml::A_NS));
    }
    let rect = stretch.child_mut("fillRect").expect("inserted above");
    let v = |r: &Element, key: &str| r.attr(key).parse::<f64>().unwrap_or(0.0);
    let (first, second, share) = if k > 1.0 { ("l", "r", 1.0 - 1.0 / k) } else { ("t", "b", 1.0 - k) };
    let room = 100000.0 - v(rect, first) - v(rect, second);
    for key in [first, second] {
        let n = v(rect, key) + share / 2.0 * room;
        rect.set_attr(key, &(n.round() as i64).to_string());
    }
}

/// Registers the formats of the new images in [Content_Types].xml.
pub fn register(package: &Path, state: &Pad) -> crate::Result<()> {
    if state.extensions.is_empty() {
        return Ok(());
    }
    let mut ct = read_xml(package, "[Content_Types].xml")?;
    let prefix = ct.root.qname.split_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
    for ext in &state.extensions {
        if ct.root.named("Default").any(|d| d.attr("Extension").eq_ignore_ascii_case(ext)) {
            continue;
        }
        let mut d = Element::new(&format!("{prefix}Default"), crate::xml::CT_NS);
        d.set_attr("Extension", ext);
        d.set_attr("ContentType", match ext.as_str() { "png" => "image/png", "mp4" => "video/mp4", _ => "image/jpeg" });
        ct.root.add(d);
    }
    write_xml(&ct, package, "[Content_Types].xml")
}

/// Canvas and image sizes for a picture of `w` × `h` pixels in a box grown by gx, gy.
pub fn canvas(w: u32, h: u32, gx: f64, gy: f64) -> (u32, u32, u32, u32) {
    let s = gx.min(gy);
    let (cw, ch) = (w as f64 * gx / s, h as f64 * gy / s);
    let f = (MAX_PIXELS / (cw * ch)).sqrt().min(1.0);
    let px = |v: f64| (v * f).round().max(1.0) as u32;
    (px(cw), px(ch), px(w as f64), px(h as f64))
}

/// Average colour of each row (or column) of an edge strip `strip_div` deep,
/// softened along the edge over `blur_div` of its length.
fn edge_colours(img: &image::RgbaImage, horizontal: bool, far: bool, strip_div: u32, blur_div: u32) -> Vec<[f64; 4]> {
    let (w, h) = img.dimensions();
    let (lines, depth) = if horizontal { (h, w) } else { (w, h) };
    let strip = (depth / strip_div).max(1);
    let mut raw = Vec::with_capacity(lines as usize);
    for line in 0..lines {
        let mut sum = [0.0; 4];
        for d in 0..strip {
            let d = if far { depth - 1 - d } else { d };
            let p = if horizontal { img.get_pixel(d, line) } else { img.get_pixel(line, d) };
            for c in 0..4 {
                sum[c] += p[c] as f64;
            }
        }
        raw.push(sum.map(|v| v / strip as f64));
    }
    let radius = (lines / blur_div).max(1) as i64;
    (0..lines as i64)
        .map(|i| {
            let (a, b) = ((i - radius).max(0), (i + radius).min(lines as i64 - 1));
            let mut sum = [0.0; 4];
            for j in a..=b {
                for c in 0..4 {
                    sum[c] += raw[j as usize][c];
                }
            }
            sum.map(|v| v / (b - a + 1) as f64)
        })
        .collect()
}

/// The padded image, encoded as JPEG (photos) or PNG.
fn padded(data: &[u8], jpeg: bool, gx: f64, gy: f64) -> Option<Vec<u8>> {
    let source = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = source.dimensions();
    let (cw, ch, iw, ih) = canvas(w, h, gx, gy);
    let horizontal = cw > iw;
    // Next to the picture the margin takes its exact edge; further out it fades
    // into a soft wash of the same colours, so the join does not show.
    let sharp = [edge_colours(&source, horizontal, false, 200, 200), edge_colours(&source, horizontal, true, 200, 200)];
    let soft = [edge_colours(&source, horizontal, false, 25, 8), edge_colours(&source, horizontal, true, 25, 8)];
    let fade = (0.4 * if horizontal { iw } else { ih } as f64).max(1.0);
    let mix = |side: usize, line: usize, distance: u32| -> [f64; 4] {
        let line = line.min(sharp[side].len() - 1);
        // Ease-out: sharp details of the edge dissolve quickly.
        let t = (distance as f64 / fade).min(1.0);
        let t = t * (2.0 - t);
        let (a, b) = (sharp[side][line], soft[side][line]);
        [0, 1, 2, 3].map(|c| a[c] * (1.0 - t) + b[c] * t)
    };
    let picture = if (iw, ih) == (w, h) { source } else { image::imageops::resize(&source, iw, ih, image::imageops::FilterType::Triangle) };
    let (ox, oy) = ((cw - iw) / 2, (ch - ih) / 2);
    let mut out = image::RgbaImage::new(cw, ch);
    let colour = |c: [f64; 4]| image::Rgba(c.map(|v| v.round().clamp(0.0, 255.0) as u8));
    for y in 0..ch {
        for x in 0..cw {
            let (inside_x, inside_y) = (x >= ox && x < ox + iw, y >= oy && y < oy + ih);
            if inside_x && inside_y {
                out.put_pixel(x, y, *picture.get_pixel(x - ox, y - oy));
            } else if horizontal {
                let line = (((y.min(oy + ih - 1).max(oy) - oy) as f64 + 0.5) * h as f64 / ih as f64) as usize;
                out.put_pixel(x, y, colour(if x < ox { mix(0, line, ox - x) } else { mix(1, line, x + 1 - (ox + iw)) }));
            } else {
                let line = (((x.min(ox + iw - 1).max(ox) - ox) as f64 + 0.5) * w as f64 / iw as f64) as usize;
                out.put_pixel(x, y, colour(if y < oy { mix(0, line, oy - y) } else { mix(1, line, y + 1 - (oy + ih)) }));
            }
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    if jpeg {
        let rgb = image::DynamicImage::ImageRgba8(out).to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90).encode_image(&rgb).ok()?;
    } else {
        out.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
    }
    Some(bytes.into_inner())
}
