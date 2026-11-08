import SwiftUI
import AppKit
import UniformTypeIdentifiers

enum DeckFunction: String, CaseIterable, Identifiable {
    case merge = "Merge", pdf = "PDF to PowerPoint", resize = "Resize slides", notes = "Extract notes"
    var id: String { rawValue }
}

struct MainView: View {
    @AppStorage("slideSize") private var sizeChoice = SlideSizeChoice.automatic.rawValue
    @AppStorage("customWidth") private var customWidth = 1920
    @AppStorage("customHeight") private var customHeight = 1080
    @State private var function = DeckFunction.merge
    @State private var files: [URL] = []
    @State private var selected: URL?
    @State private var source: URL?
    @State private var busy = false
    @State private var status = "Add the presentations in the order you want to merge them."
    @State private var mergeResult: URL?
    @State private var validation: [FileValidation] = []
    @State private var toolReport: ToolReport?
    @State private var showChecks = false
    @State private var showSettings = false

    private var isAutomatic: Bool { (SlideSizeChoice(rawValue: sizeChoice) ?? .automatic) == .automatic }
    private var sizeSummary: String {
        let choice = SlideSizeChoice(rawValue: sizeChoice) ?? .automatic
        if choice == .automatic { return "Automatic (the size every presentation shares; otherwise 16:9)" }
        let (w, h) = choice.pixels ?? (customWidth, customHeight)
        return "\(w) × \(h) px" + (choice == .custom ? " (custom)" : "")
    }
    private var explanation: String {
        switch function {
        case .merge:
            return "Combines the presentations in the order shown. Each one keeps its masters, themes, animations, transitions, media and embedded fonts, and is adapted to the slide size first. Slide size: \(sizeSummary)."
        case .pdf:
            return isAutomatic
                ? "One slide per PDF page, as an image, using the first page's proportion; text is not editable. Slide size: Automatic."
                : "One slide per PDF page, as an image fitted to \(sizeSummary); text is not editable."
        case .resize:
            return isAutomatic
                ? "Widens the slides to 16:9, adapting the layout: nothing is stretched, text boxes and full-width elements follow the new shape. Choose another size in Settings."
                : "Adapts the slides to \(sizeSummary): nothing is stretched, text boxes and full-width elements follow the new shape, pictures spanning the slide are cropped."
        case .notes:
            return "One black 16:9 slide per original slide, with its speaker notes as editable white text. Slides without notes show “No Notes on this slide”."
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .firstTextBaseline) {
                Text("PPT Show Tools").font(.largeTitle.bold())
                Spacer()
                Button("Settings…") { showSettings = true }
            }
            Picker("Function", selection: $function) {
                ForEach(DeckFunction.allCases) { Text($0.rawValue).tag($0) }
            }.pickerStyle(.segmented).fixedSize()
            Text(explanation).font(.callout).foregroundStyle(.secondary)
            if function == .merge { mergeContent } else { fileContent }
            if busy { ProgressView(function == .merge ? "Merging…" : "Converting and checking…") }
            Text(status).font(.callout).textSelection(.enabled)
            resultButtons
            if let toolReport, function != .merge { reportDetails(toolReport) }
            if function != .merge { Spacer(minLength: 0) }
        }
        .padding(24).frame(minWidth: 680, maxWidth: .infinity, minHeight: 520, maxHeight: .infinity, alignment: .topLeading)
        .disabled(busy)
        .onChange(of: function) {
            source = nil; toolReport = nil
            status = function == .merge ? "Add the presentations in the order you want to merge them." : "Choose the original file."
        }
        .background(WindowFitter())
        .sheet(isPresented: $showChecks) { ChecksSheet(validation: validation) { showChecks = false } }
        .sheet(isPresented: $showSettings) { SettingsSheet { showSettings = false } }
    }

    // MARK: Merge

    private var mergeContent: some View {
        VStack(alignment: .leading, spacing: 16) {
            List(selection: $selected) {
                ForEach(Array(files.enumerated()), id: \.element) { i, file in
                    Text("\(i + 1). \(file.lastPathComponent)").tag(file)
                }
            }.onDrop(of: [UTType.fileURL], isTargeted: nil) { providers in
                // Keep the dropped order even if file delivery is asynchronous.
                Task { @MainActor in
                    var urls: [URL] = []
                    for provider in providers {
                        let data: Data? = await withCheckedContinuation { continuation in
                            _ = provider.loadDataRepresentation(forTypeIdentifier: UTType.fileURL.identifier) { d, _ in continuation.resume(returning: d) }
                        }
                        if let data, let url = URL(dataRepresentation: data, relativeTo: nil) { urls.append(url) }
                    }
                    add(urls)
                }
                return true
            }
            HStack {
                Button("Add…") {
                    let panel = NSOpenPanel(); panel.allowsMultipleSelection = true; panel.allowedContentTypes = presentationTypes
                    if panel.runModal() == .OK { add(panel.urls) }
                }
                Button("Up") { move(-1) }.disabled(selected == nil)
                Button("Down") { move(1) }.disabled(selected == nil)
                Button("Remove") { files.removeAll { $0 == selected }; selected = nil }.disabled(selected == nil)
                Spacer()
                Button("Merge and save with PowerPoint…") { merge(powerPoint: true) }.disabled(files.count < 2)
                    .help("Merges the presentations and has Microsoft PowerPoint open and save the result. Your keyboard and mouse are not used.")
                Button("Merge…") { merge(powerPoint: false) }.disabled(files.count < 2).keyboardShortcut(.defaultAction)
            }
        }
    }

    private var presentationTypes: [UTType] { [UTType(filenameExtension: "pptx")!, UTType(filenameExtension: "pptm")!] }
    private func add(_ urls: [URL]) { for url in urls where ["pptx", "pptm"].contains(url.pathExtension.lowercased()) && !files.contains(url) { files.append(url) } }
    private func move(_ delta: Int) {
        guard let selected, let i = files.firstIndex(of: selected), files.indices.contains(i + delta) else { return }
        files.swapAt(i, i + delta)
    }

    private func merge(powerPoint: Bool) {
        let fixed: (Int64, Int64)?
        do { fixed = try SlideSizeChoice.fixedSize(choice: sizeChoice, width: customWidth, height: customHeight) } catch { status = error.localizedDescription; return }
        let isMacro = !powerPoint && files.first?.pathExtension.lowercased() == "pptm"
        let panel = NSSavePanel()
        panel.directoryURL = files.first?.deletingLastPathComponent()
        panel.nameFieldStringValue = "Merged presentation.\(isMacro ? "pptm" : "pptx")"
        panel.allowedContentTypes = powerPoint ? [UTType(filenameExtension: "pptx")!] : isMacro ? presentationTypes.reversed() : presentationTypes
        guard panel.runModal() == .OK, let output = panel.url else { return }
        runMerge(inputs: files, output: output, fixed: fixed, powerPoint: powerPoint)
    }

    private func runMerge(inputs: [URL], output: URL, fixed: (Int64, Int64)?, powerPoint: Bool) {
        busy = true; mergeResult = nil; validation = []
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let passed: Bool, slides: Int, checks: [FileValidation]
                if powerPoint {
                    guard NSWorkspace.shared.urlForApplication(withBundleIdentifier: PowerPointSave.bundleID) != nil else { try fail("Microsoft PowerPoint was not found.") }
                    let report = try PowerPointSave.merge(inputs: inputs, output: output, fixedSize: fixed) { message in DispatchQueue.main.async { status = message } }
                    (passed, slides, checks) = (report.passed, report.powerPointSlides, report.validation)
                } else {
                    let report = try OpenXMLMerger().merge(inputs: inputs, output: output, fixedSize: fixed)
                    (passed, slides, checks) = (report.structuralValidationPassed, report.totalSlides, report.validation)
                }
                DispatchQueue.main.async {
                    mergeResult = output; validation = checks; busy = false; showChecks = true
                    status = passed
                        ? "\(slides) slides\(powerPoint ? " saved by PowerPoint" : ""). Structural checks passed; visual and playback review pending."
                        : "Import NOT correct: differences found. Check the results per file before using the output."
                    if !passed {
                        let alert = NSAlert(); alert.alertStyle = .warning; alert.messageText = "The import is not correct"
                        alert.informativeText = checks.filter { !$0.issues.isEmpty }.map { $0.file + ":\n" + $0.issues.prefix(5).joined(separator: "\n") }.joined(separator: "\n\n")
                        alert.runModal()
                    }
                }
            } catch {
                DispatchQueue.main.async { status = error.localizedDescription; busy = false }
            }
        }
    }

    // MARK: Single-file functions

    private var actionTitle: String {
        switch function {
        case .pdf: return "Convert to PowerPoint…"
        case .resize: return "Resize…"
        case .notes: return "Extract notes…"
        case .merge: return "Merge…"
        }
    }

    private var fileContent: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Button("Choose file…", action: choose)
                Text(source?.lastPathComponent ?? "No file selected").lineLimit(2).textSelection(.enabled)
            }
            Text("A new file is created; the original is not changed. The checks run automatically.").font(.callout).foregroundStyle(.secondary)
            Button(actionTitle, action: startTool).disabled(source == nil).keyboardShortcut(.defaultAction)
        }
    }

    private func choose() {
        let panel = NSOpenPanel(); panel.allowedContentTypes = function == .pdf ? [.pdf] : presentationTypes
        if panel.runModal() == .OK { source = panel.url; toolReport = nil; status = "Ready to create a new copy." }
    }

    private func startTool() {
        guard let source else { return }
        let fixed: (Int64, Int64)?
        do { fixed = try SlideSizeChoice.fixedSize(choice: sizeChoice, width: customWidth, height: customHeight) } catch { status = error.localizedDescription; return }
        let task = function
        let ext = task == .resize ? source.pathExtension.lowercased() : "pptx"
        let suffix = task == .notes ? " notes" : task == .resize ? " resized" : ""
        let panel = NSSavePanel()
        panel.directoryURL = source.deletingLastPathComponent()
        panel.nameFieldStringValue = source.deletingPathExtension().lastPathComponent + suffix + "." + ext
        panel.allowedContentTypes = [UTType(filenameExtension: ext)!]
        guard panel.runModal() == .OK, let output = panel.url else { return }
        busy = true; toolReport = nil; mergeResult = nil; status = "Converting and checking…"
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let report: ToolReport
                switch task {
                case .pdf: report = try PresentationTools.pdfToPPT(input: source, output: output, fixedSize: fixed)
                case .resize: report = try PresentationTools.resize(input: source, output: output, fixedSize: fixed)
                case .notes: report = try PresentationTools.extractNotes(input: source, output: output)
                case .merge: return
                }
                DispatchQueue.main.async { toolReport = report; busy = false; status = "\(report.pages.count) slides created. Checks passed; review the appearance and the warnings in the report." }
            } catch {
                let log = PresentationTools.logFailure(output: output, error: error)
                DispatchQueue.main.async { busy = false; status = "STOPPED: " + error.localizedDescription + "\nLog: " + log }
            }
        }
    }

    // MARK: Results

    @ViewBuilder private var resultButtons: some View {
        if function == .merge, let mergeResult {
            HStack {
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([mergeResult]) }
                Button("Open in PowerPoint") { openInPowerPoint(mergeResult) }
                Button("Checks per file") { showChecks = true }
                if let report = ReportStore.file(for: mergeResult, ".report.txt") { Button("Open report") { NSWorkspace.shared.open(report) } }
            }
        } else if function != .merge, let toolReport {
            let output = URL(fileURLWithPath: toolReport.output)
            HStack {
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([output]) }
                Button("Open") { NSWorkspace.shared.open(output) }
                if let report = ReportStore.file(for: output, ".report.json") { Button("Open report") { NSWorkspace.shared.open(report) } }
            }
        }
    }

    private func reportDetails(_ report: ToolReport) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(report.checks, id: \.self) { Text("✓ " + $0) }
                ForEach(report.pages.filter { !$0.warnings.isEmpty }, id: \.sourcePage) { page in
                    Text("Slide \(page.sourcePage): " + page.warnings.joined(separator: " ")).foregroundStyle(.orange)
                }
                ForEach(report.limitations, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func openInPowerPoint(_ url: URL) {
        guard let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: PowerPointSave.bundleID) else { status = "Microsoft PowerPoint was not found."; return }
        NSWorkspace.shared.open([url], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration()) { _, error in
            if let error { Task { @MainActor in status = error.localizedDescription } }
        }
    }
}

struct ChecksSheet: View {
    let validation: [FileValidation]
    let close: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Checks per file").font(.title2.bold())
            Text("These checks are structural. They do not certify visual fidelity or playback in PowerPoint.").foregroundStyle(.secondary)
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    ForEach(Array(validation.enumerated()), id: \.offset) { _, check in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(check.file).font(.headline).textSelection(.enabled)
                            Text(check.issues.isEmpty ? "Structural checks passed · review pending" : "IMPORT NOT CORRECT").foregroundStyle(check.issues.isEmpty ? Color.primary : Color.red)
                            Text("\(check.checkedSlides) slides · \(check.slidesWithAnimation) with animation · \(check.binaryResourcesChecked) resources verified")
                            ForEach(check.issues, id: \.self) { Text($0).foregroundStyle(.red) }
                            ForEach(check.needsReview, id: \.self) { Text($0).font(.caption) }
                        }
                        Divider()
                    }
                }
            }
            Button("Close", action: close).keyboardShortcut(.cancelAction)
        }.padding(24).frame(width: 700, height: 520)
    }
}

struct SettingsSheet: View {
    @AppStorage("slideSize") private var sizeChoice = SlideSizeChoice.automatic.rawValue
    @AppStorage("customWidth") private var customWidth = 1920
    @AppStorage("customHeight") private var customHeight = 1080
    @State private var widthText = ""
    @State private var heightText = ""
    let close: () -> Void
    /// Applied as it is typed (not only on Return), and typing selects Custom.
    private func pixelField(_ title: String, value: Binding<Int>, text: Binding<String>) -> some View {
        TextField(title, text: text).frame(width: 80)
            .onAppear { text.wrappedValue = String(value.wrappedValue) }
            .onChange(of: text.wrappedValue) {
                let digits = text.wrappedValue.filter(\.isNumber)
                if digits != text.wrappedValue { text.wrappedValue = digits }
                guard let number = Int(digits), number != value.wrappedValue else { return }
                value.wrappedValue = number
                sizeChoice = SlideSizeChoice.custom.rawValue
            }
    }
    private var customValid: Bool { SlideSizeChoice.pixelRange.contains(customWidth) && SlideSizeChoice.pixelRange.contains(customHeight) }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Settings").font(.title2.bold())
            Text("Slide size").font(.headline)
            Picker("Slide size", selection: $sizeChoice) {
                ForEach(SlideSizeChoice.allCases) { Text($0.title).tag($0.rawValue) }
            }.pickerStyle(.radioGroup).labelsHidden()
            HStack {
                pixelField("Width", value: $customWidth, text: $widthText)
                Text("×")
                pixelField("Height", value: $customHeight, text: $heightText)
                Text("px")
                if customValid { Text(String(format: "%.2f × %.2f in", Double(customWidth) / 96, Double(customHeight) / 96)).foregroundStyle(.secondary) }
            }
            if sizeChoice == SlideSizeChoice.custom.rawValue && !customValid {
                Text("Each side must be between 96 and 5376 px.").foregroundStyle(.red)
            }
            Text("Automatic keeps the size every presentation shares (or their common proportion, at the largest size); with mixed proportions it uses 16:9. Typing a size selects Custom. Every presentation is adapted to the chosen size: nothing is stretched, text boxes and full-width elements follow the new shape, and pictures spanning the slide are cropped. Pixels are measured as in PowerPoint, at 96 per inch.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                Text("PPT Show Tools \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "") · David Redondo García · github.com/mentirosodiyei/PPT-Show-Tools")
                    .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                Spacer(); Button("Done", action: close).keyboardShortcut(.defaultAction)
            }
        }.padding(24).frame(width: 480)
    }
}

/// macOS restores the last window frame. Version 0.7.0 could save a window taller
/// than the screen, which hid the controls and pushed sheets off screen; a frame
/// that does not fit the visible screen is replaced by the default size, centred.
struct WindowFitter: NSViewRepresentable {
    /// Set when saved frames were just forgotten: open once at the default size.
    static let resetPendingKey = "windowFrameResetPending"
    final class Probe: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            DispatchQueue.main.async { [weak self] in
                guard let window = self?.window, let screen = window.screen ?? NSScreen.main else { return }
                let visible = screen.visibleFrame
                let pending = UserDefaults.standard.bool(forKey: WindowFitter.resetPendingKey)
                UserDefaults.standard.removeObject(forKey: WindowFitter.resetPendingKey)
                // A maximised or full-screen window is the user's choice; only an
                // off-screen frame or the one-time reset is corrected.
                guard !window.styleMask.contains(.fullScreen), pending || !visible.intersects(window.frame) || window.frame.height > visible.height + 1 else { return }
                let size = NSSize(width: min(850, visible.width), height: min(650 + window.frame.height - window.contentLayoutRect.height, visible.height))
                window.setFrame(NSRect(x: visible.midX - size.width / 2, y: visible.midY - size.height / 2, width: size.width, height: size.height), display: true)
            }
        }
    }
    func makeNSView(context: Context) -> NSView { Probe() }
    func updateNSView(_ nsView: NSView, context: Context) {}
}
