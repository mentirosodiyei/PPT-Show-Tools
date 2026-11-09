//! Extract media: the pictures, sounds and videos placed on chosen slides,
//! copied byte for byte into a new folder. Same rules as Sources/MediaExtractor.swift.
use crate::package::{checksum_file, file_name, TempDir};
use crate::tools::{emit, unpack, ToolPage, ToolReport};
use crate::verifier::{ordered_parts, resolve_part};
use crate::xml::{part_path, read_xml, R_NS};
use crate::{fail, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum MediaKind {
    Images,
    Audio,
    Video,
}
pub const KINDS: [MediaKind; 3] = [MediaKind::Images, MediaKind::Audio, MediaKind::Video];
impl MediaKind {
    pub fn title(self) -> &'static str {
        match self {
            MediaKind::Images => "Images",
            MediaKind::Audio => "Audio",
            MediaKind::Video => "Video",
        }
    }
    fn word(self) -> &'static str {
        match self {
            MediaKind::Images => "image",
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
        }
    }
    fn plural(self) -> &'static str {
        match self {
            MediaKind::Images => "images",
            MediaKind::Audio => "audio files",
            MediaKind::Video => "videos",
        }
    }
    pub fn parse(s: &str) -> Option<MediaKind> {
        match s {
            "images" => Some(MediaKind::Images),
            "audio" => Some(MediaKind::Audio),
            "video" => Some(MediaKind::Video),
            _ => None,
        }
    }
}

const AUDIO: [&str; 12] = ["mp3", "wav", "m4a", "aac", "wma", "mid", "midi", "aif", "aiff", "flac", "ogg", "oga"];
const VIDEO: [&str; 11] = ["mp4", "m4v", "mov", "wmv", "avi", "mpg", "mpeg", "mkv", "webm", "asf", "3gp"];

/// "all", "" or a list such as "1-3, 5, 8" → slide numbers (1-based), in order, without repeats.
pub fn slide_selection(spec: &str, count: usize) -> Result<Vec<usize>> {
    let text = spec.trim().to_lowercase();
    if text.is_empty() || text == "all" {
        return Ok((1..=count).collect());
    }
    let mut out = vec![];
    for part in text.split(',') {
        let bounds: Vec<&str> = part.split('-').map(str::trim).collect();
        let parsed: Option<(usize, usize)> = match bounds.as_slice() {
            [a] => a.parse().ok().map(|n| (n, n)),
            [a, b] => a.parse().ok().zip(b.parse().ok()),
            _ => None,
        };
        let Some((first, last)) = parsed.filter(|(f, l)| *f >= 1 && l >= f) else {
            return fail("Slides must be numbers or ranges, for example 1-3, 5, 8.");
        };
        if last > count {
            return fail(format!("The presentation has {count} slides; slide {last} does not exist."));
        }
        for n in first..=last {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    Ok(out)
}

/// A free folder name next to existing ones: "Name media", "Name media 2"…
fn new_folder(parent: &Path, name: &str) -> PathBuf {
    let mut candidate = parent.join(name);
    let mut n = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{name} {n}"));
        n += 1;
    }
    candidate
}

struct Item {
    part: String,
    name: String,
    kind: MediaKind,
    slides: Vec<usize>,
}

pub fn extract_media(input: &Path, destination: &Path, kinds: &[MediaKind], spec: &str) -> Result<ToolReport> {
    if kinds.is_empty() {
        return fail("Choose at least one kind of media: images, audio or video.");
    }
    if !destination.is_dir() {
        return fail("Choose an existing folder for the media.");
    }
    let root = TempDir::new("PPTShowTools-Tools")?;
    unpack(input, root.path())?;
    let ordered = ordered_parts(root.path())?;
    let chosen = slide_selection(spec, ordered.len())?;
    let width = ordered.len().to_string().len();
    let (mut items, mut linked): (Vec<Item>, Vec<String>) = (vec![], vec![]);
    let mut counters: std::collections::HashMap<(usize, MediaKind), usize> = Default::default();
    for &number in &chosen {
        let slide = &ordered[number - 1];
        let (dir, name) = slide.rsplit_once('/').unwrap_or(("", slide));
        let rels_part = format!("{dir}/_rels/{name}.rels");
        if !part_path(root.path(), &rels_part).is_file() {
            continue;
        }
        let rels = read_xml(root.path(), &rels_part)?;
        // Poster frames: the still picture of a video or sound is not an image of its own.
        let mut posters = std::collections::HashSet::new();
        if let Ok(doc) = read_xml(root.path(), slide) {
            for pic in doc.root.descendants("pic") {
                let media = pic.child("nvPicPr").and_then(|n| n.child("nvPr")).is_some_and(|nv| nv.child("videoFile").is_some() || nv.child("audioFile").is_some());
                if let (true, Some(blip)) = (media, pic.child("blipFill").and_then(|b| b.child("blip"))) {
                    posters.insert(blip.attr_ns(R_NS, "embed").to_string());
                }
            }
        }
        for rel in rels.root.named("Relationship") {
            let (kind_type, target) = (rel.attr("Type"), rel.attr("Target"));
            let ext = target.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            let kind = if kind_type.ends_with("/image") {
                (!posters.contains(rel.attr("Id"))).then_some(MediaKind::Images)
            } else if kind_type.ends_with("/audio") {
                Some(MediaKind::Audio)
            } else if kind_type.ends_with("/video") {
                Some(MediaKind::Video)
            } else if kind_type.ends_with("/media") {
                if AUDIO.contains(&ext.as_str()) {
                    Some(MediaKind::Audio)
                } else if VIDEO.contains(&ext.as_str()) {
                    Some(MediaKind::Video)
                } else {
                    None
                }
            } else {
                None
            };
            let Some(kind) = kind.filter(|k| kinds.contains(k)) else { continue };
            if rel.attr("TargetMode") == "External" {
                if !linked.iter().any(|l| l == target) {
                    linked.push(target.to_string());
                }
                continue;
            }
            let part = resolve_part(slide, target);
            if let Some(item) = items.iter_mut().find(|i| i.part == part) {
                if !item.slides.contains(&number) {
                    item.slides.push(number);
                }
                continue;
            }
            if !part_path(root.path(), &part).is_file() {
                continue;
            }
            let counter = counters.entry((number, kind)).or_insert(0);
            *counter += 1;
            let safe_ext = if !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) { ext.clone() } else { "bin".into() };
            items.push(Item { part, name: format!("slide-{number:0width$}-{}-{counter}.{safe_ext}", kind.word()), kind, slides: vec![number] });
        }
    }
    let kind_list: Vec<MediaKind> = KINDS.iter().copied().filter(|k| kinds.contains(k)).collect();
    let titles = kind_list.iter().map(|k| k.title().to_lowercase()).collect::<Vec<_>>().join(", ");
    if items.is_empty() {
        let place = if chosen.len() == ordered.len() { "any slide" } else { "the chosen slides" };
        let extra = if linked.is_empty() { String::new() } else { format!(" Some media is only linked, outside the presentation: {}", linked.join(", ")) };
        return fail(format!("No {titles} on {place}.{extra}"));
    }
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Presentation".into());
    let folder = new_folder(destination, &format!("{stem} media"));
    std::fs::create_dir(&folder).or_else(|e| fail(format!("Cannot create the folder: {e}")))?;
    let copied = (|| -> Result<()> {
        for item in &items {
            let (source, copy) = (part_path(root.path(), &item.part), folder.join(&item.name));
            crate::package::copy_new(&source, &copy)?;
            if checksum_file(&copy)? != checksum_file(&source)? {
                return fail(format!("{} was not copied exactly.", item.name));
            }
        }
        Ok(())
    })();
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(e);
    }
    let summary = kind_list.iter().map(|k| format!("{} {}", items.iter().filter(|i| i.kind == *k).count(), k.plural())).collect::<Vec<_>>().join(", ");
    let scope = if chosen.len() == ordered.len() { format!("all {} slides", ordered.len()) } else { format!("slides {}", spec.trim()) };
    let mut limitations = vec!["Only media placed on the slides themselves is extracted; pictures from layouts and masters (logos, template backgrounds) are not.".to_string(), "The still picture shown for a video or sound (its poster frame) is not counted as an image.".to_string()];
    let shared: Vec<String> = items.iter().filter(|i| i.slides.len() > 1).map(|i| format!("{} (slides {})", i.name, i.slides.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", "))).collect();
    if !shared.is_empty() {
        limitations.push(format!("Used on several slides, extracted once: {}.", shared.join("; ")));
    }
    if !linked.is_empty() {
        limitations.push(format!("Linked, outside the presentation, so not extracted: {}.", linked.join(", ")));
    }
    let report = ToolReport {
        operation: format!("Extract media ({titles})"),
        source: input.display().to_string(),
        output: folder.display().to_string(),
        pages: chosen.iter().map(|n| ToolPage { source_page: *n, font_size: None, empty: !items.iter().any(|i| i.slides.contains(n)), warnings: vec![] }).collect(),
        checks: vec![format!("{summary} from {scope}"), "Files copied byte for byte (SHA-256)".into(), format!("Saved in a new folder: {}", file_name(&folder))],
        limitations,
    };
    if let Err(e) = emit(&report) {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(e);
    }
    Ok(report)
}
