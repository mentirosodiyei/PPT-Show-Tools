import Foundation

let pNS = "http://schemas.openxmlformats.org/presentationml/2006/main"
let rNS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
let pkgNS = "http://schemas.openxmlformats.org/package/2006/relationships"
struct MergeFailure: LocalizedError { let message: String; var errorDescription: String? { message } }
func fail(_ s: String) throws -> Never { throw MergeFailure(message: s) }
func run(_ executable: String, _ args: [String], cwd: URL? = nil) throws {
    let task = Process(); task.executableURL = URL(fileURLWithPath: executable); task.arguments = args; task.currentDirectoryURL = cwd
    let log = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    FileManager.default.createFile(atPath: log.path, contents: nil)
    let handle = try FileHandle(forWritingTo: log); task.standardOutput = handle; task.standardError = handle
    defer { try? handle.close(); try? FileManager.default.removeItem(at: log) }
    try task.run(); task.waitUntilExit()
    if task.terminationStatus != 0 { try fail(String(data: try Data(contentsOf: log), encoding: .utf8) ?? "Process failed") }
}
// PowerPoint rejects slide sizes outside 1–56 inches (914400–51206400 EMU).
let minSlideEMU: Int64 = 914400, maxSlideEMU: Int64 = 51206400
func requireValidSlideSize(_ size: (Int64, Int64)) throws {
    guard (minSlideEMU...maxSlideEMU).contains(size.0), (minSlideEMU...maxSlideEMU).contains(size.1) else {
        try fail("The resulting slide size (\(size.0) × \(size.1) EMU) is outside PowerPoint's limits (1–56 in per side).")
    }
}
// PowerPoint measures pixels at 96 per inch: 1 px = 9525 EMU.
let emuPerPixel: Int64 = 9525
func describeSize(_ size: (Int64, Int64)) -> String {
    let px = String(format: "%.0f × %.0f px", Double(size.0) / Double(emuPerPixel), Double(size.1) / Double(emuPerPixel))
    return px + String(format: " (%.2f × %.2f cm)", locale: Locale(identifier: "en_US_POSIX"), Double(size.0) / 360000, Double(size.1) / 360000)
}
/// Output slide size. `fixed` is the size forced in Settings; nil means
/// automatic: a size or proportion shared by every source is kept (at the
/// largest size); mixed proportions use 16:9, at the largest 16:9 source or
/// 1920 × 1080 px when there is none. Every source is then adapted to it.
func planSlideSize(_ sizes: [(Int64, Int64)], fixed: (Int64, Int64)?) throws -> (Int64, Int64) {
    let target: (Int64, Int64)
    if let fixed {
        target = fixed
    } else if let first = sizes.first, sizes.allSatisfy({ SlideScaler.sameAspect($0, first) }) {
        target = sizes.max { $0.0 * $0.1 < $1.0 * $1.1 }!
    } else if let wide = sizes.filter({ SlideScaler.isWidescreen($0) }).max(by: { $0.0 * $0.1 < $1.0 * $1.1 }) {
        target = wide
    } else {
        target = (1920 * emuPerPixel, 1080 * emuPerPixel)
    }
    try requireValidSlideSize(target)
    return target
}
// Packages are untrusted input. Entry names are checked before anything is
// written (no absolute or parent paths, no duplicates, also differing only in
// case, as the disk is case-insensitive); the real number of bytes written is
// watched while ditto extracts, since the sizes a ZIP declares can lie; and
// symbolic links are refused afterwards, as later reads would follow them.
let maxPackageEntries = 100_000
var maxPackageBytes: Int64 {
    // PPTSHOWTOOLS_MAX_PACKAGE_BYTES lowers the limit for the test suite.
    Int64(ProcessInfo.processInfo.environment["PPTSHOWTOOLS_MAX_PACKAGE_BYTES"] ?? "") ?? 8 * 1024 * 1024 * 1024
}
private func capture(_ executable: String, _ args: [String]) throws -> (Int32, Data) {
    let task = Process(), pipe = Pipe()
    task.executableURL = URL(fileURLWithPath: executable); task.arguments = args
    task.standardOutput = pipe; task.standardError = FileHandle.nullDevice
    try task.run()
    let data = pipe.fileHandleForReading.readDataToEndOfFile()
    task.waitUntilExit()
    return (task.terminationStatus, data)
}
private func bytesOnDisk(_ dir: URL) -> Int64 {
    var total: Int64 = 0
    if let items = FileManager.default.enumerator(at: dir, includingPropertiesForKeys: [.fileSizeKey]) {
        for case let url as URL in items { total += Int64((try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) }
    }
    return total
}
func extractPackage(_ input: URL, to dir: URL) throws {
    let name = input.lastPathComponent
    let tooLarge = "“\(name)” is too large or has too many entries to process safely."
    let (status, listing) = try capture("/usr/bin/zipinfo", ["-1", input.path])
    guard status == 0 else { try fail("“\(name)” is not a valid PowerPoint ZIP package.") }
    let entries = String(decoding: listing, as: UTF8.self).split(separator: "\n", omittingEmptySubsequences: true).map(String.init)
    guard !entries.isEmpty else { try fail("“\(name)” is not a valid PowerPoint ZIP package.") }
    guard entries.count <= maxPackageEntries else { try fail(tooLarge) }
    var seen = Set<String>(), folders = Set<String>()
    for entry in entries {
        let isFolder = entry.hasSuffix("/")
        let segments = (isFolder ? String(entry.dropLast()) : entry).split(separator: "/", omittingEmptySubsequences: false)
        guard !entry.hasPrefix("/"), !entry.contains("\\"), !segments.isEmpty, !segments.contains(where: { $0.isEmpty || $0 == ".." || $0 == "." }) else { try fail("“\(name)”: unsupported ZIP entry path: \(entry)") }
        // Every folder on the way to an entry, so a file cannot also be used as a folder
        // (the shape of an archive that writes through a symbolic link).
        for depth in 1..<max(segments.count, 1) { folders.insert(segments.prefix(depth).joined(separator: "/").lowercased()) }
        if isFolder { folders.insert(segments.joined(separator: "/").lowercased()); continue }
        guard seen.insert(entry.lowercased()).inserted else { try fail("“\(name)”: duplicate ZIP entry: \(entry)") }
    }
    if let clash = seen.first(where: { folders.contains($0) }) { try fail("“\(name)”: unsupported ZIP entry path: \(clash)") }
    let (totalStatus, totals) = try capture("/usr/bin/zipinfo", ["-t", input.path])
    let numbers = String(decoding: totals, as: UTF8.self).split(whereSeparator: { !$0.isNumber }).compactMap { Int64($0) }
    guard totalStatus == 0, numbers.count >= 2 else { try fail("“\(name)” is not a valid PowerPoint ZIP package.") }
    guard numbers[1] <= maxPackageBytes else { try fail(tooLarge) }
    let task = Process()
    task.executableURL = URL(fileURLWithPath: "/usr/bin/ditto"); task.arguments = ["-x", "-k", input.path, dir.path]
    task.standardOutput = FileHandle.nullDevice; task.standardError = FileHandle.nullDevice
    try task.run()
    // Wake often so small packages finish without delay; measure the disk every 0.25 s.
    var exceeded = false, ticks = 0
    while task.isRunning {
        Thread.sleep(forTimeInterval: 0.005); ticks += 1
        if ticks % 50 == 0, bytesOnDisk(dir) > maxPackageBytes { exceeded = true; task.terminate(); break }
    }
    task.waitUntilExit()
    if exceeded || bytesOnDisk(dir) > maxPackageBytes { try? FileManager.default.removeItem(at: dir); try fail(tooLarge) }
    guard task.terminationStatus == 0 else { try fail("“\(name)” is not a valid PowerPoint ZIP package.") }
    let keys: [URLResourceKey] = [.isSymbolicLinkKey, .isRegularFileKey, .isDirectoryKey]
    guard let items = FileManager.default.enumerator(at: dir, includingPropertiesForKeys: keys) else { try fail("Cannot read the extracted package.") }
    for case let url as URL in items {
        let values = try url.resourceValues(forKeys: Set(keys))
        if values.isSymbolicLink == true || (values.isRegularFile != true && values.isDirectory != true) {
            try fail("“\(name)” contains symbolic links or special entries, which a PowerPoint file cannot have: \(url.lastPathComponent)")
        }
    }
}
/// Real parts nest a few dozen levels; a hostile part could exhaust the stack.
let maxXMLDepth = 256
/// Parts never contain a DTD; refusing one rules out entity expansion, and
/// external entities are never loaded. Only UTF-8 is accepted (PowerPoint
/// writes nothing else), so a DTD cannot hide behind another encoding.
func parseXML(at url: URL) throws -> XMLDocument {
    let data = try Data(contentsOf: url), name = url.lastPathComponent
    guard var text = String(data: data, encoding: .utf8) else { try fail("\(name): XML with an unsupported encoding (UTF-8 required).") }
    if text.hasPrefix("\u{FEFF}") { text.removeFirst() }
    if text.hasPrefix("<?xml"), let end = text.range(of: "?>") {
        let declaration = text[..<end.lowerBound]
        if let match = declaration.range(of: #"encoding\s*=\s*["'][^"']*["']"#, options: .regularExpression),
           declaration[match].range(of: "utf-8", options: .caseInsensitive) == nil {
            try fail("\(name): XML with an unsupported encoding (UTF-8 required).")
        }
    }
    if text.range(of: "<!DOCTYPE", options: .caseInsensitive) != nil {
        try fail("\(name) contains a document type declaration (DTD), which PowerPoint files never use.")
    }
    if xmlNestingExceeds(data, limit: maxXMLDepth) { try fail("\(name): XML nested too deeply.") }
    return try XMLDocument(data: data, options: [.nodePreserveAll, .nodeLoadExternalEntitiesNever])
}
/// Element depth measured on the bytes, before any recursive parser sees them.
func xmlNestingExceeds(_ data: Data, limit: Int) -> Bool {
    data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> Bool in
        let b = raw.bindMemory(to: UInt8.self), n = b.count
        func find(_ token: [UInt8], from start: Int) -> Int {
            var i = start
            while i + token.count <= n {
                if b[i] == token[0] && (0..<token.count).allSatisfy({ b[i + $0] == token[$0] }) { return i + token.count }
                i += 1
            }
            return n
        }
        func starts(_ token: [UInt8], at i: Int) -> Bool {
            i + token.count <= n && (0..<token.count).allSatisfy { b[i + $0] == token[$0] }
        }
        var i = 0, depth = 0
        while i < n {
            guard b[i] == UInt8(ascii: "<"), i + 1 < n else { i += 1; continue }
            let next = b[i + 1]
            if next == UInt8(ascii: "!") {
                if starts(Array("<!--".utf8), at: i) { i = find(Array("-->".utf8), from: i + 4) }
                else if starts(Array("<![CDATA[".utf8), at: i) { i = find(Array("]]>".utf8), from: i + 9) }
                else { i = find([UInt8(ascii: ">")], from: i) }
            } else if next == UInt8(ascii: "?") {
                i = find(Array("?>".utf8), from: i)
            } else if next == UInt8(ascii: "/") {
                depth -= 1; i = find([UInt8(ascii: ">")], from: i)
            } else {
                var j = i + 1, quote: UInt8 = 0
                while j < n {
                    let c = b[j]
                    if quote != 0 { if c == quote { quote = 0 } }
                    else if c == UInt8(ascii: "\"") || c == UInt8(ascii: "'") { quote = c }
                    else if c == UInt8(ascii: ">") { break }
                    j += 1
                }
                if j < n, b[j - 1] != UInt8(ascii: "/") { depth += 1; if depth > limit { return true } }
                i = j + 1
            }
        }
        return false
    }
}
func xml(_ root: URL, _ path: String) throws -> XMLDocument { try parseXML(at: root.appendingPathComponent(path)) }
func children(_ e: XMLElement, _ local: String) -> [XMLElement] { (e.children ?? []).compactMap { $0 as? XMLElement }.filter { $0.localName == local } }
func attr(_ e: XMLElement, _ name: String) -> String { e.attribute(forName: name)?.stringValue ?? "" }
func set(_ e: XMLElement, _ name: String, _ value: String) { if let a = e.attribute(forName: name) { a.stringValue = value } else { e.addAttribute(XMLNode.attribute(withName: name, stringValue: value) as! XMLNode) } }
// Foundation emits literal whitespace inside attributes. XML parsers normalize
// it on reopening, losing line breaks in alt text and other attribute values.
func serializedXML(_ doc: XMLDocument) -> Data {
    let raw = String(decoding: doc.xmlData, as: UTF8.self)
    var result = "", inTag = false, quote: Unicode.Scalar? = nil
    // Comments, CDATA and processing instructions are copied verbatim: a quote
    // inside them must not be taken for the start of an attribute value.
    var verbatimEnd: [Unicode.Scalar]? = nil, tail: [Unicode.Scalar] = []
    let scalars = Array(raw.unicodeScalars)
    var index = 0
    func starts(_ token: String, at i: Int) -> Bool {
        let t = Array(token.unicodeScalars)
        return i + t.count <= scalars.count && Array(scalars[i..<i + t.count]) == t
    }
    while index < scalars.count {
        let c = scalars[index]; index += 1
        if let end = verbatimEnd {
            result.unicodeScalars.append(c); tail.append(c)
            if tail.count > end.count { tail.removeFirst() }
            if tail == end { verbatimEnd = nil; tail = [] }
            continue
        }
        if quote == nil && c == "<" {
            for (open, close) in [("!--", "-->"), ("![CDATA[", "]]>"), ("?", "?>")] where starts(open, at: index) {
                verbatimEnd = Array(close.unicodeScalars)
            }
            if verbatimEnd != nil { result.unicodeScalars.append(c); continue }
        }
        if let delimiter = quote {
            if c == delimiter { quote = nil; result.unicodeScalars.append(c) }
            else if c == "\n" { result += "&#10;" }
            else if c == "\r" { result += "&#13;" }
            else if c == "\t" { result += "&#9;" }
            else { result.unicodeScalars.append(c) }
        } else {
            if c == "<" { inTag = true }
            if inTag && (c == "\"" || c == "'") { quote = c }
            if c == ">" { inTag = false }
            result.unicodeScalars.append(c)
        }
    }
    return Data(result.utf8)
}
func write(_ doc: XMLDocument, _ root: URL, _ path: String) throws { try serializedXML(doc).write(to: root.appendingPathComponent(path), options: .atomic) }
func list(_ root: XMLElement, _ local: String) -> XMLElement? { children(root, local).first }
func rid(_ e: XMLElement) -> String { e.attribute(forLocalName: "id", uri: rNS)?.stringValue ?? "" }
func setRID(_ e: XMLElement, _ value: String) { if let a = e.attribute(forLocalName: "id", uri: rNS) { a.stringValue = value } else { e.addAttribute(XMLNode.attribute(withName: "r:id", uri: rNS, stringValue: value) as! XMLNode) } }
struct MergeReport: Codable {
    let engine: String; let inputFiles: [String]; let slideCounts: [Int]; let totalSlides: Int
    let warnings: [String]; let powerPointPlaybackValidated: Bool
    let validation: [FileValidation]
    let overallStatus: String
    let validationScope: String
    var structuralValidationPassed: Bool { validation.allSatisfy { $0.issues.isEmpty } }
}

// Sequence order of children in CT_Presentation (ISO/IEC 29500-1 §19.2.1.26)
private let presentationChildOrder: [String] = [
    "sldMasterIdLst",
    "notesMasterIdLst",
    "handoutMasterIdLst",
    "sldIdLst",
    "sldSz",
    "notesSz",
    "smartTags",
    "embeddedFontLst",
    "custShowLst",
    "photoAlbum",
    "custDataLst",
    "kinsoku",
    "defaultTextStyle",
    "modifyVerifier",
    "extLst"
]

func insertInPresentationOrder(_ root: XMLElement, _ node: XMLElement) {
    let targetLocal = node.localName ?? (node.name ?? "").components(separatedBy: ":").last ?? ""
    guard let targetRank = presentationChildOrder.firstIndex(of: targetLocal) else {
        root.addChild(node)
        return
    }
    let childrenList = root.children ?? []
    for (idx, child) in childrenList.enumerated() {
        if let el = child as? XMLElement {
            let childLocal = el.localName ?? (el.name ?? "").components(separatedBy: ":").last ?? ""
            if let childRank = presentationChildOrder.firstIndex(of: childLocal), childRank > targetRank {
                root.insertChild(node, at: idx)
                return
            }
        }
    }
    root.addChild(node)
}

/// Parts of an imported presentation that nothing in the result uses: its document
/// properties (authors, company, thumbnail) and its VBA project, which is never
/// merged. They are removed instead of being carried along unused.
let unusedImportedParts = ["docProps", "ppt/vbaProject.bin", "ppt/vbaData.xml", "ppt/_rels/vbaProject.bin.rels"]

/// External targets PowerPoint loads by itself (linked pictures, media, OLE
/// links); hyperlinks only open when clicked and are left out.
func automaticExternalTargets(_ root: URL) -> [String] {
    var found: [String] = []
    guard let items = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil) else { return [] }
    for case let url as URL in items where url.lastPathComponent.hasSuffix(".rels") {
        guard let doc = try? parseXML(at: url), let rootElement = doc.rootElement() else { continue }
        for e in children(rootElement, "Relationship") where attr(e, "TargetMode") == "External" && !attr(e, "Type").hasSuffix("/hyperlink") {
            found.append(attr(e, "Target"))
        }
    }
    return Array(Set(found)).sorted()
}
func externalLinkWarning(_ file: String, _ targets: [String]) -> String? {
    guard !targets.isEmpty else { return nil }
    let sample = targets.prefix(3).map { $0.count > 80 ? String($0.prefix(80)) + "…" : $0 }.joined(separator: ", ")
    return "\(file): \(targets.count) linked file(s) or address(es) outside the presentation (\(sample)). PowerPoint may load them when the result is opened; check them if you do not trust the original."
}

struct OpenXMLMerger {
    /// writeReports false: the caller (PowerPoint save) writes its own report.
    func merge(inputs: [URL], output: URL, fixedSize: (Int64, Int64)? = nil, writeReports: Bool = true) throws -> MergeReport {
        let fm = FileManager.default
        guard inputs.count >= 2 else { try fail("Add at least two presentations.") }
        guard ["pptx", "pptm"].contains(output.pathExtension.lowercased()) else { try fail("The output must be PPTX or PPTM.") }
        guard !fm.fileExists(atPath: output.path), !(writeReports && ReportStore.collides(output, [".report.json", ".report.txt"])) else { try fail("The output file or its report already exists. Choose another name.") }
        for input in inputs {
            guard ["pptx", "pptm"].contains(input.pathExtension.lowercased()), input.standardizedFileURL != output.standardizedFileURL else { try fail("Invalid input.") }
        }
        let temp = fm.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("PPTShowTools-" + UUID().uuidString)
        try fm.createDirectory(at: temp, withIntermediateDirectories: true); defer { try? fm.removeItem(at: temp) }
        var roots: [URL] = []
        for (i, input) in inputs.enumerated() {
            let dir = temp.appendingPathComponent("input\(i)"); try fm.createDirectory(at: dir, withIntermediateDirectories: true)
            // ditto handles native macOS ZIP packages; originals are only read.
            try extractPackage(input, to: dir); roots.append(dir)
        }
        for dir in roots { let trash = dir.appendingPathComponent("[trash]"); if fm.fileExists(atPath: trash.path) { try fm.removeItem(at: trash) } }
        let docs = try roots.map { try xml($0, "ppt/presentation.xml") }
        guard docs.allSatisfy({ $0.rootElement()?.uri == pNS }) else { try fail("Only PowerPoint Open XML (Transitional) is supported.") }
        let sizes = try docs.enumerated().map { i, d -> (Int64, Int64) in
            guard let size = list(d.rootElement()!, "sldSz"),
                  let width = Int64(attr(size, "cx")), let height = Int64(attr(size, "cy")),
                  width > 0, height > 0 else {
                try fail("Cannot read the slide size of file \(i + 1): “\(inputs[i].lastPathComponent)”.")
            }
            guard (minSlideEMU...maxSlideEMU).contains(width), (minSlideEMU...maxSlideEMU).contains(height) else {
                try fail("“\(inputs[i].lastPathComponent)” has a slide size outside PowerPoint's limits (1–56 in per side).")
            }
            return (width, height)
        }
        let originals = try roots.map { try PackageSnapshot.capture($0) }
        let target = try planSlideSize(sizes, fixed: fixedSize)
        var scalingWarnings: [String] = []
        for i in sizes.indices where sizes[i] != target {
            // Each presentation is adapted on its own before the merge.
            try SlideScaler.normalize(package: roots[i], presentation: docs[i], from: sizes[i], to: target)
            let sameShape = SlideScaler.sameAspect(sizes[i], target)
            scalingWarnings.append("\(inputs[i].lastPathComponent): \(describeSize(sizes[i])) → \(describeSize(target)); \(sameShape ? "scaled proportionally" : "adapted to the new proportion: nothing is stretched; text boxes and full-width elements follow the new shape, pictures filling the slide are shown whole, with margins in their own colours, other objects keep their place").")
        }
        let counts = docs.map { children(list($0.rootElement()!, "sldIdLst")!, "sldId").count }
        guard counts.allSatisfy({ $0 > 0 }) else { try fail("One of the presentations has no slides.") }
        let base = roots[0], pres = docs[0], root = pres.rootElement()!
        let rels = try xml(base, "ppt/_rels/presentation.xml.rels"), ct = try xml(base, "[Content_Types].xml")
        var warnings = [
            "Direct Open XML engine (PowerPoint is not opened).",
            "Check playback, editing, notes, fonts, external links and OLE objects in PowerPoint."
        ]
        warnings.append(contentsOf: scalingWarnings)
        for (i, dir) in roots.enumerated() {
            if let warning = externalLinkWarning(inputs[i].lastPathComponent, automaticExternalTargets(dir)) { warnings.append(warning) }
        }
        let baseTypes = ct.rootElement()!
        // ST_SlideId allows 256…2147483647; reject anything that would overflow.
        let highestSlide = children(list(root, "sldIdLst")!, "sldId").compactMap { Int64(attr($0, "id")) }.max() ?? 255
        guard highestSlide < 2147483647 else { try fail("Too many slide identifiers to create a valid presentation.") }
        var nextSlide = UInt32(highestSlide + 1)
        var nextMaster: UInt64 = 2147483648
        for d in docs {
            if let masters = list(d.rootElement()!, "sldMasterIdLst") {
                for m in children(masters, "sldMasterId") {
                    if let id = UInt64(attr(m, "id")), id < UInt64(UInt32.max) { nextMaster = max(nextMaster, id + 1) }
                }
            }
        }
        // Master and layout IDs share a presentation-wide identifier space.
        for dir in roots {
            if let en = fm.enumerator(at: dir.appendingPathComponent("ppt/slideMasters"), includingPropertiesForKeys: nil) {
                for case let url as URL in en where url.pathExtension == "xml" {
                    let d = try parseXML(at: url)
                    for node in try d.nodes(forXPath: "//*[local-name() = 'sldLayoutId']") {
                        if let e = node as? XMLElement, let id = UInt64(attr(e, "id")), id < UInt64(UInt32.max) { nextMaster = max(nextMaster, id + 1) }
                    }
                }
            }
        }
        var relNumber = 0
        func addRel(_ type: String, _ target: String) -> String {
            var id: String
            repeat { relNumber += 1; id = "pptMerger\(relNumber)" } while children(rels.rootElement()!, "Relationship").contains(where: { attr($0, "Id") == id })
            let e = XMLElement(name: "Relationship", uri: pkgNS); set(e, "Id", id); set(e, "Type", type); set(e, "Target", target); rels.rootElement()!.addChild(e); return id
        }
        var notesTarget = children(rels.rootElement()!, "Relationship").first(where: { attr($0, "Type").hasSuffix("/notesMaster") }).map { e -> String in
            let t = attr(e, "Target"); return t.hasPrefix("/") ? t : "/ppt/" + t
        }
        warnings.append("All notes use the first notes master available. Each slide's notes content is kept; inherited notes formatting may change.")
        // Relationship targets are rooted in their original source package. Each imported
        // package is namespaced intact; relative references therefore remain unchanged.
        for i in 1..<roots.count {
            let source = roots[i], prefix = "pptMerger/source\(i)/"
            let hadVBA = fm.fileExists(atPath: source.appendingPathComponent("ppt/vbaProject.bin").path)
            for part in unusedImportedParts where fm.fileExists(atPath: source.appendingPathComponent(part).path) {
                try fm.removeItem(at: source.appendingPathComponent(part))
            }
            if notesTarget == nil {
                let sr = try xml(source, "ppt/_rels/presentation.xml.rels")
                if let e = children(sr.rootElement()!, "Relationship").first(where: { attr($0, "Type").hasSuffix("/notesMaster") }) {
                    let old = attr(e, "Target")
                    let part = old.hasPrefix("/") ? String(old.dropFirst()) : "ppt/" + old
                    notesTarget = "/" + prefix + part
                    let newID = addRel(attr(e, "Type"), notesTarget!)
                    let lst = XMLElement(name: "p:notesMasterIdLst", uri: pNS), item = XMLElement(name: "p:notesMasterId", uri: pNS)
                    setRID(item, newID); lst.addChild(item)
                    insertInPresentationOrder(root, lst)
                }
            }
            let dest = base.appendingPathComponent(prefix)
            try fm.createDirectory(at: dest.deletingLastPathComponent(), withIntermediateDirectories: true)
            try fm.copyItem(at: source, to: dest)
            guard let en = fm.enumerator(at: dest, includingPropertiesForKeys: [.isRegularFileKey]) else { try fail("Cannot read the package.") }
            for case let url as URL in en where url.lastPathComponent.hasSuffix(".rels") {
                let d = try parseXML(at: url)
                for e in children(d.rootElement()!, "Relationship") where attr(e, "TargetMode") != "External" {
                    if attr(e, "Type").hasSuffix("/notesMaster"), let notesTarget {
                        set(e, "Target", notesTarget)
                    } else if attr(e, "Target").hasPrefix("/") {
                        set(e, "Target", "/" + prefix + String(attr(e, "Target").dropFirst()))
                    }
                }
                try serializedXML(d).write(to: url, options: .atomic)
            }
            if let en = fm.enumerator(at: dest.appendingPathComponent("ppt/slideMasters"), includingPropertiesForKeys: nil) {
                for case let url as URL in en where url.pathExtension == "xml" {
                    let d = try parseXML(at: url)
                    for node in try d.nodes(forXPath: "//*[local-name() = 'sldLayoutId']") {
                        if let e = node as? XMLElement { set(e, "id", String(nextMaster)); nextMaster += 1 }
                    }
                    try serializedXML(d).write(to: url, options: .atomic)
                }
            }
            let srcCT = try xml(source, "[Content_Types].xml")
            // OPC default extensions are case-insensitive; keep actual ZIP part
            // names unchanged so slide relationships still resolve.
            var defaults: [String: String] = [:]
            for entry in children(srcCT.rootElement()!, "Default") {
                let ext = attr(entry, "Extension").lowercased(), type = attr(entry, "ContentType")
                if let existing = defaults[ext], existing != type {
                    try fail("Conflicting content types for extension .\(ext) in “\(inputs[i].lastPathComponent)”.")
                }
                defaults[ext] = type
            }
            // Explicit types prevent conflicting default extensions across packages.
            let overrides = Dictionary(children(srcCT.rootElement()!, "Override").map { (String(attr($0, "PartName").drop(while: { $0 == "/" })), attr($0, "ContentType")) }, uniquingKeysWith: { _, last in last })
            let files = fm.enumerator(at: source, includingPropertiesForKeys: [.isRegularFileKey])!
            for case let url as URL in files {
                guard (try url.resourceValues(forKeys: [.isRegularFileKey])).isRegularFile == true else { continue }
                let part = String(url.resolvingSymlinksInPath().path.dropFirst(source.resolvingSymlinksInPath().path.count + 1))
                guard part != "[Content_Types].xml", !url.lastPathComponent.hasSuffix(".rels") else { continue }
                guard let type = overrides[part] ?? defaults[url.pathExtension.lowercased()] else { try fail("Missing content type: \(part)") }
                let e = XMLElement(name: "Override", uri: baseTypes.uri); set(e, "PartName", "/" + prefix + part); set(e, "ContentType", type); baseTypes.addChild(e)
            }
            // The imported package's root metadata is not a second OPC root.
            try fm.removeItem(at: dest.appendingPathComponent("[Content_Types].xml"))
            let srcRels = try xml(source, "ppt/_rels/presentation.xml.rels")
            let byID = Dictionary(children(srcRels.rootElement()!, "Relationship").map { (attr($0, "Id"), $0) }, uniquingKeysWith: { _, last in last })
            func importList(_ name: String, _ child: String) throws {
                guard let sourceList = list(docs[i].rootElement()!, name) else { return }
                var targetList = list(root, name)
                if targetList == nil {
                    let node = XMLElement(name: "p:" + name, uri: pNS)
                    insertInPresentationOrder(root, node)
                    targetList = node
                }
                for e in children(sourceList, child) {
                    guard let rel = byID[rid(e)] else { try fail("Missing presentation relationship") }
                    let old = attr(rel, "Target")
                    let part = old.hasPrefix("/") ? String(old.dropFirst()) : URL(fileURLWithPath: "/ppt/").appendingPathComponent(old).standardized.path.dropFirst().description
                    let newRID = addRel(attr(rel, "Type"), "/" + prefix + part)
                    let node = e.copy() as! XMLElement; setRID(node, newRID)
                    if child == "sldId" { set(node, "id", String(nextSlide)); nextSlide += 1 }
                    else { set(node, "id", String(nextMaster)); nextMaster += 1 }
                    targetList!.addChild(node)
                }
            }
            try importList("sldMasterIdLst", "sldMasterId")
            try importList("sldIdLst", "sldId")
            // Embedded fonts belong to the presentation, outside the slide graph.
            // Copying their bytes alone does not make them available to PowerPoint.
            if let sourceFonts = list(docs[i].rootElement()!, "embeddedFontLst") {
                var targetFonts = list(root, "embeddedFontLst")
                if targetFonts == nil {
                    let node = XMLElement(name: "p:embeddedFontLst", uri: pNS)
                    insertInPresentationOrder(root, node)
                    targetFonts = node
                }
                for font in children(sourceFonts, "embeddedFont") {
                    let node = font.copy() as! XMLElement
                    for style in (node.children ?? []).compactMap({ $0 as? XMLElement }) where !rid(style).isEmpty {
                        guard let rel = byID[rid(style)], attr(rel, "Type").hasSuffix("/font"), attr(rel, "TargetMode") != "External" else {
                            try fail("Missing or invalid embedded font relationship in “\(inputs[i].lastPathComponent)”.")
                        }
                        let old = attr(rel, "Target")
                        let part = old.hasPrefix("/") ? String(old.dropFirst()) : URL(fileURLWithPath: "/ppt/").appendingPathComponent(old).standardized.path.dropFirst().description
                        setRID(style, addRel(attr(rel, "Type"), "/" + prefix + part))
                    }
                    targetFonts!.addChild(node)
                }
                warnings.append("\(inputs[i].lastPathComponent): embedded fonts registered in the result. Check substitutions, usage rights and different variants with the same name.")
            }
            if hadVBA { warnings.append("\(inputs[i].lastPathComponent): its VBA project (macros) was removed; only the first file's macros are kept.") }
        }
        let hasMacros = fm.fileExists(atPath: base.appendingPathComponent("ppt/vbaProject.bin").path)
        if output.pathExtension.lowercased() == "pptx" && hasMacros { try fail("The first file contains VBA: save as PPTM to keep it.") }
        for e in children(baseTypes, "Override") where attr(e, "PartName") == "/ppt/presentation.xml" {
            set(e, "ContentType", output.pathExtension.lowercased() == "pptm" ? "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml" : "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml")
        }
        // Only one presentation root is valid for the exported document. Imported
        // presentation metadata is not used by slides; omit those orphan roots.
        for i in 1..<roots.count {
            let prefix = "pptMerger/source\(i)/"
            for part in ["ppt/presentation.xml", "ppt/_rels/presentation.xml.rels", "_rels/.rels"] {
                let path = base.appendingPathComponent(prefix + part)
                if fm.fileExists(atPath: path.path) { try fm.removeItem(at: path) }
                for e in children(baseTypes, "Override") where attr(e, "PartName") == "/" + prefix + part { e.detach() }
            }
        }
        // ST_SlideId allows 256…2147483647; ST_SlideMasterId starts at 2147483648.
        guard nextSlide <= 2147483648, nextMaster <= 4294967296 else { try fail("Too many slide identifiers to create a valid presentation.") }
        try write(pres, base, "ppt/presentation.xml"); try write(rels, base, "ppt/_rels/presentation.xml.rels"); try write(ct, base, "[Content_Types].xml")
        let archive = temp.appendingPathComponent("result.zip")
        try run("/usr/bin/ditto", ["-c", "-k", "--norsrc", "--noextattr", base.path, archive.path])
        let verificationRoot = temp.appendingPathComponent("verify")
        try fm.createDirectory(at: verificationRoot, withIntermediateDirectories: true)
        try run("/usr/bin/ditto", ["-x", "-k", archive.path, verificationRoot.path])
        let validation = try PackageVerifier.verify(sources: originals, inputs: inputs, merged: verificationRoot)
        let report = MergeReport(engine: "PPTShowTools-OpenXML-v0.9.0", inputFiles: inputs.map { $0.path }, slideCounts: counts, totalSlides: counts.reduce(0, +), warnings: warnings, powerPointPlaybackValidated: false, validation: validation, overallStatus: validation.allSatisfy { $0.issues.isEmpty } ? "structural_checks_passed_needs_review" : "import_failed_validation", validationScope: "Structural comparisons against each original; visual rendering, playback and editing not validated.")
        try fm.copyItem(at: archive, to: output)
        guard writeReports else { return report }
        do {
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try ReportStore.write(try encoder.encode(report), for: output, ".report.json")
            var lines = ["PPT SHOW TOOLS — CHECKS PER FILE", report.structuralValidationPassed ? "Structural checks passed. Visual and playback review pending." : "IMPORT NOT CORRECT: differences found.", "", "Output size: \(describeSize(target))", ""]
            for (i, check) in validation.enumerated() {
                lines += ["\(i + 1). \(check.file)", check.issues.isEmpty ? "STRUCTURE: checks passed; this is not full fidelity." : "IMPORT NOT CORRECT", "\(check.checkedSlides) slides; \(check.slidesWithAnimation) with animation; \(check.binaryResourcesChecked) resources checked."]
                lines += check.checks.map { "Checked: " + $0 }
                lines += check.issues.map { "ERROR: " + $0 }
                lines += check.needsReview.map { "TO REVIEW: " + $0 }
                lines.append("")
            }
            lines += ["WARNINGS"] + warnings
            try ReportStore.write(Data(lines.joined(separator: "\n").utf8), for: output, ".report.txt")
        } catch {
            try? fm.removeItem(at: output)
            ReportStore.remove(for: output, [".report.json", ".report.txt"])
            throw error
        }
        return report
    }
}
