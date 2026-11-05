//! Window that reproduces the macOS app: same layout, controls, colours,
//! texts and behaviour, in light or dark mode following the system.
use crate::package::{extension, file_name, with_suffix};
use crate::settings::{Settings, SizeChoice, PIXEL_RANGE, SIZE_CHOICES};
use crate::tools::ToolReport;
use crate::verifier::FileValidation;
use crate::{tools, Error};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Rect, RichText, Sense, Stroke};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

// ---------- macOS look ----------

/// Colours measured on the macOS app (light and dark appearance).
struct Palette {
    bg: Color32,
    separator: Color32,
    text: Color32,
    secondary: Color32,
    button: Color32,
    button_hover: Color32,
    button_down: Color32,
    disabled_fill: Color32,
    disabled_text: Color32,
    accent: Color32,
    accent_down: Color32,
    selection: Color32,
    segment: Color32,
    divider: Color32,
    field: Color32,
    field_stroke: Color32,
    sheet: Color32,
    orange: Color32,
    red: Color32,
}
fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: hex(0x1E1E1E),
            separator: hex(0x2E2E2E),
            text: hex(0xDDDDDD),
            secondary: hex(0x969696),
            button: hex(0x303030),
            button_hover: hex(0x383838),
            button_down: hex(0x464646),
            disabled_fill: hex(0x262626),
            disabled_text: hex(0x5A5A5A),
            accent: hex(0x007AFF),
            accent_down: hex(0x0063D1),
            selection: hex(0x0059D1),
            segment: hex(0x303030),
            divider: hex(0x646464),
            field: hex(0x262626),
            field_stroke: hex(0x3E3E3E),
            sheet: hex(0x282828),
            orange: hex(0xFF9F0A),
            red: hex(0xFF453A),
        }
    } else {
        Palette {
            bg: hex(0xFFFFFF),
            separator: hex(0xE5E5E5),
            text: hex(0x262626),
            secondary: hex(0x808080),
            button: hex(0xEBEBEB),
            button_hover: hex(0xE2E2E2),
            button_down: hex(0xD4D4D4),
            disabled_fill: hex(0xF5F5F5),
            disabled_text: hex(0xBFBFBF),
            accent: hex(0x007AFF),
            accent_down: hex(0x0062CC),
            selection: hex(0x0064E1),
            segment: hex(0xEBEBEB),
            divider: hex(0xB4B4B4),
            field: hex(0xFFFFFF),
            field_stroke: hex(0xD4D4D4),
            sheet: hex(0xFFFFFF),
            orange: hex(0xFF9500),
            red: hex(0xFF3B30),
        }
    }
}

fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

const BODY: f32 = 13.0;
const CALLOUT: f32 = 12.0;

#[derive(PartialEq, Clone, Copy)]
enum ButtonKind {
    Normal,
    /// Blue default action, also triggered by Enter (`.keyboardShortcut(.defaultAction)`).
    Default,
}

/// macOS push button: 24 pt high, 12 pt side padding, 6 pt corner radius.
fn mac_button(ui: &mut egui::Ui, p: &Palette, label: &str, kind: ButtonKind, enabled: bool) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(label.to_string(), regular(BODY), p.text);
    let size = vec2(galley.size().x + 24.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let down = enabled && response.is_pointer_button_down_on();
    let (fill, text) = if !enabled {
        (p.disabled_fill, p.disabled_text)
    } else if kind == ButtonKind::Default {
        (if down { p.accent_down } else { p.accent }, Color32::WHITE)
    } else {
        (if down { p.button_down } else if response.hovered() { p.button_hover } else { p.button }, p.text)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    let galley = ui.painter().layout_no_wrap(label.to_string(), regular(BODY), text);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, text);
    response
}

fn label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).wrap().selectable(true))
}

/// Segmented picker as in macOS: equal segments, blue selection, dividers.
fn segmented(ui: &mut egui::Ui, p: &Palette, labels: &[&str], selected: usize) -> Option<usize> {
    let widths: Vec<f32> = labels.iter().map(|l| ui.painter().layout_no_wrap(l.to_string(), regular(BODY), p.text).size().x).collect();
    let segment = widths.iter().cloned().fold(0.0, f32::max) + 24.0;
    let (rect, _) = ui.allocate_exact_size(vec2(segment * labels.len() as f32, 24.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(6), p.segment);
    let mut clicked = None;
    for (i, text) in labels.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + i as f32 * segment, rect.top()), vec2(segment, 24.0));
        let response = ui.interact(r, ui.id().with(("segment", i)), if ui.is_enabled() { Sense::click() } else { Sense::hover() });
        if i == selected {
            ui.painter().rect_filled(r, CornerRadius::same(6), p.accent);
        } else if i > 0 && i - 1 != selected {
            ui.painter().vline(r.left(), r.y_range().shrink(5.0), Stroke::new(1.0_f32, p.divider));
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *text, regular(BODY), if i == selected { Color32::WHITE } else { p.text });
        if response.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// macOS radio button: 16 pt circle, blue with a white dot when selected.
fn radio(ui: &mut egui::Ui, p: &Palette, selected: bool, text: &str) -> bool {
    let galley = ui.painter().layout_no_wrap(text.to_string(), regular(BODY), p.text);
    let (rect, response) = ui.allocate_exact_size(vec2(22.0 + galley.size().x, 22.0), Sense::click());
    let center = pos2(rect.left() + 8.0, rect.center().y);
    if selected {
        ui.painter().circle_filled(center, 8.0, p.accent);
        ui.painter().circle_filled(center, 3.0, Color32::WHITE);
    } else {
        ui.painter().circle(center, 7.5, p.field, Stroke::new(1.0_f32, p.field_stroke));
    }
    ui.painter().galley(pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    response.clicked()
}

/// Rounded macOS text field for a whole number of pixels.
fn number_field(ui: &mut egui::Ui, p: &Palette, value: &mut i64, buffer: &mut String, id: &str) -> bool {
    let enabled = ui.is_enabled();
    let (rect, _) = ui.allocate_exact_size(vec2(80.0, 24.0), Sense::hover());
    ui.painter().rect(rect, CornerRadius::same(6), p.field, Stroke::new(1.0_f32, p.field_stroke), egui::StrokeKind::Inside);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 4.0))));
    let edit = egui::TextEdit::singleline(buffer).id(egui::Id::new(id)).frame(false).font(regular(BODY)).text_color(if enabled { p.text } else { p.secondary }).desired_width(64.0);
    let response = child.add_enabled(enabled, edit);
    if response.changed() {
        buffer.retain(|c| c.is_ascii_digit());
        if let Ok(v) = buffer.parse::<i64>() {
            *value = v;
            return true;
        }
    }
    false
}

fn spinner_row(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(16.0).color(p.secondary));
        ui.label(RichText::new(text).font(regular(BODY)).color(p.text));
    });
}

// ---------- App state ----------

#[derive(PartialEq, Clone, Copy)]
enum Function {
    Merge,
    Pdf,
    Resize,
    Notes,
}
const FUNCTIONS: [Function; 4] = [Function::Merge, Function::Pdf, Function::Resize, Function::Notes];
impl Function {
    fn title(self) -> &'static str {
        match self {
            Function::Merge => "Merge",
            Function::Pdf => "PDF to PowerPoint",
            Function::Resize => "Resize slides",
            Function::Notes => "Extract notes",
        }
    }
    fn action(self) -> &'static str {
        match self {
            Function::Merge => "Merge…",
            Function::Pdf => "Convert to PowerPoint…",
            Function::Resize => "Resize…",
            Function::Notes => "Extract notes…",
        }
    }
}

#[derive(Clone)]
struct Job {
    inputs: Vec<PathBuf>,
    output: PathBuf,
    fit: bool,
    fixed: Option<(i64, i64)>,
    powerpoint: bool,
}
enum Msg {
    Progress(String),
    Merged(Job, std::result::Result<(bool, usize, Vec<FileValidation>), Error>),
    Tool(std::result::Result<ToolReport, String>),
}
/// macOS NSAlert equivalent: title, text and buttons (first one is the default).
#[derive(Clone)]
struct Alert {
    title: String,
    text: String,
    buttons: Vec<&'static str>,
    choice_job: Option<Job>,
}

struct App {
    function: Function,
    settings: Settings,
    width_text: String,
    height_text: String,
    files: Vec<PathBuf>,
    selected: Option<usize>,
    source: Option<PathBuf>,
    busy: bool,
    status: String,
    merge_result: Option<PathBuf>,
    validation: Vec<FileValidation>,
    tool_report: Option<ToolReport>,
    show_checks: bool,
    show_settings: bool,
    alert: Option<Alert>,
    icon: Option<egui::TextureHandle>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    snapshot: Option<(PathBuf, u32)>,
}

const MERGE_HINT: &str = "Add the presentations in the order you want to merge them.";

impl App {
    fn new() -> App {
        let (tx, rx) = channel();
        let settings = Settings::load();
        let mut app = App {
            function: Function::Merge,
            width_text: settings.custom_width.to_string(),
            height_text: settings.custom_height.to_string(),
            settings,
            files: vec![],
            selected: None,
            source: None,
            busy: false,
            status: MERGE_HINT.into(),
            merge_result: None,
            validation: vec![],
            tool_report: None,
            show_checks: false,
            show_settings: false,
            alert: None,
            icon: None,
            tx,
            rx,
            snapshot: None,
        };
        app.snapshot_setup();
        app
    }

    fn explanation(&self) -> String {
        let size = self.settings.summary();
        match self.function {
            Function::Merge => format!("Combines the presentations in the order shown. Each one keeps its masters, themes, animations, transitions, media and embedded fonts. Slide size: {size}."),
            Function::Pdf => "One slide per PDF page, as an image. The slides use the first page's proportion; text is not editable.".into(),
            Function::Resize if size == "automatic" => "Widens the slides to 16:9 and centres the content without enlarging, stretching or cropping it. Choose another size in Settings.".into(),
            Function::Resize => format!("Changes the slide size to {size}, scaling the content proportionally and centring it, without stretching or cropping."),
            Function::Notes => "One black 16:9 slide per original slide, with its speaker notes as editable white text. Slides without notes show “No Notes on this slide”.".into(),
        }
    }

    fn set_function(&mut self, f: Function) {
        if f == self.function {
            return;
        }
        self.function = f;
        self.source = None;
        self.tool_report = None;
        self.status = if f == Function::Merge { MERGE_HINT.into() } else { "Choose the original file.".into() };
    }

    fn add(&mut self, urls: Vec<PathBuf>) {
        for url in urls {
            let e = extension(&url);
            if (e == "pptx" || e == "pptm") && !self.files.contains(&url) {
                self.files.push(url);
            }
        }
    }

    fn start_merge(&mut self, ctx: &egui::Context, job: Job) {
        self.busy = true;
        self.merge_result = None;
        self.validation.clear();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let progress = |m: String| {
                let _ = tx.send(Msg::Progress(m));
                ctx.request_repaint();
            };
            let outcome = if job.powerpoint {
                if crate::powerpoint::available() {
                    crate::powerpoint::merge(&job.inputs, &job.output, job.fit, job.fixed, &progress).map(|r| (r.passed(), r.power_point_slides, r.validation()))
                } else {
                    Err(Error::Fail("Microsoft PowerPoint was not found.".into()))
                }
            } else {
                crate::merge::merge(&job.inputs, &job.output, job.fit, job.fixed).map(|r| (r.structural_validation_passed(), r.total_slides, r.validation))
            };
            let _ = tx.send(Msg::Merged(job, outcome));
            ctx.request_repaint();
        });
    }

    fn merge(&mut self, ctx: &egui::Context, powerpoint: bool) {
        let fixed = match self.settings.fixed_size() {
            Ok(f) => f,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        let macro_first = !powerpoint && self.files.first().map(|f| extension(f) == "pptm").unwrap_or(false);
        let ext = if macro_first { "pptm" } else { "pptx" };
        let mut dialog = rfd::FileDialog::new().set_file_name(format!("Merged presentation.{ext}"));
        if let Some(dir) = self.files.first().and_then(|f| f.parent()) {
            dialog = dialog.set_directory(dir);
        }
        dialog = if powerpoint {
            dialog.add_filter("PowerPoint (PPTX)", &["pptx"])
        } else if macro_first {
            dialog.add_filter("PowerPoint with macros (PPTM)", &["pptm"]).add_filter("PowerPoint (PPTX)", &["pptx"])
        } else {
            dialog.add_filter("PowerPoint (PPTX)", &["pptx"]).add_filter("PowerPoint with macros (PPTM)", &["pptm"])
        };
        if let Some(output) = dialog.save_file() {
            self.start_merge(ctx, Job { inputs: self.files.clone(), output, fit: false, fixed, powerpoint });
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Progress(m) => self.status = m,
            Msg::Merged(job, Ok((passed, slides, validation))) => {
                self.busy = false;
                self.merge_result = Some(job.output.clone());
                self.validation = validation;
                self.show_checks = true;
                self.status = if passed {
                    format!("{slides} slides{}. Structural checks passed; visual and playback review pending.", if job.powerpoint { " saved by PowerPoint" } else { "" })
                } else {
                    "Import NOT correct: differences found. Check the results per file before using the output.".into()
                };
                if !passed {
                    let text = self.validation.iter().filter(|v| !v.issues.is_empty()).map(|v| format!("{}:\n{}", v.file, v.issues.iter().take(5).cloned().collect::<Vec<_>>().join("\n"))).collect::<Vec<_>>().join("\n\n");
                    self.alert = Some(Alert { title: "The import is not correct".into(), text, buttons: vec!["OK"], choice_job: None });
                }
            }
            Msg::Merged(job, Err(Error::FormatChoice(message))) => {
                self.busy = false;
                self.alert = Some(Alert { title: "Some presentations are not 16:9".into(), text: message, buttons: vec!["Stop and edit manually", "Fit without enlarging"], choice_job: Some(job) });
            }
            Msg::Merged(_, Err(Error::Fail(message))) => {
                self.busy = false;
                self.status = message;
            }
            Msg::Tool(result) => {
                self.busy = false;
                match result {
                    Ok(report) => {
                        self.status = format!("{} slides created. Checks passed; review the appearance and the warnings in the report.", report.pages.len());
                        self.tool_report = Some(report);
                    }
                    Err(message) => self.status = message,
                }
            }
        }
    }

    fn start_tool(&mut self, ctx: &egui::Context) {
        let Some(source) = self.source.clone() else { return };
        let fixed = match self.settings.fixed_size() {
            Ok(f) => f,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        let task = self.function;
        let ext = if task == Function::Resize { extension(&source) } else { "pptx".into() };
        let suffix = match task {
            Function::Notes => " notes",
            Function::Resize => " resized",
            _ => "",
        };
        let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}{suffix}.{ext}")).add_filter("PowerPoint", &[ext.as_str()]);
        if let Some(dir) = source.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(output) = dialog.save_file() else { return };
        self.busy = true;
        self.tool_report = None;
        self.merge_result = None;
        self.status = "Converting and checking…".into();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match task {
                Function::Pdf => tools::pdf_to_ppt(&source, &output),
                Function::Resize => tools::resize(&source, &output, fixed),
                _ => tools::extract_notes(&source, &output),
            };
            let result = result.map_err(|e| {
                let log = tools::log_failure(&source, &e.to_string());
                format!("STOPPED: {e}\nLog: {log}")
            });
            let _ = tx.send(Msg::Tool(result));
            ctx.request_repaint();
        });
    }

    // ----- Page -----

    fn header(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new("Deckwright").font(bold(26.0)).color(p.text)).selectable(false));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if mac_button(ui, p, "Settings…", ButtonKind::Normal, true).clicked() {
                    self.show_settings = true;
                }
            });
        });
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("Function").font(regular(BODY)).color(p.text));
            let titles: Vec<&str> = FUNCTIONS.iter().map(|f| f.title()).collect();
            let current = FUNCTIONS.iter().position(|f| *f == self.function).unwrap_or(0);
            if let Some(i) = segmented(ui, p, &titles, current) {
                self.set_function(FUNCTIONS[i]);
            }
        });
        let text = self.explanation();
        label(ui, &text, regular(CALLOUT), p.secondary);
    }

    fn file_list(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let available = ui.available_rect_before_wrap();
        let (rect, _) = ui.allocate_exact_size(available.size(), Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 10.0))));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
            // Rows follow each other every 24 pt, as in the macOS List.
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in 0..self.files.len() {
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), if ui.is_enabled() { Sense::click() } else { Sense::hover() });
                let selected = self.selected == Some(i);
                if selected {
                    ui.painter().rect_filled(row, CornerRadius::same(5), p.selection);
                }
                ui.painter().text(pos2(row.left() + 6.0, row.center().y), Align2::LEFT_CENTER, format!("{}. {}", i + 1, file_name(&self.files[i])), regular(BODY), if selected { Color32::WHITE } else { p.text });
                if response.clicked() {
                    self.selected = Some(i);
                }
            }
        });
    }

    fn merge_bottom(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, p: &Palette, modal: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if mac_button(ui, p, "Add…", ButtonKind::Normal, true).clicked() {
                if let Some(files) = rfd::FileDialog::new().add_filter("PowerPoint", &["pptx", "pptm"]).pick_files() {
                    self.add(files);
                }
            }
            let sel = self.selected.filter(|i| *i < self.files.len());
            if mac_button(ui, p, "Up", ButtonKind::Normal, sel.is_some()).clicked() {
                if let Some(i) = sel.filter(|i| *i > 0) {
                    self.files.swap(i, i - 1);
                    self.selected = Some(i - 1);
                }
            }
            if mac_button(ui, p, "Down", ButtonKind::Normal, sel.is_some()).clicked() {
                if let Some(i) = sel.filter(|i| i + 1 < self.files.len()) {
                    self.files.swap(i, i + 1);
                    self.selected = Some(i + 1);
                }
            }
            if mac_button(ui, p, "Remove", ButtonKind::Normal, sel.is_some()).clicked() {
                if let Some(i) = sel {
                    self.files.remove(i);
                    self.selected = None;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let can = self.files.len() >= 2;
                let enter = can && ui.is_enabled() && !modal && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if mac_button(ui, p, "Merge…", ButtonKind::Default, can).clicked() || enter {
                    self.merge(ctx, false);
                }
                let help = "Merges the presentations and has Microsoft PowerPoint open and save the result. Your keyboard and mouse are not used.";
                if mac_button(ui, p, "Merge and save with PowerPoint…", ButtonKind::Normal, can).on_hover_text(help).clicked() {
                    self.merge(ctx, true);
                }
            });
        });
        self.status_and_results(ui, p);
    }

    fn file_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, p: &Palette, modal: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if mac_button(ui, p, "Choose file…", ButtonKind::Normal, true).clicked() {
                let dialog = if self.function == Function::Pdf { rfd::FileDialog::new().add_filter("PDF", &["pdf"]) } else { rfd::FileDialog::new().add_filter("PowerPoint", &["pptx", "pptm"]) };
                if let Some(f) = dialog.pick_file() {
                    self.source = Some(f);
                    self.tool_report = None;
                    self.status = "Ready to create a new copy.".into();
                }
            }
            ui.label(RichText::new(self.source.as_ref().map(|s| file_name(s)).unwrap_or_else(|| "No file selected".into())).font(regular(BODY)).color(p.text));
        });
        label(ui, "A new file is created; the original is not changed. The checks run automatically and are saved in a report.", regular(CALLOUT), p.secondary);
        let can = self.source.is_some();
        let enter = can && ui.is_enabled() && !modal && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if mac_button(ui, p, self.function.action(), ButtonKind::Default, can).clicked() || enter {
            self.start_tool(ctx);
        }
        self.status_and_results(ui, p);
        if let Some(report) = self.tool_report.clone() {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for c in &report.checks {
                    label(ui, &format!("✓ {c}"), regular(BODY), p.text);
                }
                for page in report.pages.iter().filter(|x| !x.warnings.is_empty()) {
                    label(ui, &format!("Slide {}: {}", page.source_page, page.warnings.join(" ")), regular(BODY), p.orange);
                }
                for l in &report.limitations {
                    label(ui, l, regular(10.0), p.secondary);
                }
            });
        }
    }

    fn status_and_results(&mut self, ui: &mut egui::Ui, p: &Palette) {
        if self.busy {
            spinner_row(ui, p, if self.function == Function::Merge { "Merging…" } else { "Converting and checking…" });
        }
        label(ui, &self.status.clone(), regular(CALLOUT), p.text);
        if self.function == Function::Merge {
            if let Some(result) = self.merge_result.clone() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if mac_button(ui, p, "Show in Explorer", ButtonKind::Normal, true).clicked() {
                        reveal(&result);
                    }
                    if mac_button(ui, p, "Open in PowerPoint", ButtonKind::Normal, true).clicked() {
                        open(&result);
                    }
                    if mac_button(ui, p, "Checks per file", ButtonKind::Normal, true).clicked() {
                        self.show_checks = true;
                    }
                    if mac_button(ui, p, "Open report", ButtonKind::Normal, true).clicked() {
                        open(&with_suffix(&result, ".report.txt"));
                    }
                });
            }
        } else if let Some(report) = self.tool_report.clone() {
            let output = PathBuf::from(&report.output);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if mac_button(ui, p, "Show in Explorer", ButtonKind::Normal, true).clicked() {
                    reveal(&output);
                }
                if mac_button(ui, p, "Open", ButtonKind::Normal, true).clicked() {
                    open(&output);
                }
                if mac_button(ui, p, "Open report", ButtonKind::Normal, true).clicked() {
                    open(&with_suffix(&output, ".report.json"));
                }
            });
        }
    }

    // ----- Sheets and alerts -----

    fn backdrop(ctx: &egui::Context, id: &str) {
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new(id)).fade_in(false).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(40));
        });
    }

    /// macOS sheets slide out from the top of the window, centred.
    fn sheet(ctx: &egui::Context, p: &Palette, id: &str, width: f32, height: Option<f32>, content: impl FnOnce(&mut egui::Ui)) {
        Self::backdrop(ctx, &format!("{id}-backdrop"));
        let screen = ctx.screen_rect();
        let width = width.min(screen.width() - 40.0);
        egui::Area::new(egui::Id::new(id)).fade_in(false).order(egui::Order::Tooltip).fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + 8.0)).show(ctx, |ui| {
            egui::Frame::NONE.fill(p.sheet).corner_radius(CornerRadius::same(12)).shadow(egui::epaint::Shadow { offset: [0, 8], blur: 30, spread: 0, color: Color32::from_black_alpha(80) }).inner_margin(24.0).show(ui, |ui| {
                ui.set_width(width - 48.0);
                if let Some(h) = height {
                    ui.set_height(h.min(screen.height() - 40.0) - 48.0);
                }
                content(ui);
            });
        });
    }

    fn checks_sheet(&mut self, ctx: &egui::Context, p: &Palette) {
        let validation = self.validation.clone();
        let mut close = false;
        let height = 520.0_f32.min(ctx.screen_rect().height() - 40.0);
        Self::sheet(ctx, p, "checks", 700.0, Some(height), |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            label(ui, "Checks per file", bold(22.0), p.text);
            label(ui, "These checks are structural. They do not certify visual fidelity or playback in PowerPoint.", regular(BODY), p.secondary);
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(height - 48.0 - 110.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for check in &validation {
                    label(ui, &check.file, bold(BODY), p.text);
                    if check.issues.is_empty() {
                        label(ui, "Structural checks passed · review pending", regular(BODY), p.text);
                    } else {
                        label(ui, "IMPORT NOT CORRECT", regular(BODY), p.red);
                    }
                    label(ui, &format!("{} slides · {} with animation · {} resources verified", check.checked_slides, check.slides_with_animation, check.binary_resources_checked), regular(BODY), p.text);
                    for i in &check.issues {
                        label(ui, i, regular(BODY), p.red);
                    }
                    for r in &check.needs_review {
                        label(ui, r, regular(10.0), p.text);
                    }
                    ui.add_space(6.0);
                    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().rect_filled(line, CornerRadius::ZERO, p.separator);
                    ui.add_space(6.0);
                }
            });
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if mac_button(ui, p, "Close", ButtonKind::Normal, true).clicked() || escape {
                close = true;
            }
        });
        if close {
            self.show_checks = false;
        }
    }

    fn settings_sheet(&mut self, ctx: &egui::Context, p: &Palette) {
        let mut settings = self.settings.clone();
        let (mut wt, mut ht) = (self.width_text.clone(), self.height_text.clone());
        let mut done = false;
        Self::sheet(ctx, p, "settings", 480.0, None, |ui| {
            ui.spacing_mut().item_spacing.y = 16.0;
            label(ui, "Settings", bold(22.0), p.text);
            label(ui, "Slide size", bold(BODY), p.text);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for choice in SIZE_CHOICES {
                    if radio(ui, p, settings.slide_size == choice, choice.title()) {
                        settings.slide_size = choice;
                    }
                }
            });
            let custom = settings.slide_size == SizeChoice::Custom;
            let valid = PIXEL_RANGE.contains(&settings.custom_width) && PIXEL_RANGE.contains(&settings.custom_height);
            ui.add_enabled_ui(custom, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    number_field(ui, p, &mut settings.custom_width, &mut wt, "width");
                    ui.label(RichText::new("×").font(regular(BODY)).color(p.text));
                    number_field(ui, p, &mut settings.custom_height, &mut ht, "height");
                    ui.label(RichText::new("px").font(regular(BODY)).color(p.text));
                    if valid {
                        ui.label(RichText::new(format!("{:.2} × {:.2} in", settings.custom_width as f64 / 96.0, settings.custom_height as f64 / 96.0)).font(regular(BODY)).color(p.secondary));
                    }
                });
            });
            if custom && !valid {
                label(ui, "Each side must be between 96 and 5376 px.", regular(BODY), p.red);
            }
            label(ui, "Automatic keeps the size when every presentation has the same size or proportion, and uses 16:9 otherwise. A fixed size scales every slide proportionally and centres it; content is never stretched or cropped. Pixels are measured as in PowerPoint, at 96 per inch.", regular(10.0), p.secondary);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
                if mac_button(ui, p, "Done", ButtonKind::Default, true).clicked() || enter {
                    done = true;
                }
            });
        });
        if settings != self.settings {
            settings.save();
            self.settings = settings;
        }
        self.width_text = wt;
        self.height_text = ht;
        if done {
            self.show_settings = false;
        }
    }

    fn alert_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(alert) = self.alert.clone() else { return };
        Self::backdrop(ctx, "alert-backdrop");
        let width = 300.0;
        let mut chosen = None;
        egui::Area::new(egui::Id::new("alert")).fade_in(false).order(egui::Order::Tooltip).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Frame::NONE.fill(p.sheet).corner_radius(CornerRadius::same(14)).shadow(egui::epaint::Shadow { offset: [0, 8], blur: 30, spread: 0, color: Color32::from_black_alpha(90) }).inner_margin(20.0).show(ui, |ui| {
                ui.set_width(width - 40.0);
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    if let Some(icon) = &self.icon {
                        ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(64.0, 64.0)));
                    }
                    ui.add(egui::Label::new(RichText::new(&alert.title).font(bold(BODY)).color(p.text)).wrap());
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(&alert.text).font(regular(11.0)).color(p.text)).wrap().selectable(true));
                    });
                    for (i, b) in alert.buttons.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(width - 40.0, 24.0), Sense::click());
                        let down = response.is_pointer_button_down_on();
                        let (fill, text) = if i == 0 { (if down { p.accent_down } else { p.accent }, Color32::WHITE) } else { (if down { p.button_down } else if response.hovered() { p.button_hover } else { p.button }, p.text) };
                        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, *b, regular(BODY), text);
                        if response.clicked() || (i == 0 && ui.input(|x| x.key_pressed(egui::Key::Enter))) {
                            chosen = Some(i);
                        }
                    }
                });
            });
        });
        if let Some(i) = chosen {
            self.alert = None;
            if let Some(job) = alert.choice_job {
                if i == 1 {
                    self.start_merge(ctx, Job { fit: true, ..job });
                } else {
                    self.status = "Merge stopped. Change the listed files in PowerPoint, or choose a slide size in Settings, and try again. The originals are untouched.".into();
                }
            }
        }
    }

    // ----- Screenshot hook used to compare with the macOS app -----

    fn snapshot_setup(&mut self) {
        let Ok(path) = std::env::var("DECKWRIGHT_SNAPSHOT") else { return };
        self.snapshot = Some((PathBuf::from(path), 0));
        if let Ok(f) = std::env::var("DECKWRIGHT_SNAP_FN") {
            if let Some(found) = FUNCTIONS.iter().find(|x| x.title() == f) {
                self.set_function(*found);
            }
        }
        if let Ok(files) = std::env::var("DECKWRIGHT_SNAP_FILES") {
            self.files = files.split('|').map(PathBuf::from).collect();
            self.selected = std::env::var("DECKWRIGHT_SNAP_SEL").ok().and_then(|s| s.parse().ok());
        }
        if std::env::var("DECKWRIGHT_SNAP_SETTINGS").is_ok() {
            self.show_settings = true;
        }
        if std::env::var("DECKWRIGHT_SNAP_ALERT").is_ok() {
            self.alert = Some(Alert { title: "Some presentations are not 16:9".into(), text: "These files are not 16:9:\n\n• File 2: “Old.pptx” — 960 × 720 px (25.40 × 19.05 cm).\n\nYou can fit them centred without enlarging or cropping, choose an output size in Settings, or stop and change them yourself. The originals are not modified.".into(), buttons: vec!["Stop and edit manually", "Fit without enlarging"], choice_job: None });
        }
    }

    fn snapshot_step(&mut self, ctx: &egui::Context) {
        let Some((path, frame)) = self.snapshot.as_mut() else { return };
        *frame += 1;
        if *frame == 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let path = path.clone();
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Screenshot { image, .. } = e {
                    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(img) = image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, rgba) {
                        let _ = img.save(&path);
                    }
                    std::process::exit(0);
                }
            }
        });
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(msg);
        }
        if self.icon.is_none() {
            if let Ok(img) = image::load_from_memory(include_bytes!("../assets/icon-256.png")) {
                let img = img.to_rgba8();
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                self.icon = Some(ctx.load_texture("app-icon", color, Default::default()));
            }
        }
        let p = palette(ctx.theme() == egui::Theme::Dark);
        let modal = self.alert.is_some() || self.show_checks || self.show_settings;
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && !self.busy && !modal {
            if self.function == Function::Merge {
                self.add(dropped);
            } else if let Some(f) = dropped.into_iter().next() {
                self.source = Some(f);
                self.tool_report = None;
            }
        }
        let page = egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin { left: 24, right: 24, top: 24, bottom: 24 });
        let enabled = !self.busy && !modal;
        if self.function == Function::Merge {
            egui::TopBottomPanel::bottom("merge-bottom").frame(egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin { left: 24, right: 24, top: 0, bottom: 24 })).show_separator_line(false).show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.add_enabled_ui(enabled, |ui| self.merge_bottom(ui, ctx, &p, modal));
            });
            egui::CentralPanel::default().frame(page).show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.add_enabled_ui(enabled, |ui| {
                    self.header(ui, &p);
                    self.file_list(ui, &p);
                });
            });
        } else {
            egui::CentralPanel::default().frame(page).show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.add_enabled_ui(enabled, |ui| {
                    self.header(ui, &p);
                    self.file_content(ui, ctx, &p, modal);
                });
            });
        }
        if self.show_checks {
            self.checks_sheet(ctx, &p);
        }
        if self.show_settings {
            self.settings_sheet(ctx, &p);
        }
        self.alert_window(ctx, &p);
        self.snapshot_step(ctx);
    }
}

fn open(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}
fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
}

/// Segoe UI on Windows (closest to San Francisco), in regular, semibold and bold.
fn fonts(ctx: &egui::Context) {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let sets: [(&str, [String; 2]); 3] = [
        ("regular", [format!("{windir}\\Fonts\\segoeui.ttf"), "/System/Library/Fonts/Supplemental/Arial.ttf".into()]),
        ("semibold", [format!("{windir}\\Fonts\\seguisb.ttf"), "/System/Library/Fonts/Supplemental/Arial Bold.ttf".into()]),
        ("bold", [format!("{windir}\\Fonts\\segoeuib.ttf"), "/System/Library/Fonts/Supplemental/Arial Bold.ttf".into()]),
    ];
    let mut defs = egui::FontDefinitions::default();
    for (name, candidates) in sets {
        if let Some(data) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
            defs.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        }
    }
    let fallback: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, name) in [(FontFamily::Proportional, "regular"), (FontFamily::Name("semibold".into()), "semibold"), (FontFamily::Name("bold".into()), "bold")] {
        let mut list = vec![];
        if defs.font_data.contains_key(name) {
            list.push(name.to_string());
        } else if defs.font_data.contains_key("regular") {
            list.push("regular".to_string());
        }
        list.extend(fallback.iter().cloned());
        defs.families.insert(family, list);
    }
    ctx.set_fonts(defs);
    ctx.style_mut(|s| {
        s.interaction.selectable_labels = true;
        s.visuals.selection.bg_fill = Color32::from_rgb(0xB3, 0xD7, 0xFF);
    });
}

pub fn run() -> std::result::Result<(), String> {
    let mut viewport = egui::ViewportBuilder::default().with_title("Deckwright").with_app_id("Deckwright").with_inner_size([850.0, 650.0]).with_min_inner_size([640.0, 520.0]).with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Deckwright",
        options,
        Box::new(|cc| {
            fonts(&cc.egui_ctx);
            match std::env::var("DECKWRIGHT_SNAP_THEME").as_deref() {
                Ok("light") => cc.egui_ctx.set_theme(egui::ThemePreference::Light),
                Ok("dark") => cc.egui_ctx.set_theme(egui::ThemePreference::Dark),
                _ => cc.egui_ctx.set_theme(egui::ThemePreference::System),
            }
            Ok(Box::new(App::new()))
        }),
    )
    .map_err(|e| e.to_string())
}
