//! Output slide size chosen in Settings, stored in %APPDATA%\PPT Show Tools\settings.json
//! by the installed app. The portable app keeps it only while it is open.
//! Presets and custom sizes are pixels at 96 per inch, as PowerPoint measures them.
use crate::merge::EMU_PER_PIXEL;
use crate::{fail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum SizeChoice {
    Automatic,
    Hd720,
    Hd1080,
    Uhd,
    Standard43,
    Custom,
}
pub const SIZE_CHOICES: [SizeChoice; 6] = [SizeChoice::Automatic, SizeChoice::Hd720, SizeChoice::Hd1080, SizeChoice::Uhd, SizeChoice::Standard43, SizeChoice::Custom];

impl SizeChoice {
    pub fn title(self) -> &'static str {
        match self {
            SizeChoice::Automatic => "Automatic",
            SizeChoice::Hd720 => "16:9 · 1280 × 720 px (PowerPoint standard)",
            SizeChoice::Hd1080 => "16:9 · 1920 × 1080 px (Full HD)",
            SizeChoice::Uhd => "16:9 · 3840 × 2160 px (4K)",
            SizeChoice::Standard43 => "4:3 · 1024 × 768 px",
            SizeChoice::Custom => "Custom",
        }
    }
    pub fn pixels(self) -> Option<(i64, i64)> {
        match self {
            SizeChoice::Hd720 => Some((1280, 720)),
            SizeChoice::Hd1080 => Some((1920, 1080)),
            SizeChoice::Uhd => Some((3840, 2160)),
            SizeChoice::Standard43 => Some((1024, 768)),
            SizeChoice::Automatic | SizeChoice::Custom => None,
        }
    }
}

/// PowerPoint allows 1–56 inches per side.
pub const PIXEL_RANGE: std::ops::RangeInclusive<i64> = 96..=5376;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub slide_size: SizeChoice,
    pub custom_width: i64,
    pub custom_height: i64,
    /// Images to PowerPoint: layout ("one", "four", "random"), transition and
    /// automatic advance in seconds (0 = on click).
    pub image_layout: String,
    pub image_transition: String,
    pub image_advance: i64,
    /// Background colour (RRGGBB) when there are several pictures per slide.
    pub image_background: String,
}
impl Default for Settings {
    fn default() -> Settings {
        Settings { slide_size: SizeChoice::Automatic, custom_width: 1920, custom_height: 1080, image_layout: "one".into(), image_transition: "fade".into(), image_advance: 0, image_background: "000000".into() }
    }
}

/// Pixels to EMU, refusing sizes outside PowerPoint's limits.
pub fn fixed_from_pixels(width: i64, height: i64) -> Result<(i64, i64)> {
    if !PIXEL_RANGE.contains(&width) || !PIXEL_RANGE.contains(&height) {
        return fail("Each side of the slide size must be between 96 and 5376 px. Change it in Settings.");
    }
    Ok((width * EMU_PER_PIXEL, height * EMU_PER_PIXEL))
}

impl Settings {
    /// None means automatic.
    pub fn fixed_size(&self) -> Result<Option<(i64, i64)>> {
        if self.slide_size == SizeChoice::Automatic {
            return Ok(None);
        }
        let (w, h) = self.slide_size.pixels().unwrap_or((self.custom_width, self.custom_height));
        fixed_from_pixels(w, h).map(Some)
    }
    pub fn summary(&self) -> String {
        match self.slide_size {
            SizeChoice::Automatic => "Automatic (the size every presentation shares; otherwise 16:9)".into(),
            choice => {
                let (w, h) = choice.pixels().unwrap_or((self.custom_width, self.custom_height));
                format!("{w} × {h} px{}", if choice == SizeChoice::Custom { " (custom)" } else { "" })
            }
        }
    }
    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("PPT Show Tools").join("settings.json"))
    }
    pub fn load() -> Settings {
        if !crate::reports::installed() {
            return Settings::default();
        }
        Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        if !crate::reports::installed() {
            return;
        }
        if let Some(p) = Self::path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(data) = serde_json::to_vec_pretty(self) {
                let _ = std::fs::write(p, data);
            }
        }
    }
}
