//! PDF → PowerPoint, conservative 16:9 and notes extraction.
use crate::merge::{describe_size, require_valid_slide_size};
use crate::package::{checksum, copy_new, extension, extract, read_file, zip_dir, TempDir};
use crate::verifier::{ordered_parts, resolve_part, verify, PackageSnapshot};
use crate::xml::{read_xml, A_NS, P_NS, PKG_NS, R_NS};
use crate::{fail, scaler, textfit, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolPage {
    pub source_page: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    pub empty: bool,
    pub warnings: Vec<String>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolReport {
    pub operation: String,
    pub source: String,
    pub output: String,
    pub pages: Vec<ToolPage>,
    pub checks: Vec<String>,
    pub limitations: Vec<String>,
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub fn require_new(input: &Path, output: &Path) -> Result<()> {
    if crate::package::same_file(input, output) || output.exists() || crate::reports::collides(output, &[".report.json", ".report.log"]) {
        return fail("The output or its report already exists. Choose a new name; the original is not modified.");
    }
    Ok(())
}

fn unpack(input: &Path, root: &Path) -> Result<()> {
    let e = extension(input);
    if e != "pptx" && e != "pptm" {
        return fail("Choose a PPTX or PPTM file.");
    }
    extract(input, root)?;
    ordered_parts(root)?;
    Ok(())
}

fn emit(report: &ToolReport) -> Result<()> {
    let output = PathBuf::from(&report.output);
    crate::reports::write(&output, ".report.json", &crate::to_json(report)?)?;
    let warnings: Vec<String> = report.pages.iter().flat_map(|p| p.warnings.iter().map(move |w| format!("Slide {}: {w}", p.source_page))).collect();
    if !warnings.is_empty() {
        crate::reports::write(&output, ".report.log", warnings.join("\r\n").as_bytes())?;
    }
    Ok(())
}

/// The result only reaches its final name after every check has passed.
fn publish(staged: &Path, report: &ToolReport) -> Result<()> {
    let output = PathBuf::from(&report.output);
    copy_new(staged, &output)?;
    if let Err(e) = emit(report) {
        let _ = std::fs::remove_file(&output);
        crate::reports::remove(&output, &[".report.json", ".report.log"]);
        return Err(e);
    }
    Ok(())
}

/// With a fixed size (Settings) the slides take it and every page is fitted
/// whole; otherwise they take the first page's proportion.
pub fn pdf_to_ppt(input: &Path, output: &Path, fixed: Option<(i64, i64)>) -> Result<ToolReport> {
    require_new(input, output)?;
    if extension(output) != "pptx" {
        return fail("The PDF output must be PPTX.");
    }
    let document = crate::pdf::PdfDoc::open(input)?;
    let count = document.page_count();
    if count == 0 {
        return fail("The PDF cannot be read, is locked or has no pages.");
    }
    // Each page becomes an image; this bounds the time and disk space one PDF can take.
    const MAX_PDF_PAGES: usize = 2000;
    if count > MAX_PDF_PAGES {
        return fail(format!("The PDF has {count} pages; at most {MAX_PDF_PAGES} can be converted at once."));
    }
    let first = document.page_size(0)?;
    if first.0 <= 0.0 || first.1 <= 0.0 {
        return fail("Invalid PDF page size.");
    }
    // Keep the first page's aspect ratio; 960 pt wide, tall pages narrowed to 4032 pt.
    let mut width = 960.0;
    let mut height = width * first.1 / first.0;
    if height > 4032.0 {
        width = width * 4032.0 / height;
        height = 4032.0;
    }
    if let Some((w, h)) = fixed {
        width = w as f64 / 12700.0;
        height = h as f64 / 12700.0;
    }
    if !height.is_finite() || width < 72.0 || height < 72.0 {
        return fail("PDF page proportion not supported by PowerPoint (each side must be 1–56 in).");
    }
    let root = TempDir::new("PPTShowTools-Tools")?;
    let staging = TempDir::new("PPTShowTools-Tools")?;
    let staged = staging.path().join("result.pptx");
    let mut deck = SimpleDeck::new(root.path(), width, height)?;
    let mut pages = vec![];
    let mut hashes = vec![];
    for i in 0..count {
        let size = document.page_size(i)?;
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return fail(format!("PDF page {}: invalid size.", i + 1));
        }
        let png = document.render_png(i, 2560)?;
        hashes.push(checksum(&png));
        deck.add_image(&png, size)?;
        pages.push(ToolPage { source_page: i + 1, font_size: None, empty: false, warnings: vec![] });
    }
    deck.finish(&staged)?;
    let check = TempDir::new("PPTShowTools-Tools")?;
    unpack(&staged, check.path())?;
    if ordered_parts(check.path())?.len() != count {
        return fail("The slide count does not match the PDF pages.");
    }
    for (i, hash) in hashes.iter().enumerate() {
        if checksum(&read_file(&check.path().join("ppt").join("media").join(format!("page{}.png", i + 1)))?) != *hash {
            return fail(format!("The image of page {} was not preserved.", i + 1));
        }
    }
    let report = ToolReport {
        operation: "PDF → PowerPoint (images)".into(),
        source: input.display().to_string(),
        output: output.display().to_string(),
        pages,
        checks: strings(&["One slide per page, in order", "PNG images verified by SHA-256", "Proportional fit without cropping"]),
        limitations: vec![
            "PDF text and elements are not editable; images up to 2560 pixels on the long side.".into(),
            if fixed.is_none() { "The output uses the first page's proportion; other pages are fitted with margins." } else { "Slides use the size chosen in Settings; each page is fitted whole, with margins where its proportion differs." }.into(),
            "PDF annotations and form fields may not appear in the image. PDF links and media are not converted.".into(),
        ],
    };
    publish(&staged, &report)?;
    Ok(report)
}

pub fn notes_text(root: &Path, slide: &str) -> Result<String> {
    let (dir, name) = slide.rsplit_once('/').unwrap_or(("", slide));
    let rel = format!("{dir}/_rels/{name}.rels");
    if !crate::xml::part_path(root, &rel).exists() {
        return Ok(String::new());
    }
    let rels = read_xml(root, &rel)?;
    let Some(relation) = rels.root.named("Relationship").find(|r| r.attr("Type").ends_with("/notesSlide") && r.attr("TargetMode") != "External") else { return Ok(String::new()) };
    let doc = read_xml(root, &resolve_part(slide, relation.attr("Target")))?;
    let mut paragraphs = vec![];
    let mut shapes = vec![];
    doc.root.visit(&mut |e| if e.local() == "sp" && e.ns == P_NS { shapes.push(e) });
    for shape in shapes {
        let ph = shape.descendants("ph").into_iter().next();
        // Keep the body and explicit extra note text; omit calculated placeholders.
        if ph.map(|p| ["sldImg", "sldNum", "hdr", "ftr", "dt"].contains(&p.attr("type"))).unwrap_or(false) {
            continue;
        }
        for body in shape.named("txBody") {
            for p in body.named("p") {
                let mut text = String::new();
                for e in p.elems() {
                    if e.local() == "br" {
                        text.push('\n');
                    } else if e.local() == "r" || e.local() == "fld" {
                        for t in e.named("t") {
                            text.push_str(&t.text());
                        }
                    }
                }
                paragraphs.push(text);
            }
        }
    }
    Ok(paragraphs.join("\n").trim().to_string())
}

pub fn extract_notes(input: &Path, output: &Path) -> Result<ToolReport> {
    require_new(input, output)?;
    if extension(output) != "pptx" {
        return fail("The notes presentation is saved as PPTX.");
    }
    let source = TempDir::new("PPTShowTools-Tools")?;
    let root = TempDir::new("PPTShowTools-Tools")?;
    let staging = TempDir::new("PPTShowTools-Tools")?;
    let staged = staging.path().join("result.pptx");
    unpack(input, source.path())?;
    let slides = ordered_parts(source.path())?;
    if slides.is_empty() {
        return fail("The presentation has no slides.");
    }
    let mut deck = SimpleDeck::new(root.path(), 960.0, 540.0)?;
    let mut expected = vec![];
    let mut pages = vec![];
    for (i, part) in slides.iter().enumerate() {
        let text = notes_text(source.path(), part)?;
        let size = textfit::fitted_size(&text);
        if !text.is_empty() && size < 1.0 {
            return fail(format!("The notes of slide {} are too long to fit on one page without losing text.", i + 1));
        }
        let display = if text.is_empty() { "No Notes on this slide".to_string() } else { text.clone() };
        let display_size = if text.is_empty() { 24.0 } else { size };
        deck.add_notes(&display, display_size, text.is_empty())?;
        expected.push(display);
        let warnings = if !text.is_empty() && size < 18.0 { vec![format!("Very long notes: font size {size} pt. Check readability.")] } else { vec![] };
        pages.push(ToolPage { source_page: i + 1, font_size: Some(display_size), empty: text.is_empty(), warnings });
    }
    deck.finish(&staged)?;
    let check = TempDir::new("PPTShowTools-Tools")?;
    unpack(&staged, check.path())?;
    let actual = ordered_parts(check.path())?;
    if actual.len() != slides.len() {
        return fail("The number of notes slides does not match.");
    }
    for (i, part) in actual.iter().enumerate() {
        let doc = read_xml(check.path(), part)?;
        let mut paragraphs = vec![];
        for body in doc.root.descendants("txBody") {
            for p in body.named("p") {
                paragraphs.push(p.descendants("t").iter().map(|t| t.text()).collect::<String>());
            }
        }
        if paragraphs.join("\n") != expected[i] {
            return fail(format!("Notes text lost or changed on slide {}.", i + 1));
        }
    }
    let report = ToolReport {
        operation: "Notes → black 16:9 slides".into(),
        source: input.display().to_string(),
        output: output.display().to_string(),
        pages,
        checks: strings(&["N originals = N new slides, same order", "Full notes text compared after saving", "Black background, editable white text", "Font size computed per page with a safety margin"]),
        limitations: strings(&["Slides without notes show “No Notes on this slide” centred at 24 pt.", "Notes are extracted as text; drawings and original notes formatting are not imported.", "PowerPoint font metrics may vary slightly; check the result visually."]),
    };
    publish(&staged, &report)?;
    Ok(report)
}

/// Resizes every slide. With no fixed size the canvas only widens to 16:9 and
/// the content keeps its physical size; with a fixed size the content is
/// scaled proportionally and centred on that canvas.
pub fn resize(input: &Path, output: &Path, fixed: Option<(i64, i64)>) -> Result<ToolReport> {
    require_new(input, output)?;
    if extension(input) != extension(output) {
        return fail("Keep the original's extension (PPTX or PPTM).");
    }
    let root = TempDir::new("PPTShowTools-Tools")?;
    unpack(input, root.path())?;
    let before = PackageSnapshot::capture(root.path())?;
    let mut pres = read_xml(root.path(), "ppt/presentation.xml")?;
    let Some((w, h)) = crate::verifier::slide_size(&pres.root) else { return fail("Cannot read the slide size.") };
    crate::merge::require_source_size((w, h), &crate::package::file_name(input))?;
    let target = match fixed {
        Some(target) => {
            if (w, h) == target {
                return fail(format!("The presentation is already {}. Nothing to resize.", describe_size(target)));
            }
            target
        }
        None => {
            // Widen only. Never shrink the source page.
            if scaler::is_widescreen((w, h)) {
                return fail("The presentation is already 16:9. Choose a slide size in Settings to resize it to another size.");
            }
            let unit = ((w + 15) / 16).max((h + 8) / 9);
            (unit * 16, unit * 9)
        }
    };
    require_valid_slide_size(target)?;
    scaler::normalize(root.path(), &mut pres, (w, h), target)?;
    let work = TempDir::new("PPTShowTools-Tools")?;
    let archive = work.path().join("result.zip");
    zip_dir(root.path(), &archive)?;
    let check = TempDir::new("PPTShowTools-Tools")?;
    extract(&archive, check.path())?;
    let checks = verify(std::slice::from_ref(&before), &[input.to_path_buf()], check.path())?;
    let issues: Vec<String> = checks.iter().flat_map(|c| c.issues.clone()).collect();
    if !issues.is_empty() {
        return fail(format!("The conversion failed its checks: {}", issues.join("; ")));
    }
    let out = read_xml(check.path(), "ppt/presentation.xml")?;
    if crate::verifier::slide_size(&out.root) != Some(target) {
        return fail("The expected slide size was not kept.");
    }
    let canvas = if scaler::same_aspect((w, h), target) {
        format!("Canvas {}; content scaled proportionally", describe_size(target))
    } else {
        format!("Canvas {}; layout adapted: nothing is stretched, text boxes and full-width elements follow the new shape, pictures filling the slide are shown whole, with margins in their own colours", describe_size(target))
    };
    let report = ToolReport {
        operation: format!("Resize slides: {} → {}", describe_size((w, h)), describe_size(target)),
        source: input.display().to_string(),
        output: output.display().to_string(),
        pages: (0..before.slides.len()).map(|i| ToolPage { source_page: i + 1, font_size: None, empty: false, warnings: vec![] }).collect(),
        checks: vec![canvas, "Order, text, notes, animations, transitions and resources verified after saving".into()],
        limitations: strings(&["When the proportion changes, the layout is adapted: text boxes and full-width elements follow the new shape, pictures filling the slide are shown whole, with margins in their own colours and other objects keep their size and place.", "Playback and the appearance of OLE, SmartArt, fonts and advanced effects cannot be certified automatically. Check the result in PowerPoint."]),
    };
    publish(&archive, &report)?;
    Ok(report)
}

/// Small native OOXML writer for image decks and plain editable notes.
pub struct SimpleDeck {
    root: PathBuf,
    width: f64,
    height: f64,
    count: usize,
    image_pages: std::collections::HashSet<usize>,
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn emu(points: f64) -> i64 {
    (points * 12700.0).round() as i64
}
const GROUP: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";
const CLR_MAP: &str = "<p:clrMap accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" bg1=\"lt1\" bg2=\"lt2\" folHlink=\"folHlink\" hlink=\"hlink\" tx1=\"dk1\" tx2=\"dk2\"/>";

fn rels(items: &[(&str, &str, String)]) -> String {
    let body: String = items.iter().map(|(id, kind, target)| format!("<Relationship Id=\"{id}\" Type=\"{R_NS}/{kind}\" Target=\"{}\"/>", esc(target))).collect();
    format!("<Relationships xmlns=\"{PKG_NS}\">{body}</Relationships>")
}

impl SimpleDeck {
    pub fn new(root: &Path, width: f64, height: f64) -> Result<SimpleDeck> {
        for folder in ["_rels", "ppt/_rels", "ppt/slides/_rels", "ppt/media", "ppt/slideMasters/_rels", "ppt/slideLayouts/_rels", "ppt/theme"] {
            std::fs::create_dir_all(crate::xml::part_path(root, folder)).or_else(|e| fail(e.to_string()))?;
        }
        Ok(SimpleDeck { root: root.to_path_buf(), width, height, count: 0, image_pages: Default::default() })
    }
    fn put(&self, name: &str, text: &str) -> Result<()> {
        std::fs::write(crate::xml::part_path(&self.root, name), text).or_else(|e| fail(format!("Cannot write {name}: {e}")))
    }
    fn add(&mut self, body: &str, black: bool) -> Result<()> {
        self.count += 1;
        let n = self.count;
        self.put(&format!("ppt/slides/slide{n}.xml"), &format!("<p:sld xmlns:p=\"{P_NS}\" xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\"><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>{GROUP}{body}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>", if black { "000000" } else { "FFFFFF" }))?;
        let mut relations = vec![("layout", "slideLayout", "../slideLayouts/slideLayout1.xml".to_string())];
        if self.image_pages.contains(&n) {
            relations.push(("image", "image", format!("../media/page{n}.png")));
        }
        self.put(&format!("ppt/slides/_rels/slide{n}.xml.rels"), &rels(&relations))
    }
    pub fn add_image(&mut self, png: &[u8], size: (f64, f64)) -> Result<()> {
        let n = self.count + 1;
        self.image_pages.insert(n);
        std::fs::write(crate::xml::part_path(&self.root, &format!("ppt/media/page{n}.png")), png).or_else(|e| fail(e.to_string()))?;
        let factor = (self.width / size.0).min(self.height / size.1);
        let (w, h) = (size.0 * factor, size.1 * factor);
        let body = format!("<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Page {n}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"image\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>", emu((self.width - w) / 2.0), emu((self.height - h) / 2.0), emu(w), emu(h));
        self.add(&body, false)
    }
    pub fn add_notes(&mut self, text: &str, font_size: f64, centered: bool) -> Result<()> {
        let sz = ((font_size * 100.0).round() as i64).max(100);
        let paragraphs: String = text.split('\n').map(|line| format!("<a:p><a:pPr algn=\"{}\"/><a:r><a:rPr lang=\"en-US\" sz=\"{sz}\"><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill><a:latin typeface=\"Arial\"/><a:ea typeface=\"Arial\"/><a:cs typeface=\"Arial\"/></a:rPr><a:t xml:space=\"preserve\">{}</a:t></a:r><a:endParaRPr sz=\"{sz}\"/></a:p>", if centered { "ctr" } else { "l" }, esc(line))).collect();
        let body = if text.is_empty() { String::new() } else { format!("<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Notes\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"457200\" y=\"457200\"/><a:ext cx=\"11277600\" cy=\"5943600\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" rIns=\"0\" tIns=\"0\" bIns=\"0\" anchor=\"ctr\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>") };
        self.add(&body, true)
    }
    pub fn finish(&self, output: &Path) -> Result<()> {
        if self.count == 0 {
            return fail("There are no slides to save.");
        }
        let a = A_NS;
        self.put("_rels/.rels", &rels(&[("office", "officeDocument", "ppt/presentation.xml".into())]))?;
        let ids: String = (1..=self.count).map(|i| format!("<p:sldId id=\"{}\" r:id=\"slide{i}\"/>", 255 + i)).collect();
        self.put("ppt/presentation.xml", &format!("<p:presentation xmlns:p=\"{P_NS}\" xmlns:r=\"{R_NS}\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"master\"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>", emu(self.width), emu(self.height)))?;
        let mut pres_rels = vec![("master".to_string(), "slideMaster", "slideMasters/slideMaster1.xml".to_string())];
        pres_rels.extend((1..=self.count).map(|i| (format!("slide{i}"), "slide", format!("slides/slide{i}.xml"))));
        let pres_rels: Vec<(&str, &str, String)> = pres_rels.iter().map(|(a, b, c)| (a.as_str(), *b, c.clone())).collect();
        self.put("ppt/_rels/presentation.xml.rels", &rels(&pres_rels))?;
        self.put("ppt/slideMasters/slideMaster1.xml", &format!("<p:sldMaster xmlns:p=\"{P_NS}\" xmlns:a=\"{a}\" xmlns:r=\"{R_NS}\"><p:cSld><p:spTree>{GROUP}</p:spTree></p:cSld>{CLR_MAP}<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"layout\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>"))?;
        self.put("ppt/slideMasters/_rels/slideMaster1.xml.rels", &rels(&[("layout", "slideLayout", "../slideLayouts/slideLayout1.xml".into()), ("theme", "theme", "../theme/theme1.xml".into())]))?;
        self.put("ppt/slideLayouts/slideLayout1.xml", &format!("<p:sldLayout xmlns:p=\"{P_NS}\" xmlns:a=\"{a}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{GROUP}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"))?;
        self.put("ppt/slideLayouts/_rels/slideLayout1.xml.rels", &rels(&[("master", "slideMaster", "../slideMasters/slideMaster1.xml".into())]))?;
        let colors: String = [("dk1", "000000"), ("lt1", "FFFFFF"), ("dk2", "222222"), ("lt2", "EEEEEE"), ("accent1", "4472C4"), ("accent2", "ED7D31"), ("accent3", "A5A5A5"), ("accent4", "FFC000"), ("accent5", "5B9BD5"), ("accent6", "70AD47"), ("hlink", "0563C1"), ("folHlink", "954F72")].iter().map(|(k, v)| format!("<a:{k}><a:srgbClr val=\"{v}\"/></a:{k}>")).collect();
        let fill = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
        let line = format!("<a:ln w=\"12700\">{fill}<a:prstDash val=\"solid\"/></a:ln>");
        self.put("ppt/theme/theme1.xml", &format!("<a:theme xmlns:a=\"{a}\" name=\"PPT Tools\"><a:themeElements><a:clrScheme name=\"Default\">{colors}</a:clrScheme><a:fontScheme name=\"Arial\"><a:majorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Default\"><a:fillStyleLst>{}</a:fillStyleLst><a:lnStyleLst>{}</a:lnStyleLst><a:effectStyleLst>{}</a:effectStyleLst><a:bgFillStyleLst>{}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>", fill.repeat(3), line.repeat(3), "<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3), fill.repeat(3)))?;
        let mut overrides = vec![("ppt/presentation.xml".to_string(), "presentation.main"), ("ppt/slideMasters/slideMaster1.xml".into(), "slideMaster"), ("ppt/slideLayouts/slideLayout1.xml".into(), "slideLayout")];
        overrides.extend((1..=self.count).map(|i| (format!("ppt/slides/slide{i}.xml"), "slide")));
        let types: String = overrides.iter().map(|(p, k)| format!("<Override PartName=\"/{p}\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.{k}+xml\"/>")).collect();
        self.put("[Content_Types].xml", &format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>{types}</Types>"))?;
        zip_dir(&self.root, output)
    }
}
