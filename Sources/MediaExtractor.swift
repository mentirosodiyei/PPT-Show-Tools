import Foundation

/// Extract media: the pictures, sounds and videos placed on chosen slides,
/// copied byte for byte into a new folder. Same rules as windows/src/media.rs.
enum MediaKind: String, CaseIterable, Identifiable {
    case images, audio, video
    var id: String { rawValue }
    var title: String { self == .images ? "Images" : self == .audio ? "Audio" : "Video" }
    var word: String { self == .images ? "image" : rawValue }
}

extension PresentationTools {
    static let audioExtensions: Set<String> = ["mp3", "wav", "m4a", "aac", "wma", "mid", "midi", "aif", "aiff", "flac", "ogg", "oga"]
    static let videoExtensions: Set<String> = ["mp4", "m4v", "mov", "wmv", "avi", "mpg", "mpeg", "mkv", "webm", "asf", "3gp"]

    /// "all", "" or a list such as "1-3, 5, 8" → slide numbers (1-based), in order, without repeats.
    static func slideSelection(_ spec: String, count: Int) throws -> [Int] {
        let text = spec.trimmingCharacters(in: .whitespaces).lowercased()
        if text.isEmpty || text == "all" { return Array(1...max(count, 1)).filter { $0 <= count } }
        var out: [Int] = []
        for part in text.split(separator: ",") {
            let bounds = part.split(separator: "-", omittingEmptySubsequences: false).map { $0.trimmingCharacters(in: .whitespaces) }
            guard (1...2).contains(bounds.count), let first = Int(bounds[0]), let last = Int(bounds.last!), first >= 1, last >= first else {
                try fail("Slides must be numbers or ranges, for example 1-3, 5, 8.")
            }
            guard last <= count else { try fail("The presentation has \(count) slides; slide \(last) does not exist.") }
            for n in first...last where !out.contains(n) { out.append(n) }
        }
        return out
    }

    /// A free folder name next to existing ones: "Name media", "Name media 2"…
    static func newFolder(in parent: URL, named name: String) -> URL {
        var candidate = parent.appendingPathComponent(name), n = 2
        while fm.fileExists(atPath: candidate.path) { candidate = parent.appendingPathComponent("\(name) \(n)"); n += 1 }
        return candidate
    }

    static func extractMedia(input: URL, destination: URL, kinds: Set<MediaKind>, slides spec: String = "all") throws -> ToolReport {
        guard !kinds.isEmpty else { try fail("Choose at least one kind of media: images, audio or video.") }
        var isFolder: ObjCBool = false
        guard fm.fileExists(atPath: destination.path, isDirectory: &isFolder), isFolder.boolValue else { try fail("Choose an existing folder for the media.") }
        let root = try temp(); defer { try? fm.removeItem(at: root) }
        try unpack(input, root)
        let ordered = try orderedParts(root)
        let chosen = try slideSelection(spec, count: ordered.count)
        let width = String(ordered.count).count
        struct Item { let part: String; let name: String; let kind: MediaKind; var slides: [Int] }
        var items: [Item] = [], byPart: [String: Int] = [:], linked: [String] = [], counters: [String: Int] = [:]
        for number in chosen {
            let slide = ordered[number - 1]
            let relsPath = (slide as NSString).deletingLastPathComponent + "/_rels/" + (slide as NSString).lastPathComponent + ".rels"
            guard fm.fileExists(atPath: root.appendingPathComponent(relsPath).path), let rels = try xml(root, relsPath).rootElement() else { continue }
            // Poster frames: the still picture of a video or sound is not an image of its own.
            var posters = Set<String>()
            if let doc = try? xml(root, slide), let pics = try? doc.nodes(forXPath: "//*[local-name() = 'pic']") {
                for case let pic as XMLElement in pics {
                    guard let nv = list(pic, "nvPicPr").flatMap({ list($0, "nvPr") }), list(nv, "videoFile") != nil || list(nv, "audioFile") != nil,
                          let embed = list(pic, "blipFill").flatMap({ list($0, "blip") })?.attribute(forLocalName: "embed", uri: rNS)?.stringValue else { continue }
                    posters.insert(embed)
                }
            }
            for rel in children(rels, "Relationship") {
                let type = attr(rel, "Type"), target = attr(rel, "Target")
                let ext = (target as NSString).pathExtension.lowercased()
                let kind: MediaKind?
                if type.hasSuffix("/image") { kind = posters.contains(attr(rel, "Id")) ? nil : .images }
                else if type.hasSuffix("/audio") { kind = .audio }
                else if type.hasSuffix("/video") { kind = .video }
                else if type.hasSuffix("/media") { kind = audioExtensions.contains(ext) ? .audio : videoExtensions.contains(ext) ? .video : nil }
                else { kind = nil }
                guard let kind, kinds.contains(kind) else { continue }
                if attr(rel, "TargetMode") == "External" { if !linked.contains(target) { linked.append(target) }; continue }
                let part = resolvePart(owner: slide, target: target)
                if let i = byPart[part] { if !items[i].slides.contains(number) { items[i].slides.append(number) }; continue }
                guard (try? root.appendingPathComponent(part).resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else { continue }
                let key = "\(number)-\(kind.word)"
                counters[key, default: 0] += 1
                let padded = String(repeating: "0", count: max(0, width - String(number).count)) + String(number)
                let safeExt = ext.allSatisfy { $0.isLetter || $0.isNumber } && !ext.isEmpty ? ext : "bin"
                byPart[part] = items.count
                items.append(Item(part: part, name: "slide-\(padded)-\(kind.word)-\(counters[key]!).\(safeExt)", kind: kind, slides: [number]))
            }
        }
        guard !items.isEmpty else {
            try fail("No \(MediaKind.allCases.filter { kinds.contains($0) }.map { $0.title.lowercased() }.joined(separator: ", ")) on \(chosen.count == ordered.count ? "any slide" : "the chosen slides")." + (linked.isEmpty ? "" : " Some media is only linked, outside the presentation: " + linked.joined(separator: ", ")))
        }
        let folder = newFolder(in: destination, named: input.deletingPathExtension().lastPathComponent + " media")
        try fm.createDirectory(at: folder, withIntermediateDirectories: false)
        do {
            for item in items {
                let source = root.appendingPathComponent(item.part), copy = folder.appendingPathComponent(item.name)
                try fm.copyItem(at: source, to: copy)
                guard try checksumFile(copy) == checksumFile(source) else { try fail("\(item.name) was not copied exactly.") }
            }
        } catch {
            try? fm.removeItem(at: folder); throw error
        }
        let count = { (k: MediaKind) in items.filter { $0.kind == k }.count }
        let summary = MediaKind.allCases.filter { kinds.contains($0) }.map { "\(count($0)) \($0 == .images ? "images" : $0 == .audio ? "audio files" : "videos")" }.joined(separator: ", ")
        let scope = chosen.count == ordered.count ? "all \(ordered.count) slides" : "slides " + spec.trimmingCharacters(in: .whitespaces)
        var limitations = ["Only media placed on the slides themselves is extracted; pictures from layouts and masters (logos, template backgrounds) are not.", "The still picture shown for a video or sound (its poster frame) is not counted as an image."]
        let shared = items.filter { $0.slides.count > 1 }
        if !shared.isEmpty { limitations.append("Used on several slides, extracted once: " + shared.map { "\($0.name) (slides \($0.slides.map(String.init).joined(separator: ", ")))" }.joined(separator: "; ") + ".") }
        if !linked.isEmpty { limitations.append("Linked, outside the presentation, so not extracted: " + linked.joined(separator: ", ") + ".") }
        let pages = chosen.map { n in ToolPage(sourcePage: n, fontSize: nil, empty: !items.contains { $0.slides.contains(n) }, warnings: []) }
        let r = ToolReport(operation: "Extract media (\(MediaKind.allCases.filter { kinds.contains($0) }.map { $0.title.lowercased() }.joined(separator: ", ")))", source: input.path, output: folder.path, pages: pages, checks: ["\(summary) from \(scope)", "Files copied byte for byte (SHA-256)", "Saved in a new folder: \(folder.lastPathComponent)"], limitations: limitations)
        do { try emit(r) } catch { try? fm.removeItem(at: folder); throw error }
        return r
    }
}
