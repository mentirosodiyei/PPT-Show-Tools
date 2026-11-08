import Foundation

/// Where reports and error logs go. Nothing is written next to the files the
/// user converts: everything goes to ~/Library/Logs/PPT Show Tools, and files there
/// older than 30 days are deleted.
///
/// PPTSHOWTOOLS_LOG_DIR replaces that folder (tests). PPTSHOWTOOLS_REPORTS_NEXT_TO_OUTPUT=1
/// keeps the old behaviour of writing `<output>.report.json` beside the output,
/// for scripts and the test suite.
enum ReportStore {
    static let keepDays = 30
    private static let lock = NSLock()
    private static var bases: [String: URL] = [:]

    static var nextToOutput: Bool { ProcessInfo.processInfo.environment["PPTSHOWTOOLS_REPORTS_NEXT_TO_OUTPUT"] == "1" }

    static var folder: URL {
        if let custom = ProcessInfo.processInfo.environment["PPTSHOWTOOLS_LOG_DIR"], !custom.isEmpty { return URL(fileURLWithPath: custom) }
        return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/PPT Show Tools")
    }

    /// Creates the folder and deletes what is older than 30 days.
    @discardableResult static func prepare() -> URL? {
        let fm = FileManager.default, dir = folder
        do { try fm.createDirectory(at: dir, withIntermediateDirectories: true) } catch { return nil }
        let limit = Date().addingTimeInterval(-Double(keepDays) * 86_400)
        let items = (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.contentModificationDateKey, .isRegularFileKey])) ?? []
        for item in items {
            guard let values = try? item.resourceValues(forKeys: [.contentModificationDateKey, .isRegularFileKey]),
                  values.isRegularFile == true, let date = values.contentModificationDate, date < limit else { continue }
            try? fm.removeItem(at: item)
        }
        return dir
    }

    /// Path without suffix shared by every report file of one output.
    /// The same output gets the same base for the rest of the process.
    static func base(for output: URL) -> URL? {
        if nextToOutput { return output }
        lock.lock(); defer { lock.unlock() }
        if let known = bases[output.standardizedFileURL.path] { return known }
        guard let dir = prepare() else { return nil }
        let stamp = DateFormatter(); stamp.dateFormat = "yyyyMMdd-HHmmss"; stamp.locale = Locale(identifier: "en_US_POSIX")
        let base = dir.appendingPathComponent("\(output.deletingPathExtension().lastPathComponent)-\(stamp.string(from: Date()))-\(UUID().uuidString.prefix(6))")
        bases[output.standardizedFileURL.path] = base
        return base
    }

    /// The report file of an output written earlier in this process, if any.
    static func file(for output: URL, _ suffix: String) -> URL? {
        guard let base = base(for: output) else { return nil }
        let url = URL(fileURLWithPath: base.path + suffix)
        return FileManager.default.fileExists(atPath: url.path) ? url : nil
    }

    static func write(_ data: Data, for output: URL, _ suffix: String) throws {
        guard var base = base(for: output) else { return }
        // A new conversion to a name used earlier in this session starts a new report.
        if !nextToOutput, FileManager.default.fileExists(atPath: base.path + suffix) {
            lock.lock(); bases[output.standardizedFileURL.path] = nil; lock.unlock()
            guard let fresh = self.base(for: output) else { return }
            base = fresh
        }
        try data.write(to: URL(fileURLWithPath: base.path + suffix), options: .withoutOverwriting)
    }

    static func remove(for output: URL, _ suffixes: [String]) {
        guard let base = base(for: output) else { return }
        for suffix in suffixes { try? FileManager.default.removeItem(atPath: base.path + suffix) }
    }

    /// Only when reports go beside the output can they collide with an existing file.
    static func collides(_ output: URL, _ suffixes: [String]) -> Bool {
        nextToOutput && suffixes.contains { FileManager.default.fileExists(atPath: output.path + $0) }
    }

    /// Error log in the logs folder; returns its path, or a note when it could not be written.
    static func logFailure(output: URL, error: Error) -> String {
        let text = Data(error.localizedDescription.utf8)
        let dir = nextToOutput ? output.deletingLastPathComponent() : prepare()
        guard let dir else { return "No log could be written." }
        let log = dir.appendingPathComponent("PPTShowTools-ERROR-" + UUID().uuidString + ".log")
        do { try text.write(to: log, options: .withoutOverwriting); return log.path } catch { return "No log could be written." }
    }
}
