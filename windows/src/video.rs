//! Videos are never modified. When a video filled the slide and the new size
//! has another proportion, it keeps its own proportion, centred, and a copy is
//! placed behind it: scaled to cover the whole slide without distortion
//! (cropped only outside the slide), heavily blurred and silent. The copy plays
//! and pauses with the original. FFmpeg makes the blurred copy; without it the
//! background is a blurred still of the video's poster frame.
use crate::padding::{Pad, PartPad};
use crate::xml::{Document, Element, Node, R_NS};
use std::path::{Path, PathBuf};

pub const BACKDROP_NAME: &str = "PPT Show Tools blurred background";
const VIDEO_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/video";
const MEDIA_REL: &str = "http://schemas.microsoft.com/office/2007/relationships/media";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

pub fn is_video(shape: &Element) -> bool {
    shape.local() == "pic" && shape.child("nvPicPr").and_then(|n| n.child("nvPr")).is_some_and(|p| p.child("videoFile").is_some())
}

fn shape_id(shape: &Element) -> String {
    shape.child("nvPicPr").and_then(|n| n.child("cNvPr")).map(|c| c.attr("id").to_string()).unwrap_or_default()
}

/// Ids of top-level videos that cover (almost) the whole slide.
pub fn spanning_videos(tree: &Element, width: f64, height: f64) -> Vec<String> {
    let mut out = vec![];
    for shape in tree.elems() {
        if !is_video(shape) {
            continue;
        }
        let ext = shape.child("spPr").and_then(|s| s.child("xfrm")).and_then(|t| t.child("ext"));
        let (cx, cy) = ext.map(|e| (e.attr("cx").parse::<f64>().unwrap_or(0.0), e.attr("cy").parse::<f64>().unwrap_or(0.0))).unwrap_or((0.0, 0.0));
        if cx >= 0.9 * width && cy >= 0.9 * height {
            out.push(shape_id(shape));
        }
    }
    out
}

/// Size of the blurred copy: small, since it is blurred, in the slide's proportion.
pub fn backdrop_size(slide: (f64, f64)) -> (u32, u32) {
    let aspect = slide.0 / slide.1;
    let even = |v: f64| ((v / 2.0).round() as u32 * 2).max(16);
    let (mut w, mut h) = (even(360.0 * aspect), 360);
    if w > 2560 {
        w = 2560;
        h = even(2560.0 / aspect);
    }
    (w, h)
}
/// Blur strength: 15 % of the copy's height: no detail is left, only colours and movement.
pub fn blur_sigma(height: u32) -> f32 {
    0.15 * height as f32
}

/// FFmpeg, from places a user installs it to. Never from the app's own folder
/// unless the app is installed (a portable app in Downloads could be next to a
/// planted file), and never from the current folder.
pub fn ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PPTSHOWTOOLS_FFMPEG").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let mut dirs: Vec<PathBuf> = vec![];
    if crate::reports::installed() {
        dirs.extend(exe_dir.clone());
    }
    let cwd = std::env::current_dir().ok();
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            if d.is_absolute() && Some(&d) != exe_dir.as_ref() && Some(&d) != cwd.as_ref() {
                dirs.push(d);
            }
        }
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        dirs.extend(env("LOCALAPPDATA").map(|d| d.join("Microsoft").join("WinGet").join("Links")));
        dirs.extend(env("ProgramData").map(|d| d.join("chocolatey").join("bin")));
        dirs.extend(env("ProgramFiles").map(|d| d.join("ffmpeg").join("bin")));
        dirs.extend(env("USERPROFILE").map(|d| d.join("scoop").join("shims")));
    } else {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// Blurred, silent copy of a video in the slide's proportion. False if FFmpeg is missing or fails.
fn blurred_video(input: &Path, output: &Path, size: (u32, u32)) -> bool {
    let Some(ffmpeg) = ffmpeg() else { return false };
    let (w, h) = size;
    let filter = format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},gblur=sigma={:.1},format=yuv420p", blur_sigma(h));
    let mut command = std::process::Command::new(ffmpeg);
    command.args(["-nostdin", "-y", "-loglevel", "error", "-i"]).arg(input).args(["-an", "-vf", &filter, "-c:v", "libx264", "-preset", "veryfast", "-crf", "30", "-movflags", "+faststart"]).arg(output);
    command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = command.spawn() else { return false };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1800);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success() && output.is_file(),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

/// Poster frame scaled to cover `size`, blurred like the video, as PNG.
fn blurred_poster(data: &[u8], size: (u32, u32)) -> Option<Vec<u8>> {
    let img = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = size;
    let scale = (w as f64 / img.width() as f64).max(h as f64 / img.height() as f64);
    let (sw, sh) = (((img.width() as f64 * scale).round() as u32).max(w), ((img.height() as f64 * scale).round() as u32).max(h));
    let scaled = image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Triangle);
    let cropped = image::imageops::crop_imm(&scaled, (sw - w) / 2, (sh - h) / 2, w, h).to_image();
    let blurred = image::imageops::blur(&cropped, blur_sigma(h));
    let mut out = std::io::Cursor::new(vec![]);
    blurred.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn max_id(e: &Element, local: &str) -> u64 {
    let mut m = 0;
    e.visit(&mut |x| {
        if x.local() == local {
            m = m.max(x.attr("id").parse().unwrap_or(0));
        }
    });
    m
}

/// Adds a blurred copy behind each video in `ids`; returns warnings for the report.
pub fn add_backdrops(package: &Path, doc: &mut Document, ids: &[String], slide: (f64, f64), part: &mut PartPad, pad: &mut Pad) -> Vec<String> {
    let mut warnings = vec![];
    for id in ids {
        match add_backdrop(package, doc, id, slide, part, pad) {
            Some(true) => {}
            Some(false) => warnings.push("A video now has a blurred still behind it. Install FFmpeg to get a moving blurred background.".into()),
            None => warnings.push("A video keeps its proportion; no blurred background could be made for it (linked video or unreadable poster frame).".into()),
        }
    }
    warnings
}

/// Some(true): moving copy; Some(false): still; None: nothing added.
fn add_backdrop(package: &Path, doc: &mut Document, id: &str, slide: (f64, f64), part: &mut PartPad, pad: &mut Pad) -> Option<bool> {
    let next_shape = max_id(&doc.root, "cNvPr") + 1;
    let tree = doc.root.first_descendant_mut("spTree")?;
    let index = tree.children.iter().position(|n| matches!(n, Node::Elem(e) if is_video(e) && shape_id(e) == id))?;
    let Node::Elem(original) = &tree.children[index] else { return None };
    let nv = original.child("nvPicPr")?.child("nvPr")?;
    let media_id = nv.descendants("media").first().map(|m| m.attr_ns(R_NS, "embed").to_string()).filter(|s| !s.is_empty()).or_else(|| nv.child("videoFile").map(|v| v.attr_ns(R_NS, "link").to_string()))?;
    let video = part.target(&media_id)?;
    let poster = part.target(original.child("blipFill")?.child("blip")?.attr_ns(R_NS, "embed"))?;
    let size = backdrop_size(slide);
    let poster_png = blurred_poster(&std::fs::read(crate::xml::part_path(package, &poster)).ok()?, size)?;
    let stem = uuid::Uuid::new_v4().simple().to_string();
    let poster_name = format!("ppt/media/pst-blur-{}.png", &stem[..8]);
    std::fs::write(crate::xml::part_path(package, &poster_name), poster_png).ok()?;
    pad.extensions.insert("png".into());
    let video_name = format!("ppt/media/pst-blur-{}.mp4", &stem[..8]);
    let moving = blurred_video(&crate::xml::part_path(package, &video), &crate::xml::part_path(package, &video_name), size);
    let poster_rel = part.add_relationship(pad, IMAGE_REL, &format!("/{poster_name}"))?;
    let mut copy = original.clone();
    let rels = if moving {
        pad.extensions.insert("mp4".into());
        Some((part.add_relationship(pad, VIDEO_REL, &format!("/{video_name}"))?, part.add_relationship(pad, MEDIA_REL, &format!("/{video_name}"))?))
    } else {
        None
    };
    // Identity: a new id and a name the verifier recognises; no click action.
    if let Some(c) = copy.child_mut("nvPicPr").and_then(|n| n.child_mut("cNvPr")) {
        c.set_attr("id", &next_shape.to_string());
        c.set_attr("name", BACKDROP_NAME);
        c.remove_attr("descr");
        c.remove_attr("title");
        c.children.clear();
    }
    if let Some(nv) = copy.child_mut("nvPicPr").and_then(|n| n.child_mut("nvPr")) {
        match &rels {
            Some((video_rel, media_rel)) => {
                if let Some(v) = nv.child_mut("videoFile") {
                    v.set_attr_ns(R_NS, "link", "r", video_rel);
                }
                if let Some(m) = nv.first_descendant_mut("media") {
                    m.set_attr_ns(R_NS, "embed", "r", media_rel);
                    m.children.retain(|n| !matches!(n, Node::Elem(e) if e.local() == "trim" || e.local() == "bmkLst"));
                }
            }
            None => nv.children.clear(),
        }
    }
    if let Some(blip) = copy.child_mut("blipFill").and_then(|b| b.child_mut("blip")) {
        blip.set_attr_ns(R_NS, "embed", "r", &poster_rel);
        blip.children.clear();
    }
    copy.children.retain(|n| !matches!(n, Node::Elem(e) if e.local() == "style"));
    // Frame: the whole slide; only a plain rectangle, no border or effects.
    if let Some(sp) = copy.child_mut("spPr") {
        sp.children.retain(|n| matches!(n, Node::Elem(e) if e.local() == "xfrm" || e.local() == "prstGeom"));
        if let Some(t) = sp.child_mut("xfrm") {
            t.attrs.retain(|a| a.local() != "rot" && a.local() != "flipH" && a.local() != "flipV");
            if let Some(off) = t.child_mut("off") {
                off.set_attr("x", "0");
                off.set_attr("y", "0");
            }
            if let Some(ext) = t.child_mut("ext") {
                ext.set_attr("cx", &(slide.0.round() as i64).to_string());
                ext.set_attr("cy", &(slide.1.round() as i64).to_string());
            }
        }
    }
    tree.children.insert(index, Node::Elem(copy));
    if moving {
        if let Some(timing) = doc.root.child_mut("timing") {
            let mut next_node = max_id(timing, "cTn");
            follow(timing, id, &next_shape.to_string(), &mut next_node);
        }
    }
    Some(moving)
}

fn targets(e: &Element, spid: &str) -> bool {
    e.descendants("spTgt").iter().any(|t| t.attr("spid") == spid)
}

/// Every media command and media node of the original also drives the copy (muted).
fn follow(e: &mut Element, original: &str, copy: &str, next: &mut u64) {
    let mut i = 0;
    while i < e.children.len() {
        let clone = match &e.children[i] {
            Node::Elem(c) if ["cmd", "video", "audio"].contains(&c.local()) && targets(c, original) => {
                let mut c = c.clone();
                c.visit_mut(&mut |x| {
                    if x.local() == "spTgt" && x.attr("spid") == original {
                        x.set_attr("spid", copy);
                    }
                    if x.local() == "cTn" {
                        *next += 1;
                        x.set_attr("id", &next.to_string());
                    }
                    if x.local() == "cMediaNode" {
                        x.set_attr("vol", "0");
                        x.set_attr("mute", "1");
                    }
                });
                Some(c)
            }
            _ => None,
        };
        match clone {
            Some(c) => {
                e.children.insert(i + 1, Node::Elem(c));
                i += 2;
            }
            None => {
                if let Node::Elem(child) = &mut e.children[i] {
                    follow(child, original, copy, next);
                }
                i += 1;
            }
        }
    }
}

/// The verifier compares a part without the copies this module adds.
pub fn without_backdrops(element: &Element) -> Element {
    let mut ids = vec![];
    element.visit(&mut |e| {
        if e.local() == "cNvPr" && e.attr("name") == BACKDROP_NAME {
            ids.push(e.attr("id").to_string());
        }
    });
    let mut copy = element.clone();
    if ids.is_empty() {
        return copy;
    }
    fn strip(e: &mut Element, ids: &[String]) {
        e.children.retain(|n| match n {
            Node::Elem(c) if c.local() == "pic" => !c.child("nvPicPr").and_then(|v| v.child("cNvPr")).is_some_and(|p| p.attr("name") == BACKDROP_NAME),
            Node::Elem(c) if ["cmd", "video", "audio"].contains(&c.local()) => !ids.iter().any(|id| targets(c, id)),
            _ => true,
        });
        for c in e.elems_mut() {
            strip(c, ids);
        }
    }
    strip(&mut copy, &ids);
    copy
}
