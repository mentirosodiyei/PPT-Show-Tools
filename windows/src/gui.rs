//! Window that reproduces the macOS app: same layout, controls, colours,
//! texts and behaviour, in light or dark mode following the system.
use crate::merge::MergeReport;
use crate::package::{extension, file_name, with_suffix};
use crate::powerpoint::PowerPointMergeReport;
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
    capsule: Color32,
    capsule_stroke: Color32,
    pill: Color32,
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
            capsule: hex(0x2A2A2A),
            capsule_stroke: hex(0x3A3A3A),
            pill: hex(0x5A5A5A),
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
            capsule: hex(0xF4F4F4),
            capsule_stroke: hex(0xE0E0E0),
            pill: hex(0xFFFFFF),
            sheet: hex(0xFFFFFF),
            orange: hex(0xFF9500),
            red: hex(0xFF3B30),
        }
    }
}

fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
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

/// Toolbar tabs: centred capsule with a pill on the selected tab.
fn toolbar_tabs(ui: &mut egui::Ui, p: &Palette, tab: &mut Tab) {
    let full = ui.max_rect();
    ui.painter().rect_filled(full, CornerRadius::ZERO, p.bg);
    ui.painter().hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0_f32, p.separator));
    let labels = [(Tab::Merge, "Fusiona"), (Tab::Tools, "Converteix i extreu")];
    let widths: Vec<f32> = labels.iter().map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), semibold(BODY), p.text).size().x).collect();
    let segment = widths.iter().cloned().fold(0.0, f32::max) + 18.0;
    let capsule = Rect::from_center_size(pos2(full.center().x, full.top() + 26.0), vec2(segment * 2.0 + 14.0, 36.0));
    ui.painter().rect(capsule, CornerRadius::same(18), p.capsule, Stroke::new(1.0_f32, p.capsule_stroke), egui::StrokeKind::Inside);
    for (i, (value, text)) in labels.iter().enumerate() {
        let r = Rect::from_min_size(pos2(capsule.left() + 4.0 + i as f32 * (segment + 6.0), capsule.top() + 4.0), vec2(segment, 28.0));
        let response = ui.interact(r, ui.id().with(("tab", i)), Sense::click());
        if *tab == *value {
            ui.painter().add(egui::epaint::Shadow { offset: [0, 1], blur: 3, spread: 0, color: Color32::from_black_alpha(30) }.as_shape(r, CornerRadius::same(14)));
            ui.painter().rect_filled(r, CornerRadius::same(14), p.pill);
        }
        let font = if *tab == *value { semibold(BODY) } else { regular(BODY) };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *text, font, p.text);
        if response.clicked() {
            *tab = *value;
        }
    }
}

/// Segmented picker as in the macOS Tools tab: blue selected segment, dividers.
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

fn spinner_row(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(16.0).color(p.secondary));
        ui.label(RichText::new(text).font(regular(BODY)).color(p.text));
    });
}

// ---------- App state ----------

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Merge,
    Tools,
}
#[derive(PartialEq, Clone, Copy)]
enum Operation {
    Pdf,
    Wide,
    Notes,
}
const OPERATIONS: [Operation; 3] = [Operation::Pdf, Operation::Wide, Operation::Notes];
impl Operation {
    fn title(self) -> &'static str {
        match self {
            Operation::Pdf => "PDF a PowerPoint",
            Operation::Wide => "Converteix a 16:9",
            Operation::Notes => "Extreu les notes",
        }
    }
    fn explanation(self) -> &'static str {
        match self {
            Operation::Pdf => "Una pàgina PDF = una diapositiva amb una imatge. Es conserva la proporció de la primera pàgina; text no editable.",
            Operation::Wide => "Eixampla el llenç a 16:9 i centra el contingut sense estirar-lo, ampliar-lo ni retallar-lo. Conserva els marges laterals de l’original 4:3.",
            Operation::Notes => "N diapositives originals = N diapositives noves de notes en 16:9. Fons negre, text blanc editable i mida adaptada a cada pàgina. Sense notes: «No Notes on this slide» en mida discreta.",
        }
    }
}

#[derive(Clone)]
struct Job {
    inputs: Vec<PathBuf>,
    output: PathBuf,
    fit: bool,
    powerpoint: bool,
}
enum Outcome {
    OpenXml(MergeReport),
    PowerPoint(PowerPointMergeReport),
}
enum Msg {
    Progress(String),
    Merged(Job, std::result::Result<Outcome, Error>),
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
    tab: Tab,
    files: Vec<PathBuf>,
    selected: Option<usize>,
    busy: bool,
    status: String,
    result: Option<PathBuf>,
    validation: Vec<FileValidation>,
    show_checks: bool,
    alert: Option<Alert>,
    operation: Operation,
    source: Option<PathBuf>,
    tool_status: String,
    tool_report: Option<ToolReport>,
    tool_busy: bool,
    icon: Option<egui::TextureHandle>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    snapshot: Option<(PathBuf, u32)>,
}

impl App {
    fn new() -> App {
        let (tx, rx) = channel();
        let mut app = App {
            tab: Tab::Merge,
            files: vec![],
            selected: None,
            busy: false,
            status: "Afegeix els PowerPoint en l’ordre que vulguis fusionar-los.".into(),
            result: None,
            validation: vec![],
            show_checks: false,
            alert: None,
            operation: Operation::Pdf,
            source: None,
            tool_status: "Selecciona una funció i el fitxer original.".into(),
            tool_report: None,
            tool_busy: false,
            icon: None,
            tx,
            rx,
            snapshot: None,
        };
        app.snapshot_setup();
        app
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
        self.result = None;
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
                    crate::powerpoint::merge(&job.inputs, &job.output, job.fit, &progress).map(Outcome::PowerPoint)
                } else {
                    Err(Error::Fail("No s’ha trobat Microsoft PowerPoint.".into()))
                }
            } else {
                crate::merge::merge(&job.inputs, &job.output, job.fit).map(Outcome::OpenXml)
            };
            let _ = tx.send(Msg::Merged(job, outcome));
            ctx.request_repaint();
        });
    }

    fn merge(&mut self, ctx: &egui::Context, powerpoint: bool) {
        let macro_first = !powerpoint && self.files.first().map(|f| extension(f) == "pptm").unwrap_or(false);
        let ext = if macro_first { "pptm" } else { "pptx" };
        let mut dialog = rfd::FileDialog::new().set_file_name(format!("Presentacio_fusionada.{ext}"));
        if let Some(dir) = self.files.first().and_then(|f| f.parent()) {
            dialog = dialog.set_directory(dir);
        }
        dialog = if powerpoint {
            dialog.add_filter("PowerPoint (PPTX)", &["pptx"])
        } else if macro_first {
            dialog.add_filter("PowerPoint amb macros (PPTM)", &["pptm"]).add_filter("PowerPoint (PPTX)", &["pptx"])
        } else {
            dialog.add_filter("PowerPoint (PPTX)", &["pptx"]).add_filter("PowerPoint amb macros (PPTM)", &["pptm"])
        };
        if let Some(output) = dialog.save_file() {
            self.start_merge(ctx, Job { inputs: self.files.clone(), output, fit: false, powerpoint });
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Progress(m) => self.status = m,
            Msg::Merged(job, Ok(outcome)) => {
                self.busy = false;
                self.result = Some(job.output.clone());
                let (passed, validation, slides, by_powerpoint) = match &outcome {
                    Outcome::OpenXml(r) => (r.structural_validation_passed(), r.validation.clone(), r.total_slides, false),
                    Outcome::PowerPoint(r) => (r.passed(), r.validation(), r.power_point_slides, true),
                };
                self.validation = validation;
                self.show_checks = true;
                self.status = match (passed, by_powerpoint) {
                    (true, false) => format!("{slides} diapositives. Comprovació estructural superada; revisió visual i reproducció pendents."),
                    (true, true) => format!("{slides} diapositives desades per PowerPoint. Comprovació estructural superada; revisió visual i reproducció pendents."),
                    (false, false) => "Importació NO correcta: s’han detectat diferències. Consulta les comprovacions per fitxer abans d’utilitzar el resultat.".into(),
                    (false, true) => "Importació NO correcta després de desar amb PowerPoint. Consulta les comprovacions abans d’utilitzar el resultat.".into(),
                };
                if !passed {
                    let text = self.validation.iter().filter(|v| !v.issues.is_empty()).map(|v| format!("{}:\n{}", v.file, v.issues.iter().take(5).cloned().collect::<Vec<_>>().join("\n"))).collect::<Vec<_>>().join("\n\n");
                    self.alert = Some(Alert { title: "La importació no és correcta".into(), text, buttons: vec!["D’acord"], choice_job: None });
                }
            }
            Msg::Merged(job, Err(Error::FormatChoice(message))) => {
                self.busy = false;
                self.alert = Some(Alert { title: "Hi ha presentacions que no són 16:9".into(), text: message, buttons: vec!["Atura i modifica manualment", "Encaixa sense ampliar"], choice_job: Some(job) });
            }
            Msg::Merged(_, Err(Error::Fail(message))) => {
                self.busy = false;
                self.status = message;
            }
            Msg::Tool(result) => {
                self.tool_busy = false;
                match result {
                    Ok(report) => {
                        self.tool_status = format!("{} diapositives creades. Comprovacions superades; revisa l’aspecte i els avisos de l’informe.", report.pages.len());
                        self.tool_report = Some(report);
                    }
                    Err(message) => self.tool_status = message,
                }
            }
        }
    }

    fn start_tool(&mut self, ctx: &egui::Context) {
        let Some(source) = self.source.clone() else { return };
        let ext = if self.operation == Operation::Wide { extension(&source) } else { "pptx".into() };
        let suffix = match self.operation {
            Operation::Notes => "-notes",
            Operation::Wide => "-16x9",
            Operation::Pdf => "-presentacio",
        };
        let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}{suffix}.{ext}")).add_filter("PowerPoint", &[ext.as_str()]);
        if let Some(dir) = source.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(output) = dialog.save_file() else { return };
        let op = self.operation;
        self.tool_busy = true;
        self.tool_report = None;
        self.tool_status = "Convertint i comprovant…".into();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match op {
                Operation::Pdf => tools::pdf_to_ppt(&source, &output),
                Operation::Wide => tools::widescreen(&source, &output),
                Operation::Notes => tools::extract_notes(&source, &output),
            };
            let result = result.map_err(|e| {
                let log = tools::log_failure(&source, &e.to_string());
                format!("ATURAT: {e}\nRegistre: {log}")
            });
            let _ = tx.send(Msg::Tool(result));
            ctx.request_repaint();
        });
    }

    // ----- Fusiona -----

    fn merge_header(&mut self, ui: &mut egui::Ui, p: &Palette) {
        label(ui, "PPT Merger", bold(26.0), p.text);
        label(ui, "Windows · Fusió directa Open XML (sense obrir PowerPoint)", regular(BODY), p.secondary);
        label(ui, "La mida 16:9 de sortida s’adapta a la més gran de les entrades. Els altres formats es poden encaixar centrats sense ampliar ni retallar ni modificar-ne el contingut. Es conserven els màsters, dissenys, temes, patrons de notes, animacions, transicions, multimèdia i fonts incrustades de cada presentació.", regular(CALLOUT), p.text);
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

    fn merge_bottom(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, p: &Palette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if mac_button(ui, p, "Afegeix…", ButtonKind::Normal, true).clicked() {
                if let Some(files) = rfd::FileDialog::new().add_filter("PowerPoint", &["pptx", "pptm"]).pick_files() {
                    self.add(files);
                }
            }
            let sel = self.selected.filter(|i| *i < self.files.len());
            if mac_button(ui, p, "Amunt", ButtonKind::Normal, sel.is_some()).clicked() {
                if let Some(i) = sel.filter(|i| *i > 0) {
                    self.files.swap(i, i - 1);
                    self.selected = Some(i - 1);
                }
            }
            if mac_button(ui, p, "Avall", ButtonKind::Normal, sel.is_some()).clicked() {
                if let Some(i) = sel.filter(|i| i + 1 < self.files.len()) {
                    self.files.swap(i, i + 1);
                    self.selected = Some(i + 1);
                }
            }
            if mac_button(ui, p, "Elimina", ButtonKind::Normal, sel.is_some()).clicked() {
                if let Some(i) = sel {
                    self.files.remove(i);
                    self.selected = None;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let can = self.files.len() >= 2;
                let enter = can && ui.is_enabled() && self.alert.is_none() && !self.show_checks && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if mac_button(ui, p, "Fusiona presentacions…", ButtonKind::Default, can).clicked() || enter {
                    self.merge(ctx, false);
                }
                let help = "Fusiona amb Open XML i fa que Microsoft PowerPoint obri i desi el resultat. No controla el teclat ni el ratolí.";
                if mac_button(ui, p, "Fusiona i desa amb PowerPoint…", ButtonKind::Normal, can).on_hover_text(help).clicked() {
                    self.merge(ctx, true);
                }
            });
        });
        if self.busy {
            spinner_row(ui, p, "Fusionant…");
        }
        label(ui, &self.status.clone(), regular(CALLOUT), p.text);
        if let Some(result) = self.result.clone() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if mac_button(ui, p, "Mostra el resultat", ButtonKind::Normal, true).clicked() {
                    reveal(&result);
                }
                if mac_button(ui, p, "Obre amb PowerPoint", ButtonKind::Normal, true).clicked() {
                    open(&result);
                }
                if mac_button(ui, p, "Comprovacions per fitxer", ButtonKind::Normal, true).clicked() {
                    self.show_checks = true;
                }
                if mac_button(ui, p, "Obre l’informe", ButtonKind::Normal, true).clicked() {
                    open(&with_suffix(&result, ".report.txt"));
                }
            });
        }
    }

    // ----- Converteix i extreu -----

    fn tools_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, p: &Palette) {
        label(ui, "Eines de presentacions", bold(26.0), p.text);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("Funció").font(regular(BODY)).color(p.text));
            let titles: Vec<&str> = OPERATIONS.iter().map(|o| o.title()).collect();
            let current = OPERATIONS.iter().position(|o| *o == self.operation).unwrap_or(0);
            if let Some(i) = segmented(ui, p, &titles, current) {
                if OPERATIONS[i] != self.operation {
                    self.operation = OPERATIONS[i];
                    self.source = None;
                    self.tool_report = None;
                    self.tool_status = "Selecciona el fitxer original.".into();
                }
            }
        });
        ui.allocate_ui(vec2(ui.available_width(), 44.0), |ui| {
            ui.set_min_height(44.0);
            label(ui, self.operation.explanation(), regular(CALLOUT), p.text);
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if mac_button(ui, p, "Selecciona el fitxer…", ButtonKind::Normal, true).clicked() {
                let dialog = if self.operation == Operation::Pdf { rfd::FileDialog::new().add_filter("PDF", &["pdf"]) } else { rfd::FileDialog::new().add_filter("PowerPoint", &["pptx", "pptm"]) };
                if let Some(f) = dialog.pick_file() {
                    self.source = Some(f);
                    self.tool_report = None;
                    self.tool_status = "Preparat per crear una còpia nova.".into();
                }
            }
            ui.label(RichText::new(self.source.as_ref().map(|s| file_name(s)).unwrap_or_else(|| "Cap fitxer seleccionat".into())).font(regular(BODY)).color(p.text));
        });
        label(ui, "Es crea un fitxer nou. L’original es conserva. Les comprovacions es fan automàticament i es desen en un informe.", regular(CALLOUT), p.secondary);
        let can = self.source.is_some();
        let enter = can && ui.is_enabled() && self.alert.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if mac_button(ui, p, &format!("{}…", self.operation.title()), ButtonKind::Default, can).clicked() || enter {
            self.start_tool(ctx);
        }
        if self.tool_busy {
            spinner_row(ui, p, "Convertint i comprovant…");
        }
        label(ui, &self.tool_status.clone(), regular(CALLOUT), p.text);
        if let Some(report) = self.tool_report.clone() {
            let output = PathBuf::from(&report.output);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if mac_button(ui, p, "Mostra el resultat", ButtonKind::Normal, true).clicked() {
                    reveal(&output);
                }
                if mac_button(ui, p, "Obre el resultat", ButtonKind::Normal, true).clicked() {
                    open(&output);
                }
                if mac_button(ui, p, "Obre l’informe", ButtonKind::Normal, true).clicked() {
                    open(&with_suffix(&output, ".report.json"));
                }
            });
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for c in &report.checks {
                    label(ui, &format!("✓ {c}"), regular(BODY), p.text);
                }
                for page in report.pages.iter().filter(|x| !x.warnings.is_empty()) {
                    label(ui, &format!("Diapositiva {}: {}", page.source_page, page.warnings.join(" ")), regular(BODY), p.orange);
                }
                for l in &report.limitations {
                    label(ui, l, regular(10.0), p.secondary);
                }
            });
        }
    }

    // ----- Sheet and alerts -----

    fn checks_sheet(&mut self, ctx: &egui::Context, p: &Palette) {
        let screen = ctx.screen_rect();
        let size = vec2(700.0_f32.min(screen.width() - 40.0), 520.0_f32.min(screen.height() - 70.0));
        // macOS sheets slide out under the toolbar, centred, and dim the window slightly.
        egui::Area::new(egui::Id::new("sheet-backdrop")).fade_in(false).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(40));
        });
        egui::Area::new(egui::Id::new("sheet")).fade_in(false).order(egui::Order::Tooltip).fixed_pos(pos2(screen.center().x - size.x / 2.0, screen.top() + 60.0)).show(ctx, |ui| {
            egui::Frame::NONE.fill(p.sheet).corner_radius(CornerRadius::same(12)).shadow(egui::epaint::Shadow { offset: [0, 8], blur: 30, spread: 0, color: Color32::from_black_alpha(80) }).inner_margin(24.0).show(ui, |ui| {
                ui.set_width(size.x - 48.0);
                ui.set_height(size.y - 48.0);
                ui.spacing_mut().item_spacing.y = 12.0;
                label(ui, "Comprovacions per PowerPoint", bold(22.0), p.text);
                label(ui, "El control és estructural. No certifica la fidelitat visual ni la reproducció a PowerPoint.", regular(BODY), p.secondary);
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(size.y - 48.0 - 110.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for check in &self.validation {
                        label(ui, &check.file, bold(BODY), p.text);
                        if check.issues.is_empty() {
                            label(ui, "Comprovació estructural superada · revisió pendent", regular(BODY), p.text);
                        } else {
                            label(ui, "IMPORTACIÓ NO CORRECTA", regular(BODY), p.red);
                        }
                        label(ui, &format!("{} diapositives · {} amb animació · {} recursos verificats", check.checked_slides, check.slides_with_animation, check.binary_resources_checked), regular(BODY), p.text);
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
                if mac_button(ui, p, "Tanca", ButtonKind::Normal, true).clicked() || escape {
                    self.show_checks = false;
                }
            });
        });
    }

    fn alert_window(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(alert) = self.alert.clone() else { return };
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("alert-backdrop")).fade_in(false).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(40));
        });
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
                        let kind = if i == 0 { ButtonKind::Default } else { ButtonKind::Normal };
                        let galley_w = width - 40.0;
                        let (rect, response) = ui.allocate_exact_size(vec2(galley_w, 24.0), Sense::click());
                        let down = response.is_pointer_button_down_on();
                        let (fill, text) = if kind == ButtonKind::Default { (if down { p.accent_down } else { p.accent }, Color32::WHITE) } else { (if down { p.button_down } else if response.hovered() { p.button_hover } else { p.button }, p.text) };
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
                    self.status = "Fusió aturada. Modifica els fitxers indicats a PowerPoint i torna-ho a provar. Els originals estan intactes.".into();
                }
            }
        }
    }

    // ----- Screenshot hook used to compare with the macOS app -----

    fn snapshot_setup(&mut self) {
        let Ok(path) = std::env::var("PPTMERGER_SNAPSHOT") else { return };
        self.snapshot = Some((PathBuf::from(path), 0));
        if std::env::var("PPTMERGER_SNAP_TAB").as_deref() == Ok("1") {
            self.tab = Tab::Tools;
        }
        if let Ok(files) = std::env::var("PPTMERGER_SNAP_FILES") {
            self.files = files.split('|').map(PathBuf::from).collect();
            self.selected = std::env::var("PPTMERGER_SNAP_SEL").ok().and_then(|s| s.parse().ok());
        }
        if std::env::var("PPTMERGER_SNAP_CHECKS").is_ok() {
            self.validation = vec![FileValidation { file: "LivePremier 1.pptx".into(), status: "ok".into(), checked_slides: 159, slides_with_animation: 12, binary_resources_checked: 40, checks: vec![], issues: vec![], needs_review: vec!["No comprovat: aspecte visual, geometria final, fonts heretades, reproducció d’animacions/àudio/vídeo, edició i enllaços externs.".into()] }];
            self.show_checks = true;
        }
        if std::env::var("PPTMERGER_SNAP_ALERT").is_ok() {
            self.alert = Some(Alert { title: "Hi ha presentacions que no són 16:9".into(), text: "Aquests fitxers no tenen proporció 16:9:\n\n• Fitxer 2: «Antiga.pptx» — 25,40 × 19,05 cm.\n\nPots encaixar-los centrats sense ampliar-los ni retallar-los, o aturar la fusió per modificar-los manualment. Els originals no es modificaran.".into(), buttons: vec!["Atura i modifica manualment", "Encaixa sense ampliar"], choice_job: None });
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
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && self.tab == Tab::Merge && !self.busy {
            self.add(dropped);
        }
        let modal = self.alert.is_some() || self.show_checks;
        egui::TopBottomPanel::top("toolbar").exact_height(52.0).frame(egui::Frame::NONE.fill(p.bg)).show_separator_line(false).show(ctx, |ui| {
            ui.add_enabled_ui(!modal, |ui| toolbar_tabs(ui, &p, &mut self.tab));
        });
        let page = egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin { left: 24, right: 24, top: 24, bottom: 24 });
        if self.tab == Tab::Merge {
            egui::TopBottomPanel::bottom("merge-bottom").frame(egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin { left: 24, right: 24, top: 0, bottom: 24 })).show_separator_line(false).show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.add_enabled_ui(!self.busy && !modal, |ui| self.merge_bottom(ui, ctx, &p));
            });
            egui::CentralPanel::default().frame(page).show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.add_enabled_ui(!self.busy && !modal, |ui| {
                    self.merge_header(ui, &p);
                    self.file_list(ui, &p);
                });
            });
        } else {
            egui::CentralPanel::default().frame(page).show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.add_enabled_ui(!self.tool_busy && !modal, |ui| self.tools_tab(ui, ctx, &p));
            });
        }
        if self.show_checks {
            self.checks_sheet(ctx, &p);
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
    let mut viewport = egui::ViewportBuilder::default().with_title("PPT Merger").with_inner_size([850.0, 650.0]).with_min_inner_size([640.0, 520.0]).with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "PPT Merger",
        options,
        Box::new(|cc| {
            fonts(&cc.egui_ctx);
            match std::env::var("PPTMERGER_SNAP_THEME").as_deref() {
                Ok("light") => cc.egui_ctx.set_theme(egui::ThemePreference::Light),
                Ok("dark") => cc.egui_ctx.set_theme(egui::ThemePreference::Dark),
                _ => cc.egui_ctx.set_theme(egui::ThemePreference::System),
            }
            Ok(Box::new(App::new()))
        }),
    )
    .map_err(|e| e.to_string())
}
