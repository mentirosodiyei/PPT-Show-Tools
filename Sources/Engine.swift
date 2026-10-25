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
    if task.terminationStatus != 0 { try fail(String(data: try Data(contentsOf: log), encoding: .utf8) ?? "Error del procés") }
}
// PowerPoint rejects slide sizes outside 1–56 inches (914400–51206400 EMU).
let minSlideEMU: Int64 = 914400, maxSlideEMU: Int64 = 51206400
func requireValidSlideSize(_ size: (Int64, Int64)) throws {
    guard (minSlideEMU...maxSlideEMU).contains(size.0), (minSlideEMU...maxSlideEMU).contains(size.1) else {
        try fail("La mida de diapositiva resultant (\(size.0) × \(size.1) EMU) supera els límits de PowerPoint (2,54–142,24 cm per costat).")
    }
}
// Packages are untrusted input. ditto drops "../" components, but keeps symbolic
// links, which later reads would follow outside the package. Reject them, and
// refuse archives that would expand beyond a sane size before writing anything.
let maxPackageBytes: Int64 = 8 * 1024 * 1024 * 1024, maxPackageEntries = 100_000
func extractPackage(_ input: URL, to dir: URL) throws {
    let task = Process(), pipe = Pipe()
    task.executableURL = URL(fileURLWithPath: "/usr/bin/zipinfo"); task.arguments = ["-t", input.path]
    task.standardOutput = pipe; task.standardError = FileHandle.nullDevice
    try task.run()
    let summary = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    task.waitUntilExit()
    let numbers = summary.split(whereSeparator: { !$0.isNumber }).compactMap { Int64($0) }
    guard task.terminationStatus == 0, numbers.count >= 2 else { try fail("«\(input.lastPathComponent)» no és un paquet ZIP vàlid de PowerPoint.") }
    guard numbers[0] <= maxPackageEntries, numbers[1] <= maxPackageBytes else { try fail("«\(input.lastPathComponent)» és massa gran o té massa entrades per processar-lo amb seguretat.") }
    try run("/usr/bin/ditto", ["-x", "-k", input.path, dir.path])
    let keys: [URLResourceKey] = [.isSymbolicLinkKey, .isRegularFileKey, .isDirectoryKey]
    guard let items = FileManager.default.enumerator(at: dir, includingPropertiesForKeys: keys) else { try fail("No es pot llegir el paquet extret.") }
    for case let url as URL in items {
        let values = try url.resourceValues(forKeys: Set(keys))
        if values.isSymbolicLink == true || (values.isRegularFile != true && values.isDirectory != true) {
            try fail("«\(input.lastPathComponent)» conté enllaços simbòlics o entrades especials, no admesos en un PowerPoint: \(url.lastPathComponent)")
        }
    }
}
func xml(_ root: URL, _ path: String) throws -> XMLDocument { try XMLDocument(contentsOf: root.appendingPathComponent(path), options: [.nodePreserveAll]) }
func children(_ e: XMLElement, _ local: String) -> [XMLElement] { (e.children ?? []).compactMap { $0 as? XMLElement }.filter { $0.localName == local } }
func attr(_ e: XMLElement, _ name: String) -> String { e.attribute(forName: name)?.stringValue ?? "" }
func set(_ e: XMLElement, _ name: String, _ value: String) { if let a = e.attribute(forName: name) { a.stringValue = value } else { e.addAttribute(XMLNode.attribute(withName: name, stringValue: value) as! XMLNode) } }
// Foundation emits literal whitespace inside attributes. XML parsers normalize
// it on reopening, losing line breaks in alt text and other attribute values.
func serializedXML(_ doc: XMLDocument) -> Data {
    let raw = String(decoding: doc.xmlData, as: UTF8.self)
    var result = "", inTag = false, quote: Unicode.Scalar? = nil
    for c in raw.unicodeScalars {
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
struct FormatChoiceRequired: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
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

struct OpenXMLMerger {
    func merge(inputs: [URL], output: URL, fitNonWidescreen: Bool = false) throws -> MergeReport {
        let fm = FileManager.default
        guard inputs.count >= 2 else { try fail("Afegeix dues presentacions com a mínim.") }
        guard ["pptx", "pptm"].contains(output.pathExtension.lowercased()) else { try fail("La sortida ha de ser PPTX o PPTM.") }
        guard !fm.fileExists(atPath: output.path), !fm.fileExists(atPath: output.path + ".report.json"), !fm.fileExists(atPath: output.path + ".report.txt") else { try fail("El fitxer de sortida o l’informe ja existeix. Tria un altre nom.") }
        for input in inputs {
            guard ["pptx", "pptm"].contains(input.pathExtension.lowercased()), input.standardizedFileURL != output.standardizedFileURL else { try fail("Entrada no vàlida.") }
        }
        let temp = fm.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("PPTMerger-" + UUID().uuidString)
        try fm.createDirectory(at: temp, withIntermediateDirectories: true); defer { try? fm.removeItem(at: temp) }
        var roots: [URL] = []
        for (i, input) in inputs.enumerated() {
            let dir = temp.appendingPathComponent("input\(i)"); try fm.createDirectory(at: dir, withIntermediateDirectories: true)
            // ditto handles native macOS ZIP packages; originals are only read.
            try extractPackage(input, to: dir); roots.append(dir)
        }
        for dir in roots { let trash = dir.appendingPathComponent("[trash]"); if fm.fileExists(atPath: trash.path) { try fm.removeItem(at: trash) } }
        let docs = try roots.map { try xml($0, "ppt/presentation.xml") }
        guard docs.allSatisfy({ $0.rootElement()?.uri == pNS }) else { try fail("Només es permet Open XML Transitional de PowerPoint.") }
        let sizes = try docs.enumerated().map { i, d -> (Int64, Int64) in
            guard let size = list(d.rootElement()!, "sldSz"),
                  let width = Int64(attr(size, "cx")), let height = Int64(attr(size, "cy")),
                  width > 0, height > 0 else {
                try fail("No es poden llegir les dimensions del fitxer \(i + 1): «\(inputs[i].lastPathComponent)».")
            }
            return (width, height)
        }
        func dimensions(_ size: (Int64, Int64)) -> String {
            // Open XML stores sizes in EMU: 360,000 EMU per centimetre.
            String(format: "%.2f × %.2f cm", locale: Locale(identifier: "ca_ES"), Double(size.0) / 360000, Double(size.1) / 360000)
        }
        let originals = try roots.map { try PackageSnapshot.capture($0) }
        let invalid = sizes.indices.filter { !SlideScaler.isWidescreen(sizes[$0]) }
        if !invalid.isEmpty && !fitNonWidescreen {
            let details = invalid.map { i in "• Fitxer \(i + 1): «\(inputs[i].lastPathComponent)» — \(dimensions(sizes[i]))." }.joined(separator: "\n")
            throw FormatChoiceRequired(message: "Aquests fitxers no tenen proporció 16:9:\n\n" + details + "\n\nPots encaixar-los centrats sense ampliar-los ni retallar-los, o aturar la fusió per modificar-los manualment. Els originals no es modificaran.")
        }
        // Smallest exact 16:9 canvas at least as wide AND as high as every
        // source. This also handles portrait decks and equal-area dimensions.
        let unit = sizes.map { max(($0.0 + 15) / 16, ($0.1 + 8) / 9) }.max()!
        let target: (Int64, Int64) = (unit * 16, unit * 9)
        try requireValidSlideSize(target)
        var scalingWarnings: [String] = []
        for i in sizes.indices where sizes[i] != target {
            let fit = !SlideScaler.isWidescreen(sizes[i])
            try SlideScaler.normalize(package: roots[i], presentation: docs[i], from: sizes[i], to: target, fitWithoutEnlarging: fit)
            scalingWarnings.append("\(inputs[i].lastPathComponent): \(dimensions(sizes[i])) → \(dimensions(target)) (16:9); \(fit ? "centrat sense ampliar ni retallar" : "escala proporcional").")
        }
        let counts = docs.map { children(list($0.rootElement()!, "sldIdLst")!, "sldId").count }
        guard counts.allSatisfy({ $0 > 0 }) else { try fail("Hi ha una presentació sense diapositives.") }
        let base = roots[0], pres = docs[0], root = pres.rootElement()!
        let rels = try xml(base, "ppt/_rels/presentation.xml.rels"), ct = try xml(base, "[Content_Types].xml")
        var warnings = [
            "Motor Open XML directe (sense obrir PowerPoint).",
            "Cal revisar reproducció, edició, notes, tipus de lletra, enllaços externs i objectes OLE a PowerPoint."
        ]
        warnings.append(contentsOf: scalingWarnings)
        let baseTypes = ct.rootElement()!
        var nextSlide = (children(list(root, "sldIdLst")!, "sldId").compactMap { UInt32(attr($0, "id")) }.max() ?? 255) + 1
        var nextMaster: UInt64 = 2147483648
        for d in docs {
            if let masters = list(d.rootElement()!, "sldMasterIdLst") {
                for m in children(masters, "sldMasterId") {
                    if let id = UInt64(attr(m, "id")) { nextMaster = max(nextMaster, id + 1) }
                }
            }
        }
        // Master and layout IDs share a presentation-wide identifier space.
        for dir in roots {
            if let en = fm.enumerator(at: dir.appendingPathComponent("ppt/slideMasters"), includingPropertiesForKeys: nil) {
                for case let url as URL in en where url.pathExtension == "xml" {
                    let d = try XMLDocument(contentsOf: url)
                    for node in try d.nodes(forXPath: "//*[local-name() = 'sldLayoutId']") {
                        if let e = node as? XMLElement, let id = UInt64(attr(e, "id")) { nextMaster = max(nextMaster, id + 1) }
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
        warnings.append("Totes les notes utilitzen el primer patró de notes disponible. El contingut per diapositiva es conserva; el format heretat de les notes pot canviar.")
        // Relationship targets are rooted in their original source package. Each imported
        // package is namespaced intact; relative references therefore remain unchanged.
        for i in 1..<roots.count {
            let source = roots[i], prefix = "pptMerger/source\(i)/"
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
            guard let en = fm.enumerator(at: dest, includingPropertiesForKeys: [.isRegularFileKey]) else { try fail("No es pot llegir el paquet.") }
            for case let url as URL in en where url.lastPathComponent.hasSuffix(".rels") {
                let d = try XMLDocument(contentsOf: url, options: .nodePreserveAll)
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
                    let d = try XMLDocument(contentsOf: url, options: .nodePreserveAll)
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
                    try fail("Tipus de contingut contradictoris per a l’extensió .\(ext) a «\(inputs[i].lastPathComponent)».")
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
                guard let type = overrides[part] ?? defaults[url.pathExtension.lowercased()] else { try fail("Falta ContentType: \(part)") }
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
                    guard let rel = byID[rid(e)] else { try fail("Relació de presentació absent") }
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
                            try fail("Relació de font incrustada absent o no vàlida a «\(inputs[i].lastPathComponent)».")
                        }
                        let old = attr(rel, "Target")
                        let part = old.hasPrefix("/") ? String(old.dropFirst()) : URL(fileURLWithPath: "/ppt/").appendingPathComponent(old).standardized.path.dropFirst().description
                        setRID(style, addRel(attr(rel, "Type"), "/" + prefix + part))
                    }
                    targetFonts!.addChild(node)
                }
                warnings.append("\(inputs[i].lastPathComponent): fonts incrustades registrades al resultat. Revisa substitucions, permisos d’ús i possibles variants diferents amb el mateix nom.")
            }
            if fm.fileExists(atPath: source.appendingPathComponent("ppt/vbaProject.bin").path) { warnings.append("\(inputs[i].lastPathComponent): VBA conservat com a recurs inactiu; no integrat al projecte VBA de sortida.") }
        }
        let hasMacros = fm.fileExists(atPath: base.appendingPathComponent("ppt/vbaProject.bin").path)
        if output.pathExtension.lowercased() == "pptx" && hasMacros { try fail("El primer fitxer conté VBA: desa en PPTM per conservar-lo.") }
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
        guard nextSlide <= 2147483648 else { try fail("Hi ha massa identificadors de diapositiva per crear una presentació vàlida.") }
        try write(pres, base, "ppt/presentation.xml"); try write(rels, base, "ppt/_rels/presentation.xml.rels"); try write(ct, base, "[Content_Types].xml")
        let archive = temp.appendingPathComponent("result.zip")
        try run("/usr/bin/ditto", ["-c", "-k", "--norsrc", "--noextattr", base.path, archive.path])
        let verificationRoot = temp.appendingPathComponent("verify")
        try fm.createDirectory(at: verificationRoot, withIntermediateDirectories: true)
        try run("/usr/bin/ditto", ["-x", "-k", archive.path, verificationRoot.path])
        let validation = try PackageVerifier.verify(sources: originals, inputs: inputs, merged: verificationRoot)
        let report = MergeReport(engine: "OpenXML-namespaced-v0.5.1", inputFiles: inputs.map { $0.path }, slideCounts: counts, totalSlides: counts.reduce(0, +), warnings: warnings, powerPointPlaybackValidated: false, validation: validation, overallStatus: validation.allSatisfy { $0.issues.isEmpty } ? "structural_checks_passed_needs_review" : "import_failed_validation", validationScope: "Structural comparisons against each original; visual rendering, playback and editing not validated.")
        try fm.copyItem(at: archive, to: output)
        do {
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(report).write(to: URL(fileURLWithPath: output.path + ".report.json"), options: .withoutOverwriting)
            var lines = ["PPT MERGER — COMPROVACIONS PER FITXER", report.structuralValidationPassed ? "Comprovació estructural superada. Revisió visual i reproducció pendents." : "IMPORTACIÓ NO CORRECTA: diferències detectades.", "", "Mida de sortida: \(dimensions(target)) · 16:9", ""]
            for (i, check) in validation.enumerated() {
                lines += ["\(i + 1). \(check.file)", check.issues.isEmpty ? "ESTRUCTURA: comprovacions superades; no equival a fidelitat total." : "IMPORTACIÓ NO CORRECTA", "\(check.checkedSlides) diapositives; \(check.slidesWithAnimation) amb animació; \(check.binaryResourcesChecked) recursos comprovats."]
                lines += check.checks.map { "Comprovat: " + $0 }
                lines += check.issues.map { "ERROR: " + $0 }
                lines += check.needsReview.map { "REVISIÓ PENDENT: " + $0 }
                lines.append("")
            }
            lines += ["AVISOS"] + warnings
            try lines.joined(separator: "\n").write(to: URL(fileURLWithPath: output.path + ".report.txt"), atomically: false, encoding: .utf8)
        } catch {
            try? fm.removeItem(at: output)
            try? fm.removeItem(at: URL(fileURLWithPath: output.path + ".report.json"))
            throw error
        }
        return report
    }
}
