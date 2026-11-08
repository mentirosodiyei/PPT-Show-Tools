import SwiftUI
import AppKit

/// Output slide size chosen in Settings. Presets and custom sizes are in pixels
/// at 96 per inch, as PowerPoint measures them.
enum SlideSizeChoice: String, CaseIterable, Identifiable {
    case automatic, hd720 = "1280x720", hd1080 = "1920x1080", uhd = "3840x2160", standard43 = "1024x768", custom
    var id: String { rawValue }
    var title: String {
        switch self {
        case .automatic: return "Automatic"
        case .hd720: return "16:9 · 1280 × 720 px (PowerPoint standard)"
        case .hd1080: return "16:9 · 1920 × 1080 px (Full HD)"
        case .uhd: return "16:9 · 3840 × 2160 px (4K)"
        case .standard43: return "4:3 · 1024 × 768 px"
        case .custom: return "Custom"
        }
    }
    var pixels: (Int, Int)? {
        switch self {
        case .hd720: return (1280, 720)
        case .hd1080: return (1920, 1080)
        case .uhd: return (3840, 2160)
        case .standard43: return (1024, 768)
        case .automatic, .custom: return nil
        }
    }
    /// PowerPoint allows 1–56 inches per side.
    static let pixelRange = 96...5376
    /// nil means automatic. Throws for a custom size outside PowerPoint's limits.
    static func fixedSize(choice: String, width: Int, height: Int) throws -> (Int64, Int64)? {
        let selected = SlideSizeChoice(rawValue: choice) ?? .automatic
        guard selected != .automatic else { return nil }
        let (w, h) = selected.pixels ?? (width, height)
        guard pixelRange.contains(w), pixelRange.contains(h) else { try fail("Each side of the slide size must be between 96 and 5376 px. Change it in Settings.") }
        return (Int64(w) * emuPerPixel, Int64(h) * emuPerPixel)
    }
}

@main
struct PPTShowToolsApp: App {
    init() {
        if CommandLine.arguments.count > 1 { exit(Self.runCommandLine(CommandLine.arguments)) }
        Self.forgetBrokenWindowFrames()
        Self.importDeckwrightSettings()
        DispatchQueue.global(qos: .utility).async { ReportStore.prepare() }  // deletes logs older than 30 days
    }

    /// This app was called Deckwright up to 0.8.0: bring over its slide size once.
    static func importDeckwrightSettings() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: "importedDeckwrightSettings") else { return }
        defaults.set(true, forKey: "importedDeckwrightSettings")
        guard defaults.object(forKey: "slideSize") == nil, let old = UserDefaults(suiteName: "io.github.mentirosodiyei.deckwright") else { return }
        for key in ["slideSize", "customWidth", "customHeight"] {
            if let value = old.object(forKey: key) { defaults.set(value, forKey: key) }
        }
    }

    /// Versions 0.7.0–0.7.2 could save a window as tall as (or taller than) the
    /// screen, and macOS keeps restoring it. Forget saved window frames once, so
    /// the window opens at its default size again.
    static func forgetBrokenWindowFrames() {
        let defaults = UserDefaults.standard, flag = "windowFramesReset-0.7.3"
        guard !defaults.bool(forKey: flag) else { return }
        for key in defaults.dictionaryRepresentation().keys where key.hasPrefix("NSWindow Frame") { defaults.removeObject(forKey: key) }
        defaults.set(true, forKey: flag)
        defaults.set(true, forKey: WindowFitter.resetPendingKey)
    }

    /// Same commands as the Windows build, used by the test suite.
    static func runCommandLine(_ all: [String]) -> Int32 {
        var args: [String] = [], size: String?, i = 0
        while i < all.count {
            // --fit-non169 is accepted for compatibility: every size is now adapted.
            if all[i] == "--fit-non169" { i += 1; continue }
            if all[i] == "--size", i + 1 < all.count { size = all[i + 1]; i += 2; continue }
            args.append(all[i]); i += 1
        }
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        func paths(_ from: Int) -> [URL] { args.dropFirst(from).map { URL(fileURLWithPath: $0) } }
        func printJSON<T: Encodable>(_ value: T) throws { print(String(data: try encoder.encode(value), encoding: .utf8)!) }
        do {
            // --size WIDTHxHEIGHT in pixels, or "auto".
            var fixed: (Int64, Int64)? = nil
            if let size, size != "auto" {
                let parts = size.lowercased().split(separator: "x").compactMap { Int($0) }
                guard parts.count == 2 else { try fail("--size must be WIDTHxHEIGHT in pixels, for example 4800x1344, or auto.") }
                fixed = try SlideSizeChoice.fixedSize(choice: SlideSizeChoice.custom.rawValue, width: parts[0], height: parts[1])
            }
            let command = args.count > 1 ? args[1] : ""
            switch command {
            case "--office-prepare" where args.count >= 4:
                try printJSON(try OfficePipeline.prepare(inputs: paths(3), destination: URL(fileURLWithPath: args[2]))); return 0
            case "--office-verify" where args.count >= 4:
                let checks = try OfficePipeline.verify(inputs: paths(3), output: URL(fileURLWithPath: args[2]))
                try printJSON(checks); return checks.allSatisfy { $0.issues.isEmpty } ? 0 : 2
            case "--verify" where args.count >= 5:
                let checks = try PackageVerifier.inspect(inputs: paths(3), output: URL(fileURLWithPath: args[2]))
                try printJSON(checks); return checks.allSatisfy { $0.issues.isEmpty } ? 0 : 2
            case "--pdf-to-ppt", "--resize", "--to-169", "--extract-notes":
                guard args.count == 4 else { break }
                let input = URL(fileURLWithPath: args[3]), output = URL(fileURLWithPath: args[2])
                do {
                    let report: ToolReport
                    switch command {
                    case "--pdf-to-ppt": report = try PresentationTools.pdfToPPT(input: input, output: output, fixedSize: fixed)
                    case "--extract-notes": report = try PresentationTools.extractNotes(input: input, output: output)
                    default: report = try PresentationTools.resize(input: input, output: output, fixedSize: command == "--to-169" ? nil : fixed)
                    }
                    try printJSON(report); return 0
                } catch {
                    let log = PresentationTools.logFailure(output: output, error: error)
                    fputs(error.localizedDescription + "\nLog: " + log + "\n", stderr); return 1
                }
            case "--merge-powerpoint" where args.count >= 5:
                let report = try PowerPointSave.merge(inputs: paths(3), output: URL(fileURLWithPath: args[2]), fixedSize: fixed) { fputs($0 + "\n", stderr) }
                print("\(report.powerPointSlides) slides saved by PowerPoint. Report: \(ReportStore.file(for: URL(fileURLWithPath: args[2]), ".report.json")?.path ?? "none")"); return report.passed ? 0 : 2
            case "--merge" where args.count >= 5:
                let report = try OpenXMLMerger().merge(inputs: paths(3), output: URL(fileURLWithPath: args[2]), fixedSize: fixed)
                print("\(report.totalSlides) slides. Report: \(ReportStore.file(for: URL(fileURLWithPath: args[2]), ".report.json")?.path ?? "none")"); return report.structuralValidationPassed ? 0 : 2
            default: break
            }
            try fail("""
            Usage: PPTShowTools --merge [--size WxH] output.pptx input1.pptx input2.pptx […]
                   PPTShowTools --merge-powerpoint [--size WxH] output.pptx input1.pptx input2.pptx […]
                   PPTShowTools --resize [--size WxH] output.pptx input.pptx
                   PPTShowTools --pdf-to-ppt output.pptx input.pdf
                   PPTShowTools --extract-notes output.pptx input.pptx
            """)
        } catch {
            fputs(error.localizedDescription + "\n", stderr); return 1
        }
    }

    var body: some Scene {
        WindowGroup("PPT Show Tools") { MainView() }.defaultSize(width: 850, height: 650).windowResizability(.contentMinSize)
    }
}
