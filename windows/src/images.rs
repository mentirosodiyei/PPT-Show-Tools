//! Images to PowerPoint: one or several pictures per slide, never cropped or
//! stretched, with an optional transition and automatic advance. Same rules
//! as Sources/ImageDeck.swift.
use crate::package::{checksum, extension, file_name, read_file, TempDir};
use crate::tools::{emu, esc, publish, unpack, SimpleDeck, ToolPage, ToolReport};
use crate::verifier::ordered_parts;
use crate::xml::read_xml;
use crate::{fail, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ImageLayout {
    One,
    Four,
    Random,
}
pub const LAYOUTS: [ImageLayout; 3] = [ImageLayout::One, ImageLayout::Four, ImageLayout::Random];
impl ImageLayout {
    pub fn title(self) -> &'static str {
        match self {
            ImageLayout::One => "One per slide",
            ImageLayout::Four => "Four per slide",
            ImageLayout::Random => "Random",
        }
    }
    pub fn parse(s: &str) -> Option<ImageLayout> {
        match s {
            "one" => Some(ImageLayout::One),
            "four" => Some(ImageLayout::Four),
            "random" => Some(ImageLayout::Random),
            _ => None,
        }
    }
}

pub const TRANSITIONS: [&str; 9] = ["none", "fade", "push", "wipe", "split", "cover", "dissolve", "zoom", "random"];
pub fn transition_title(name: &str) -> String {
    if name == "none" {
        return "None".into();
    }
    let mut c = name.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
/// The p:transition element; `advance` in seconds, None for “on click”.
pub fn transition_xml(name: &str, advance: Option<f64>) -> String {
    let effect = match name {
        "none" => String::new(),
        "push" => "<p:push dir=\"l\"/>".into(),
        "wipe" => "<p:wipe dir=\"l\"/>".into(),
        "split" => "<p:split orient=\"vert\" dir=\"out\"/>".into(),
        "cover" => "<p:cover dir=\"l\"/>".into(),
        other => format!("<p:{other}/>"),
    };
    let time = advance.map(|a| format!(" advTm=\"{}\"", (a * 1000.0).round() as i64)).unwrap_or_default();
    match (effect.is_empty(), time.is_empty()) {
        (true, true) => String::new(),
        (true, false) => format!("<p:transition{time}/>"),
        _ => format!("<p:transition spd=\"med\"{time}>{effect}</p:transition>"),
    }
}

/// Formats read today by cameras, phones and the web.
pub const IMAGE_EXTENSIONS: [&str; 11] = ["jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "avif", "tif", "tiff", "bmp"];
const MAX_IMAGES: usize = 2000;

/// Name order as File Explorer shows it: numbers by value, letters ignoring case.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = vec![];
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, text)) if *d == digit => text.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(pb.iter()) {
        let o = match (x.0, y.0) {
            (true, true) => {
                let (tx, ty) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
                tx.len().cmp(&ty.len()).then(tx.cmp(ty))
            }
            _ => x.1.to_lowercase().cmp(&y.1.to_lowercase()),
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    pa.len().cmp(&pb.len()).then(a.cmp(b))
}

/// Files and folders as dropped or chosen: folders give their images (not
/// subfolders), sorted by name. Hidden files are skipped.
pub fn image_files(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let is_image = |p: &Path| IMAGE_EXTENSIONS.contains(&extension(p).as_str()) && !file_name(p).starts_with('.');
    let mut out = vec![];
    for input in inputs {
        if input.is_dir() {
            let mut items: Vec<PathBuf> = std::fs::read_dir(input).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_file() && is_image(p)).collect()).unwrap_or_default();
            items.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));
            out.extend(items);
        } else if input.is_file() && is_image(input) {
            out.push(input.clone());
        }
    }
    out
}

struct Prepared {
    data: Vec<u8>,
    ext: String,
    width: f64,
    height: f64,
    note: Option<String>,
}

/// HEIC, HEIF and AVIF are decoded by the FFmpeg shipped with the app.
fn decode_with_ffmpeg(path: &Path) -> Option<image::DynamicImage> {
    let ffmpeg = crate::video::ffmpeg()?;
    let mut command = std::process::Command::new(ffmpeg);
    command.args(["-nostdin", "-loglevel", "error", "-i"]).arg(path).args(["-frames:v", "1", "-c:v", "png", "-f", "image2pipe", "-"]);
    command.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let out = command.output().ok()?;
    if !out.status.success() {
        return None;
    }
    image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Png).ok()
}

fn has_transparent_pixels(img: &image::DynamicImage) -> bool {
    img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p[3] < 255)
}

/// JPEG, PNG and GIF go in unchanged; other formats, and photos stored
/// sideways, are converted (JPEG, or PNG when they have transparency).
fn prepare(path: &Path) -> Option<Prepared> {
    use image::ImageDecoder;
    let ext = extension(path);
    let keep = ["jpg", "jpeg", "png", "gif"].contains(&ext.as_str());
    let (mut img, rotated) = if ["heic", "heif", "avif"].contains(&ext.as_str()) {
        (decode_with_ffmpeg(path)?, false)
    } else {
        let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
        let mut decoder = reader.into_decoder().ok()?;
        let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
        let (w, h) = decoder.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        let upright = orientation == image::metadata::Orientation::NoTransforms;
        if keep && upright {
            let data = read_file(path).ok()?;
            return Some(Prepared { data, ext: if ext == "jpeg" { "jpg".into() } else { ext }, width: w as f64, height: h as f64, note: None });
        }
        let mut img = image::DynamicImage::from_decoder(decoder).ok()?;
        img.apply_orientation(orientation);
        (img, !upright)
    };
    let _ = &mut img;
    let png = has_transparent_pixels(&img);
    let mut out = std::io::Cursor::new(vec![]);
    if png {
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).encode_image(&img.to_rgb8()).ok()?;
    }
    let note = if rotated && keep { "turned upright".to_string() } else { format!("converted from {} to {}", ext.to_uppercase(), if png { "PNG" } else { "JPEG" }) };
    Some(Prepared { data: out.into_inner(), ext: if png { "png".into() } else { "jpg".into() }, width: img.width() as f64, height: img.height() as f64, note: Some(note) })
}

/// Spaces (x, y, w, h in points) for `count` pictures on a `w` × `h` slide: one
/// fills the slide; two side by side; three as one large and two stacked; four in a grid.
pub fn cells(count: usize, w: f64, h: f64) -> Vec<(f64, f64, f64, f64)> {
    if count <= 1 {
        return vec![(0.0, 0.0, w, h)];
    }
    let m = 0.03 * w.min(h);
    let (iw, ih) = (w - 2.0 * m, h - 2.0 * m);
    let (cw, ch) = ((iw - m) / 2.0, (ih - m) / 2.0);
    match count {
        2 => vec![(m, m, cw, ih), (m + cw + m, m, cw, ih)],
        3 => vec![(m, m, cw, ih), (m + cw + m, m, cw, ch), (m + cw + m, m + ch + m, cw, ch)],
        _ => (0..4).map(|i| (m + (i % 2) as f64 * (cw + m), m + (i / 2) as f64 * (ch + m), cw, ch)).collect(),
    }
}

/// SplitMix64: the same random layouts on macOS and Windows for the same seed.
struct SplitMix(u64);
impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// How many pictures go on each slide.
pub fn groups(total: usize, layout: ImageLayout, seed: u64) -> Vec<usize> {
    let (mut out, mut left, mut random) = (vec![], total, SplitMix(seed));
    while left > 0 {
        let n = match layout {
            ImageLayout::One => 1,
            ImageLayout::Four => left.min(4),
            ImageLayout::Random => 1 + (random.next() % left.min(4) as u64) as usize,
        };
        out.push(n);
        left -= n;
    }
    out
}

fn format_seconds(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

pub fn images_to_ppt(inputs: &[PathBuf], output: &Path, fixed: Option<(i64, i64)>, layout: ImageLayout, transition: &str, advance: Option<f64>, background: &str) -> Result<ToolReport> {
    let files = image_files(inputs);
    if files.is_empty() {
        return fail("No images found. Add JPEG, PNG, GIF, HEIC, WebP, AVIF, TIFF or BMP files, or a folder that contains them.");
    }
    if files.len() > MAX_IMAGES {
        return fail(format!("There are {} images; at most {MAX_IMAGES} can go into one presentation.", files.len()));
    }
    if extension(output) != "pptx" {
        return fail("The output must be PPTX.");
    }
    if output.exists() || files.iter().any(|f| crate::package::same_file(f, output)) || crate::reports::collides(output, &[".report.json", ".report.log"]) {
        return fail("The output or its report already exists. Choose a new name; the originals are not modified.");
    }
    if background.len() != 6 || !background.chars().all(|c| c.is_ascii_hexdigit()) {
        return fail("The background colour must be six hexadecimal digits (RRGGBB), for example 1F3864.");
    }
    if !TRANSITIONS.contains(&transition) {
        return fail(format!("--transition must be one of: {}.", TRANSITIONS.join(", ")));
    }
    if let Some(a) = advance {
        if !(a > 0.0 && a <= 3600.0) {
            return fail("Automatic advance must be between 0.1 and 3600 seconds.");
        }
    }
    // Slide size from Settings; Automatic is 16:9 (1280 × 720 px, PowerPoint's standard).
    let (width, height) = fixed.map(|(w, h)| (w as f64 / 12700.0, h as f64 / 12700.0)).unwrap_or((960.0, 540.0));
    let mut prepared = vec![];
    let mut skipped = vec![];
    for f in &files {
        match prepare(f) {
            Some(p) => prepared.push((f.clone(), p)),
            None => skipped.push(file_name(f)),
        }
    }
    if prepared.is_empty() {
        return fail("None of the images could be read.");
    }
    let seed = std::env::var("PPTSHOWTOOLS_RANDOM_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1));
    let plan = groups(prepared.len(), layout, seed);
    let root = TempDir::new("PPTShowTools-Tools")?;
    let staging = TempDir::new("PPTShowTools-Tools")?;
    let staged = staging.path().join("result.pptx");
    let mut deck = SimpleDeck::new(root.path(), width, height)?;
    // One per slide stays on black; with several per slide the colour is the user's.
    let colour = if layout == ImageLayout::One { "000000".to_string() } else { background.to_ascii_uppercase() };
    deck.background = Some(colour.clone());
    let transition_element = transition_xml(transition, advance);
    let (mut pages, mut hashes, mut next) = (vec![], vec![], 0usize);
    for (slide, count) in plan.iter().enumerate() {
        let (mut body, mut relations, mut warnings) = (String::new(), vec![], vec![]);
        for (i, (x0, y0, cw, ch)) in cells(*count, width, height).into_iter().enumerate() {
            let (file, image) = &prepared[next];
            next += 1;
            let media = format!("ppt/media/image{next}.{}", image.ext);
            std::fs::write(crate::xml::part_path(root.path(), &media), &image.data).or_else(|e| fail(e.to_string()))?;
            hashes.push((media, checksum(&image.data)));
            deck.extensions.insert(image.ext.clone());
            relations.push((format!("img{}", i + 1), "image", format!("../media/image{next}.{}", image.ext)));
            let scale = (cw / image.width).min(ch / image.height);
            let (w, h) = (image.width * scale, image.height * scale);
            let (x, y) = (x0 + (cw - w) / 2.0, y0 + (ch - h) / 2.0);
            body += &format!("<p:pic><p:nvPicPr><p:cNvPr id=\"{}\" name=\"{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"img{}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>", i + 2, esc(&file_name(file)), i + 1, emu(x), emu(y), emu(w), emu(h));
            if let Some(note) = &image.note {
                warnings.push(format!("{}: {note}.", file_name(file)));
            }
        }
        deck.add_with(&body, true, &relations, &transition_element)?;
        pages.push(ToolPage { source_page: slide + 1, font_size: None, empty: false, warnings });
    }
    deck.finish(&staged)?;
    // Check the saved package: slide count, every picture byte for byte, transitions.
    let check = TempDir::new("PPTShowTools-Tools")?;
    unpack(&staged, check.path())?;
    let slides = ordered_parts(check.path())?;
    if slides.len() != plan.len() {
        return fail("The slide count does not match the plan.");
    }
    for (media, hash) in &hashes {
        if checksum(&read_file(&crate::xml::part_path(check.path(), media))?) != *hash {
            return fail(format!("The picture {media} was not preserved."));
        }
    }
    if !transition_element.is_empty() {
        for s in &slides {
            if read_xml(check.path(), s)?.root.child("transition").is_none() {
                return fail(format!("A transition is missing on {s}."));
            }
        }
    }
    let mut limitations = vec![format!("Pictures are never cropped or stretched: each one fits whole in its space, centred, on {}", if colour == "000000" { "a black background.".to_string() } else { format!("the chosen background colour (#{colour}).") }), "JPEG, PNG and GIF pictures are kept byte for byte; other formats, and photos stored sideways, are converted (JPEG, or PNG with transparency).".to_string()];
    if !skipped.is_empty() {
        limitations.push(format!("Skipped (unreadable): {}.", skipped.join(", ")));
    }
    let source = if inputs.len() == 1 { inputs[0].display().to_string() } else { format!("{} items", inputs.len()) };
    let report = ToolReport {
        operation: format!("Images → PowerPoint ({}, transition: {}{})", layout.title().to_lowercase(), transition_title(transition).to_lowercase(), advance.map(|a| format!(", every {} s", format_seconds(a))).unwrap_or_default()),
        source,
        output: output.display().to_string(),
        pages,
        checks: vec![format!("{} pictures on {} slides, in order", prepared.len(), plan.len()), "Pictures verified by SHA-256 after saving".into(), if transition_element.is_empty() { "No transition".into() } else { "Transition on every slide".into() }],
        limitations,
    };
    publish(&staged, &report)?;
    Ok(report)
}
