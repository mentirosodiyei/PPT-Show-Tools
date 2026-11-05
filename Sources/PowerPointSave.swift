import AppKit
import Foundation

struct PowerPointMergeReport: Codable {
    let engine: String
    let inputFiles: [String]
    let output: String
    let openXML: MergeReport
    let powerPointSlides: Int
    let powerPointValidation: [FileValidation]
    let overallStatus: String
    let warnings: [String]
    var passed: Bool { openXML.structuralValidationPassed && powerPointValidation.allSatisfy { $0.issues.isEmpty } }
    var validation: [FileValidation] { openXML.validation + powerPointValidation }
}

/// Native PowerPoint step driven through Apple Events (Automation permission),
/// never through Accessibility, synthetic keys, the clipboard or window focus.
/// PowerPoint's AppleScript can open, count, save and close; it cannot paste
/// with "Keep Source Formatting" (verified on 16.114: paste uses the
/// destination theme). The Open XML merge already keeps each source master, so
/// PowerPoint re-saves that merge into its own canonical package.
struct PowerPointSave {
    static let bundleID = "com.microsoft.Powerpoint"
    // Paths travel as arguments, never interpolated into the script source.
    // Presentations are addressed by name: iterating "presentations" and
    // closing references hangs PowerPoint 16.114.
    static let script = """
    on run argv
      set sourcePath to item 1 of argv
      set sourceName to item 2 of argv
      set targetPath to item 3 of argv
      set targetName to item 4 of argv
      set expected to (item 5 of argv) as integer
      tell application id "com.microsoft.Powerpoint"
        with timeout of 1800 seconds
          -- The sandbox "Grant access" dialog can swallow the open event's reply,
          -- so never wait for it: send it once and poll. Resending would queue
          -- one more dialog per attempt.
          ignoring application responses
            open POSIX file sourcePath
          end ignoring
          set p to missing value
          repeat 1200 times
            try
              set p to presentation sourceName
              exit repeat
            on error
              delay 0.5
            end try
          end repeat
          if p is missing value then error "PowerPoint did not open the presentation within 10 minutes. If it shows “Grant Access”, select the destination folder (not the file), click Grant Access and try again."
          set n to count slides of p
          if n is not expected then
            close presentation sourceName saving no
            error "PowerPoint opened " & n & " slides instead of " & expected & "."
          end if
          save p in POSIX file targetPath
          set closed to "closed"
          try
            close presentation targetName saving no
          on error
            set closed to "open"
          end try
        end timeout
      end tell
      return (n as text) & "|" & closed
    end run
    """

    /// Opens `source` in PowerPoint, saves it as `target` and closes it.
    /// Returns the slide count PowerPoint reported and whether it closed the file.
    static func resave(_ source: URL, to target: URL, expectedSlides: Int, timeout: TimeInterval = 1900) throws -> (slides: Int, closed: Bool) {
        guard NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) != nil else { try fail("Microsoft PowerPoint was not found.") }
        let task = Process(), out = Pipe(), err = Pipe()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        task.arguments = ["-e", script, source.path, source.lastPathComponent, target.path, target.lastPathComponent, String(expectedSlides)]
        task.standardOutput = out; task.standardError = err
        let finished = DispatchSemaphore(value: 0)
        task.terminationHandler = { _ in finished.signal() }
        try task.run()
        // Read both pipes while waiting so a large message never blocks osascript.
        var output = Data(), errors = Data()
        let readers = DispatchGroup()
        DispatchQueue.global().async(group: readers) { output = out.fileHandleForReading.readDataToEndOfFile() }
        DispatchQueue.global().async(group: readers) { errors = err.fileHandleForReading.readDataToEndOfFile() }
        if finished.wait(timeout: .now() + timeout) == .timedOut {
            task.terminate()
            try fail("PowerPoint did not respond in time. It may be showing a dialog (folder access, file repair or sign-in). Check PowerPoint; the result was not validated.")
        }
        readers.wait()
        let message = String(decoding: errors, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard task.terminationStatus == 0 else {
            if message.contains("-1743") {
                try fail("Allow Deckwright to control Microsoft PowerPoint: System Settings → Privacy & Security → Automation.")
            }
            try fail("PowerPoint could not save the merge: " + (message.isEmpty ? "unknown error" : message))
        }
        let parts = String(decoding: output, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines).split(separator: "|")
        guard parts.count == 2, let slides = Int(parts[0]) else { try fail("Unexpected reply from PowerPoint.") }
        return (slides, parts[1] == "closed")
    }

    static func merge(inputs: [URL], output: URL, fixedSize: (Int64, Int64)? = nil, progress: (String) -> Void = { _ in }) throws -> PowerPointMergeReport {
        let fm = FileManager.default
        guard output.pathExtension.lowercased() == "pptx" else { try fail("Saving with PowerPoint produces PPTX. To keep macros, use Merge with a PPTM output.") }
        guard !fm.fileExists(atPath: output.path), !ReportStore.collides(output, [".report.json", ".report.txt"]) else { try fail("The output file or its report already exists. Choose another name.") }
        // PowerPoint is sandboxed: the intermediate lives in the destination folder,
        // so a single folder access grant covers both files.
        let intermediate = output.deletingLastPathComponent().appendingPathComponent("Deckwright-temp-\(UUID().uuidString.prefix(8)).pptx")
        defer { try? fm.removeItem(at: intermediate) }
        progress("Merging and checking…")
        let openXML = try OpenXMLMerger().merge(inputs: inputs, output: intermediate, fixedSize: fixedSize, writeReports: false)
        guard openXML.structuralValidationPassed else {
            try fail("The Open XML merge failed its checks; it is not sent to PowerPoint.\n" + openXML.validation.flatMap { v in v.issues.prefix(5).map { v.file + ": " + $0 } }.joined(separator: "\n"))
        }
        progress("Opening and saving with PowerPoint… If PowerPoint shows “Grant Access”, select the folder (not the file) or a parent folder such as Documents, and click Grant Access. This is needed once per folder.")
        let saved = try resave(intermediate, to: output, expectedSlides: openXML.totalSlides)
        progress("Checking the file saved by PowerPoint…")
        // The Open XML report describes a merge made without PowerPoint; that line no longer applies.
        var warnings = ["Open XML merge saved with Microsoft PowerPoint through AppleScript."] + openXML.warnings.filter { !$0.hasPrefix("Direct Open XML engine") }
        warnings.append("Saved with Microsoft PowerPoint from the Open XML merge. Each presentation keeps its master and theme, like “Keep Source Formatting”, but PowerPoint's paste command was not run.")
        if !saved.closed { warnings.append("PowerPoint could not close the result; it is still open. Close it without saving changes.") }
        var issues: [String] = []
        if saved.slides != openXML.totalSlides { issues.append("PowerPoint saved \(saved.slides) slides; expected \(openXML.totalSlides).") }
        // A result that cannot even be checked is not left behind as if it were valid.
        let verified: [FileValidation]
        do { verified = try OfficePipeline.verify(inputs: [intermediate], output: output) } catch { try? fm.removeItem(at: output); throw error }
        let checks = verified.map {
            FileValidation(file: "Open XML merge → saved by PowerPoint", status: issues.isEmpty ? $0.status : "import_failed_validation", checkedSlides: $0.checkedSlides, slidesWithAnimation: $0.slidesWithAnimation, binaryResourcesChecked: $0.binaryResourcesChecked, checks: $0.checks, issues: issues + $0.issues, needsReview: [
                "PowerPoint opened and saved the merge; nothing was pasted. Appearance, fonts, editing and playback are not certified.",
                "PowerPoint reorganises parts and identifiers when saving; the content is compared, not the bytes."
            ])
        }
        let passed = checks.allSatisfy { $0.issues.isEmpty }
        let report = PowerPointMergeReport(engine: "Deckwright-OpenXML+PowerPoint-AppleEvents-v0.8.0", inputFiles: inputs.map { $0.path }, output: output.path, openXML: openXML, powerPointSlides: saved.slides, powerPointValidation: checks, overallStatus: passed ? "structural_checks_passed_needs_review" : "import_failed_validation", warnings: warnings)
        do {
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try ReportStore.write(try encoder.encode(report), for: output, ".report.json")
            var lines = ["DECKWRIGHT — OPEN XML MERGE SAVED WITH POWERPOINT", passed ? "Structural checks passed. Visual and playback review pending." : "IMPORT NOT CORRECT: differences found.", ""]
            for (i, check) in report.validation.enumerated() {
                lines += ["\(i + 1). \(check.file)", check.issues.isEmpty ? "STRUCTURE: checks passed; this is not full fidelity." : "IMPORT NOT CORRECT"]
                lines += check.issues.map { "ERROR: " + $0 } + check.needsReview.map { "TO REVIEW: " + $0 } + [""]
            }
            lines += ["WARNINGS"] + warnings
            try ReportStore.write(Data(lines.joined(separator: "\n").utf8), for: output, ".report.txt")
        } catch {
            ReportStore.remove(for: output, [".report.json", ".report.txt"])
            throw error
        }
        return report
    }
}
