//! Largest Arial size at which notes text fits the notes slide box, using the
//! real Arial metrics (word wrap like PowerPoint, with a safety reserve).
use std::sync::OnceLock;

struct Metrics {
    advances: std::collections::HashMap<char, f64>,
    fallback: f64,
    line: f64,
}

fn font_candidates() -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    if let Ok(windir) = std::env::var("WINDIR") {
        v.push(std::path::Path::new(&windir).join("Fonts").join("arial.ttf"));
    }
    v.push("C:\\Windows\\Fonts\\arial.ttf".into());
    v.push("/System/Library/Fonts/Supplemental/Arial.ttf".into());
    v.push("/Library/Fonts/Arial.ttf".into());
    v
}

fn metrics() -> &'static Option<Metrics> {
    static M: OnceLock<Option<Metrics>> = OnceLock::new();
    M.get_or_init(|| {
        let data = font_candidates().into_iter().find_map(|p| std::fs::read(p).ok())?;
        let face = ttf_parser::Face::parse(&data, 0).ok()?;
        let upem = face.units_per_em() as f64;
        let mut advances = std::collections::HashMap::new();
        for code in (0x20u32..0x250).chain(0x2000..0x2070).chain([0x20AC, 0x2122]) {
            if let Some(c) = char::from_u32(code) {
                if let Some(g) = face.glyph_index(c) {
                    advances.insert(c, face.glyph_hor_advance(g).unwrap_or(0) as f64 / upem);
                }
            }
        }
        let line = (face.ascender() as f64 - face.descender() as f64 + face.line_gap() as f64) / upem;
        Some(Metrics { advances, fallback: 0.6, line })
    })
}

fn char_width(m: Option<&Metrics>, c: char) -> f64 {
    match m {
        Some(m) => *m.advances.get(&c).unwrap_or(&if c.is_whitespace() { 0.278 } else { m.fallback }),
        // Conservative estimate when Arial is missing.
        None => if c.is_whitespace() { 0.3 } else { 0.62 },
    }
}

fn line_count(paragraph: &str, size: f64, width: f64, m: Option<&Metrics>) -> usize {
    let w = |s: &str| s.chars().map(|c| char_width(m, c)).sum::<f64>() * size;
    let mut lines = 1;
    let mut current = 0.0;
    let space = char_width(m, ' ') * size;
    for word in paragraph.split(' ') {
        let mut ww = w(word);
        if current > 0.0 && current + space + ww > width {
            lines += 1;
            current = 0.0;
        }
        if current > 0.0 {
            current += space;
        }
        // Words longer than the line break by characters.
        if ww > width {
            for c in word.chars() {
                let cw = char_width(m, c) * size;
                if current + cw > width && current > 0.0 {
                    lines += 1;
                    current = 0.0;
                }
                current += cw;
            }
            ww = 0.0;
        }
        current += ww;
    }
    lines
}

pub fn fits(text: &str, size: f64) -> bool {
    let m = metrics().as_ref();
    let line = m.map(|m| m.line).unwrap_or(1.2) * size;
    let lines: usize = text.split('\n').map(|p| line_count(p, size, 888.0, m)).sum();
    // 420 pt of the 468 pt box: safety reserve for PowerPoint font metrics.
    lines as f64 * line <= 420.0
}

/// 0 when the text does not fit even at 1 pt (DrawingML minimum).
pub fn fitted_size(text: &str) -> f64 {
    if text.is_empty() || !fits(text, 1.0) {
        return 0.0;
    }
    let (mut low, mut high) = (1.0, 400.0);
    for _ in 0..22 {
        let mid = (low + high) / 2.0;
        if fits(text, mid) {
            low = mid
        } else {
            high = mid
        }
    }
    (low * 100.0).floor() / 100.0
}
