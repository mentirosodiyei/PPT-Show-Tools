import CryptoKit
import Foundation

/// Updates from the project's GitHub releases. A release is installed only if
/// its SHA256SUMS.txt carries a valid Ed25519 signature from the author's key
/// (tools/release-sign.swift) and the download matches its checksum, so a file
/// replaced on GitHub, or by anyone in between, is never installed.
/// Same rules as windows/src/updater.rs.
enum Updater {
    static let repository = "mentirosodiyei/PPT-Show-Tools"
    /// Public half of the release signing key.
    static let releaseKey = "3gayXZS9YvENGojbepLokzqvcMrleqUdcIw/+8+ReaM="
    static let checkInterval: TimeInterval = 12 * 3600
    static let maxDownload = 500 * 1024 * 1024

    struct Release {
        let version: [Int], tag: String, notes: String, page: URL, assets: [String: URL]
        var versionText: String { version.map(String.init).joined(separator: ".") }
    }
    struct Failure: LocalizedError { let message: String; var errorDescription: String? { message } }

    /// The test suite points the updater at a local server with its own key;
    /// that is only accepted for 127.0.0.1.
    private static var testServer: URL? {
        guard let text = ProcessInfo.processInfo.environment["PPTSHOWTOOLS_UPDATE_URL"], let url = URL(string: text), url.host == "127.0.0.1" else { return nil }
        return url
    }
    static var apiURL: URL { testServer ?? URL(string: "https://api.github.com/repos/\(repository)/releases?per_page=10")! }
    static var publicKey: String { testServer != nil ? (ProcessInfo.processInfo.environment["PPTSHOWTOOLS_UPDATE_KEY"] ?? releaseKey) : releaseKey }

    static var currentVersion: String {
        ProcessInfo.processInfo.environment["PPTSHOWTOOLS_VERSION"].flatMap { testServer != nil ? $0 : nil }
            ?? (Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String) ?? "0"
    }

    /// "v0.10.1" → [0, 10, 1].
    static func parse(_ tag: String) -> [Int]? {
        let parts = tag.trimmingCharacters(in: CharacterSet(charactersIn: "vV")).split(separator: ".").map { Int($0) }
        guard (2...4).contains(parts.count), parts.allSatisfy({ $0 != nil }) else { return nil }
        return parts.map { $0! }
    }
    static func newer(_ a: [Int], than b: [Int]) -> Bool {
        for i in 0..<max(a.count, b.count) {
            let x = i < a.count ? a[i] : 0, y = i < b.count ? b[i] : 0
            if x != y { return x > y }
        }
        return false
    }

    /// Only HTTPS from GitHub, or the local test server.
    static func allowed(_ url: URL) -> Bool {
        if testServer != nil { return url.scheme == "http" && url.host == "127.0.0.1" }
        guard url.scheme == "https", let host = url.host?.lowercased() else { return false }
        return ["github.com", "api.github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"].contains(host)
    }

    final class Guard: NSObject, URLSessionTaskDelegate {
        func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
            completionHandler(request.url.map(Updater.allowed) == true ? request : nil)
        }
    }

    static func fetch(_ url: URL, limit: Int = 4 * 1024 * 1024) throws -> Data {
        guard allowed(url) else { throw Failure(message: "Refused to download from \(url.host ?? "an unknown host").") }
        var request = URLRequest(url: url, timeoutInterval: 60)
        request.setValue("PPTShowTools/\(currentVersion)", forHTTPHeaderField: "User-Agent")
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        let session = URLSession(configuration: .ephemeral, delegate: Guard(), delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        var result: Result<Data, Error> = .failure(Failure(message: "No answer from GitHub."))
        let done = DispatchSemaphore(value: 0)
        session.dataTask(with: request) { data, response, error in
            defer { done.signal() }
            if let error { result = .failure(error); return }
            guard let http = response as? HTTPURLResponse, http.statusCode == 200, let data else { result = .failure(Failure(message: "GitHub answered with an error.")); return }
            result = data.count <= limit ? .success(data) : .failure(Failure(message: "The download is larger than expected."))
        }.resume()
        done.wait()
        return try result.get()
    }

    /// The newest published release newer than this app, or nil.
    static func latest() throws -> Release? {
        guard let list = try JSONSerialization.jsonObject(with: fetch(apiURL)) as? [[String: Any]] else { throw Failure(message: "Unexpected answer from GitHub.") }
        let current = parse(currentVersion) ?? [0]
        var best: Release?
        for item in list where (item["draft"] as? Bool) != true {
            guard let tag = item["tag_name"] as? String, let version = parse(tag), newer(version, than: current),
                  let page = (item["html_url"] as? String).flatMap(URL.init(string:)) else { continue }
            var assets: [String: URL] = [:]
            for asset in item["assets"] as? [[String: Any]] ?? [] {
                if let name = asset["name"] as? String, let link = (asset["browser_download_url"] as? String).flatMap(URL.init(string:)) { assets[name] = link }
            }
            let release = Release(version: version, tag: tag, notes: item["body"] as? String ?? "", page: page, assets: assets)
            if best == nil || newer(version, than: best!.version) { best = release }
        }
        return best
    }

    static var architecture: String {
        var info = utsname(); uname(&info)
        let machine = withUnsafeBytes(of: &info.machine) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
        return machine == "arm64" ? "AppleSilicon" : "Intel"
    }
    static func assetName(_ release: Release) -> String { "PPTShowTools-\(release.versionText)-\(architecture).dmg" }

    /// Checksums whose signature is valid, by file name.
    static func verifiedChecksums(_ release: Release) throws -> [String: String] {
        guard let sumsURL = release.assets["SHA256SUMS.txt"], let sigURL = release.assets["SHA256SUMS.txt.sig"] else {
            throw Failure(message: "This release is not signed, so it is not installed automatically.")
        }
        let sums = try fetch(sumsURL), sigText = String(decoding: try fetch(sigURL), as: UTF8.self)
        guard let keyData = Data(base64Encoded: publicKey), let key = try? Curve25519.Signing.PublicKey(rawRepresentation: keyData),
              let signature = Data(base64Encoded: sigText.trimmingCharacters(in: .whitespacesAndNewlines)), key.isValidSignature(signature, for: sums) else {
            throw Failure(message: "The release's signature is not valid. The update was not installed.")
        }
        var out: [String: String] = [:]
        for line in String(decoding: sums, as: UTF8.self).split(separator: "\n") {
            let parts = line.split(separator: " ", maxSplits: 1).map { $0.trimmingCharacters(in: CharacterSet(charactersIn: " *")) }
            if parts.count == 2, parts[0].count == 64 { out[parts[1]] = parts[0].lowercased() }
        }
        return out
    }

    /// Downloads this Mac's disk image and checks it; returns its path in a new temporary folder.
    static func download(_ release: Release, progress: (String) -> Void = { _ in }) throws -> URL {
        let name = assetName(release)
        progress("Checking the release signature…")
        let sums = try verifiedChecksums(release)
        guard let expected = sums[name], let url = release.assets[name] else { throw Failure(message: "This release has no \(name).") }
        progress("Downloading \(name)…")
        let data = try fetch(url, limit: maxDownload)
        guard checksum(data) == expected else { throw Failure(message: "The download does not match the release checksum. The update was not installed.") }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("PPTShowTools-Update-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let file = folder.appendingPathComponent(name)
        try data.write(to: file)
        return file
    }

    /// Replaces `app` with the app in the verified disk image `dmg`. The running
    /// app is moved aside (macOS keeps it running) and removed on the next launch.
    @discardableResult
    static func install(dmg: URL, over app: URL, expectedVersion: String) throws -> URL {
        let fm = FileManager.default
        let mount = fm.temporaryDirectory.appendingPathComponent("PPTShowTools-Mount-" + UUID().uuidString)
        try fm.createDirectory(at: mount, withIntermediateDirectories: true)
        try run("/usr/bin/hdiutil", ["attach", "-nobrowse", "-noautoopen", "-readonly", "-mountpoint", mount.path, dmg.path])
        defer { try? run("/usr/bin/hdiutil", ["detach", "-quiet", "-force", mount.path]); try? fm.removeItem(at: mount) }
        let source = mount.appendingPathComponent("PPT Show Tools.app")
        guard let info = NSDictionary(contentsOf: source.appendingPathComponent("Contents/Info.plist")),
              info["CFBundleIdentifier"] as? String == Bundle.main.bundleIdentifier ?? "io.github.mentirosodiyei.pptshowtools",
              info["CFBundleShortVersionString"] as? String == expectedVersion else {
            throw Failure(message: "The disk image does not contain the expected version of PPT Show Tools.")
        }
        let folder = app.deletingLastPathComponent()
        guard fm.isWritableFile(atPath: folder.path), !app.path.contains("/AppTranslocation/"), !app.path.hasPrefix("/Volumes/") else {
            throw Failure(message: "PPT Show Tools cannot replace itself where it is (\(folder.path)). Move it to Applications, or install the update by hand.")
        }
        let staged = folder.appendingPathComponent(".PPT Show Tools-new-\(UUID().uuidString.prefix(8)).app")
        let old = folder.appendingPathComponent(".PPT Show Tools-old-\(UUID().uuidString.prefix(8)).app")
        try run("/usr/bin/ditto", ["--noqtn", source.path, staged.path])
        do {
            try fm.moveItem(at: app, to: old)
            do { try fm.moveItem(at: staged, to: app) } catch { try? fm.moveItem(at: old, to: app); throw error }
        } catch {
            try? fm.removeItem(at: staged); throw error
        }
        return app
    }

    /// Removes the copies left by an update, once the new version runs.
    static func cleanUp() {
        let folder = Bundle.main.bundleURL.deletingLastPathComponent()
        for item in (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? [] where item.hasPrefix(".PPT Show Tools-old-") || item.hasPrefix(".PPT Show Tools-new-") {
            try? FileManager.default.removeItem(at: folder.appendingPathComponent(item))
        }
    }

    /// Short release notes for the update window: Markdown headings and links simplified.
    static func plainNotes(_ notes: String) -> String {
        let text = notes.replacingOccurrences(of: "**", with: "").replacingOccurrences(of: "`", with: "")
            .replacingOccurrences(of: #"\[([^\]]+)\]\([^)]+\)"#, with: "$1", options: .regularExpression)
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.hasPrefix("#") ? String($0.drop { $0 == "#" || $0 == " " }).uppercased() : String($0) }
            .joined(separator: "\n")
        return text.count > 4000 ? String(text.prefix(4000)) + "…" : text
    }
}
