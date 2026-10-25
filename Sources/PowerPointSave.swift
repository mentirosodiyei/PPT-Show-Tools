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
          if p is missing value then error "PowerPoint no ha obert la presentació en 10 minuts. Si mostra el diàleg «Concedeix accés», selecciona la carpeta de destinació (no el fitxer) i prem Concedeix accés; després torna-ho a provar."
          set n to count slides of p
          if n is not expected then
            close presentation sourceName saving no
            error "PowerPoint ha obert " & n & " diapositives en lloc de " & expected & "."
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
        guard NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) != nil else { try fail("No s’ha trobat Microsoft PowerPoint.") }
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
            try fail("PowerPoint no ha respost a temps. Pot ser que mostri un diàleg (accés a la carpeta, reparació del fitxer o inici de sessió). Revisa PowerPoint; el resultat no s’ha validat.")
        }
        readers.wait()
        let message = String(decoding: errors, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard task.terminationStatus == 0 else {
            if message.contains("-1743") {
                try fail("Cal autoritzar PPT Merger a controlar Microsoft PowerPoint: Configuració del Sistema → Privacitat i seguretat → Automatització.")
            }
            try fail("PowerPoint no ha pogut desar la fusió: " + (message.isEmpty ? "error desconegut" : message))
        }
        let parts = String(decoding: output, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines).split(separator: "|")
        guard parts.count == 2, let slides = Int(parts[0]) else { try fail("Resposta inesperada de PowerPoint.") }
        return (slides, parts[1] == "closed")
    }

    static func merge(inputs: [URL], output: URL, fitNonWidescreen: Bool = false, progress: (String) -> Void = { _ in }) throws -> PowerPointMergeReport {
        let fm = FileManager.default
        guard output.pathExtension.lowercased() == "pptx" else { try fail("El motor amb PowerPoint desa en PPTX. Per conservar macros, fes servir la fusió Open XML en PPTM.") }
        guard !fm.fileExists(atPath: output.path), !fm.fileExists(atPath: output.path + ".report.json"), !fm.fileExists(atPath: output.path + ".report.txt") else { try fail("El fitxer de sortida o l’informe ja existeix. Tria un altre nom.") }
        // PowerPoint is sandboxed: the intermediate lives in the destination folder,
        // so a single folder access grant covers both files.
        let intermediate = output.deletingLastPathComponent().appendingPathComponent("PPT-Merger-temporal-\(UUID().uuidString.prefix(8)).pptx")
        defer { for suffix in ["", ".report.json", ".report.txt"] { try? fm.removeItem(atPath: intermediate.path + suffix) } }
        progress("Fusionant i comprovant amb Open XML…")
        let openXML = try OpenXMLMerger().merge(inputs: inputs, output: intermediate, fitNonWidescreen: fitNonWidescreen)
        guard openXML.structuralValidationPassed else {
            try fail("La fusió Open XML no supera la comprovació; no s’envia a PowerPoint.\n" + openXML.validation.flatMap { v in v.issues.prefix(5).map { v.file + ": " + $0 } }.joined(separator: "\n"))
        }
        progress("Obrint i desant amb PowerPoint… Si PowerPoint mostra «Concedeix accés», selecciona la carpeta (no el fitxer) o una carpeta superior, com Documents, i prem Concedeix accés. Només cal fer-ho una vegada per carpeta.")
        let saved = try resave(intermediate, to: output, expectedSlides: openXML.totalSlides)
        progress("Comprovant el fitxer desat per PowerPoint…")
        // The Open XML report describes a merge made without PowerPoint; that line no longer applies.
        var warnings = ["Fusió Open XML desada amb Microsoft PowerPoint per AppleScript."] + openXML.warnings.filter { !$0.hasPrefix("Motor Open XML directe") }
        warnings.append("Desat amb Microsoft PowerPoint a partir de la fusió Open XML. Cada presentació conserva el seu patró i tema, com «Mantenir el format d’origen», però no s’ha executat l’ordre d’enganxar de PowerPoint.")
        if !saved.closed { warnings.append("PowerPoint no ha pogut tancar el resultat; continua obert. Tanca’l sense desar canvis.") }
        var issues: [String] = []
        if saved.slides != openXML.totalSlides { issues.append("PowerPoint ha desat \(saved.slides) diapositives; se n’esperaven \(openXML.totalSlides).") }
        let checks = try OfficePipeline.verify(inputs: [intermediate], output: output).map {
            FileValidation(file: "Fusió Open XML → desat per PowerPoint", status: issues.isEmpty ? $0.status : "import_failed_validation", checkedSlides: $0.checkedSlides, slidesWithAnimation: $0.slidesWithAnimation, binaryResourcesChecked: $0.binaryResourcesChecked, checks: $0.checks, issues: issues + $0.issues, needsReview: [
                "PowerPoint ha obert i desat la fusió; no s’ha executat cap enganxament. No es certifica l’aspecte, les fonts, l’edició ni la reproducció.",
                "PowerPoint reorganitza parts i identificadors en desar; es compara el contingut, no els bytes."
            ])
        }
        let passed = checks.allSatisfy { $0.issues.isEmpty }
        let report = PowerPointMergeReport(engine: "OpenXML+PowerPoint-AppleEvents-v0.6.0", inputFiles: inputs.map { $0.path }, output: output.path, openXML: openXML, powerPointSlides: saved.slides, powerPointValidation: checks, overallStatus: passed ? "structural_checks_passed_needs_review" : "import_failed_validation", warnings: warnings)
        do {
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(report).write(to: URL(fileURLWithPath: output.path + ".report.json"), options: .withoutOverwriting)
            var lines = ["PPT MERGER — FUSIÓ OPEN XML DESADA AMB POWERPOINT", passed ? "Comprovació estructural superada. Revisió visual i reproducció pendents." : "IMPORTACIÓ NO CORRECTA: diferències detectades.", ""]
            for (i, check) in report.validation.enumerated() {
                lines += ["\(i + 1). \(check.file)", check.issues.isEmpty ? "ESTRUCTURA: comprovacions superades; no equival a fidelitat total." : "IMPORTACIÓ NO CORRECTA"]
                lines += check.issues.map { "ERROR: " + $0 } + check.needsReview.map { "REVISIÓ PENDENT: " + $0 } + [""]
            }
            lines += ["AVISOS"] + warnings
            try Data(lines.joined(separator: "\n").utf8).write(to: URL(fileURLWithPath: output.path + ".report.txt"), options: .withoutOverwriting)
        } catch {
            try? fm.removeItem(at: URL(fileURLWithPath: output.path + ".report.json"))
            throw error
        }
        return report
    }
}
