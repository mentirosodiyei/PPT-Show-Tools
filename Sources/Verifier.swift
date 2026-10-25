import Foundation
import CryptoKit

func checksum(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }

func semantic(_ node: XMLNode?) -> String {
    guard let node else { return "ABSENT" }
    func field(_ s: String) -> String { "\(s.utf8.count):" + s }
    if let e = node as? XMLElement {
        let attributes = (e.attributes ?? []).map { field(($0.uri ?? "") + "|" + ($0.localName ?? $0.name ?? "")) + field($0.stringValue ?? "") }.sorted().joined()
        let body = (e.children ?? []).filter { child in
            child.kind == .element || !(child.stringValue ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || e.localName == "t"
        }.map { semantic($0) }.joined()
        return "E" + field(e.uri ?? "") + field(e.localName ?? "") + field(attributes) + field(body)
    }
    return "T" + field(node.stringValue ?? "")
}

func packageFiles(_ root: URL) -> [String] {
    let path = root.resolvingSymlinksInPath().path
    guard let files = FileManager.default.enumerator(at: root, includingPropertiesForKeys: [.isRegularFileKey]) else { return [] }
    return files.compactMap { item in
        guard let url = item as? URL, (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else { return nil }
        return String(url.resolvingSymlinksInPath().path.dropFirst(path.count + 1))
    }.sorted()
}

func resolvePart(owner: String, target: String) -> String {
    let clean = String(target.split(separator: "#", omittingEmptySubsequences: false).first ?? "").removingPercentEncoding ?? target
    let path = clean.hasPrefix("/") ? clean : "/" + (owner as NSString).deletingLastPathComponent + "/" + clean
    return String((path as NSString).standardizingPath.drop(while: { $0 == "/" }))
}

func orderedParts(_ root: URL) throws -> [String] {
    let d = try xml(root, "ppt/presentation.xml"), r = try xml(root, "ppt/_rels/presentation.xml.rels")
    let relationships = Dictionary(children(r.rootElement()!, "Relationship").map { (attr($0, "Id"), attr($0, "Target")) }, uniquingKeysWith: { _, last in last })
    guard let ids = list(d.rootElement()!, "sldIdLst") else { try fail("Falta la llista de diapositives.") }
    return try children(ids, "sldId").map { e in
        guard let target = relationships[rid(e)] else { try fail("Falta una relació de diapositiva.") }
        return resolvePart(owner: "ppt/presentation.xml", target: target)
    }
}

struct PartFingerprint {
    let timing: String
    let timingNode: XMLElement?
    let transition: String
    let text: [String]
    let shapeIdentity: [String]
    let shapeIDs: Set<String>
    let timingTargetIDs: Set<String>

    init(_ element: XMLElement) throws {
        let tNode = list(element, "timing")
        timing = semantic(tNode)
        timingNode = tNode?.copy() as? XMLElement
        transition = semantic(list(element, "transition"))
        var texts: [String] = [], shapes: [String] = [], ids = Set<String>(), targetIDs = Set<String>()
        func visit(_ e: XMLElement) {
            if e.localName == "t", e.uri == SlideScaler.drawingNS { texts.append(e.stringValue ?? "") }
            if e.localName == "cNvPr" {
                shapes.append(semantic(e))
                let id = attr(e, "id")
                if !id.isEmpty { ids.insert(id) }
            }
            if e.localName == "spTgt" {
                let spid = attr(e, "spid")
                if !spid.isEmpty { targetIDs.insert(spid) }
            }
            for child in e.children ?? [] {
                if let child = child as? XMLElement { visit(child) }
            }
        }
        visit(element)
        text = texts
        shapeIdentity = shapes
        shapeIDs = ids
        timingTargetIDs = targetIDs
    }

    func expectedTiming(from sourceSize: (Int64, Int64)?, to targetSize: (Int64, Int64)?) -> String {
        guard let timingNode, let s = sourceSize, let t = targetSize, s != t else { return timing }
        let fit = !SlideScaler.isWidescreen(s)
        let rawFactor = min(Double(t.0) / Double(s.0), Double(t.1) / Double(s.1))
        let factor = fit ? min(1.0, rawFactor) : rawFactor
        let rx = (Double(s.0) * factor) / Double(t.0)
        let ry = (Double(s.1) * factor) / Double(t.1)
        guard abs(rx - 1.0) > 1e-9 || abs(ry - 1.0) > 1e-9 else { return timing }
        let shiftXNorm = (Double(t.0) - Double(s.0) * factor) / 2.0 / Double(t.0)
        let shiftYNorm = (Double(t.1) - Double(s.1) * factor) / 2.0 / Double(t.1)
        guard let copy = timingNode.copy() as? XMLElement else { return timing }
        SlideScaler.scaleTimingAnimations(copy, rx: rx, ry: ry, shiftXNorm: shiftXNorm, shiftYNorm: shiftYNorm)
        return semantic(copy)
    }
}

struct ResourceLink {
    let id: String; let type: String; let target: String; let external: Bool
}

func ownerOfRels(_ file: String) -> String {
    if file == "_rels/.rels" { return "" }
    let directory = (file as NSString).deletingLastPathComponent
    return ((directory as NSString).deletingLastPathComponent as NSString).appendingPathComponent(String((file as NSString).lastPathComponent.dropLast(5)))
}

struct PackageSnapshot {
    let slides: [String]; let parts: [String: PartFingerprint]; let notes: [String: String]; let binaries: [String: String]
    let hasVBA: Bool
    let links: [String: [ResourceLink]]
    let slideSize: (Int64, Int64)?

    static func capture(_ root: URL) throws -> PackageSnapshot {
        var parts: [String: PartFingerprint] = [:], notes: [String: String] = [:], binaries: [String: String] = [:]
        var links: [String: [ResourceLink]] = [:]
        var slideSize: (Int64, Int64)? = nil
        if let presDoc = try? xml(root, "ppt/presentation.xml"),
           let presRoot = presDoc.rootElement(),
           let sz = list(presRoot, "sldSz"),
           let w = Int64(attr(sz, "cx")), let h = Int64(attr(sz, "cy")), w > 0, h > 0 {
            slideSize = (w, h)
        }
        for file in packageFiles(root) {
            if file.hasSuffix(".rels"), ["ppt/slides/", "ppt/slideLayouts/", "ppt/slideMasters/", "ppt/notesSlides/"].contains(where: { file.hasPrefix($0) }) {
                links[file] = children(try xml(root, file).rootElement()!, "Relationship").map {
                    ResourceLink(id: attr($0, "Id"), type: attr($0, "Type"), target: attr($0, "Target"), external: attr($0, "TargetMode") == "External")
                }
            }
            if file.hasSuffix(".xml"), ["ppt/slides/", "ppt/slideLayouts/", "ppt/slideMasters/"].contains(where: { file.hasPrefix($0) }) {
                parts[file] = try PartFingerprint(xml(root, file).rootElement()!)
            } else if file.hasPrefix("ppt/notesSlides/"), file.hasSuffix(".xml") {
                notes[file] = semantic(try xml(root, file).rootElement())
            } else if file.hasPrefix("ppt/media/") || file.hasPrefix("ppt/embeddings/") || file.hasPrefix("ppt/fonts/") {
                binaries[file] = checksum(try Data(contentsOf: root.appendingPathComponent(file)))
            }
        }
        return PackageSnapshot(
            slides: try orderedParts(root),
            parts: parts,
            notes: notes,
            binaries: binaries,
            hasVBA: FileManager.default.fileExists(atPath: root.appendingPathComponent("ppt/vbaProject.bin").path),
            links: links,
            slideSize: slideSize
        )
    }
}

struct FileValidation: Codable {
    let file: String; let status: String; let checkedSlides: Int; let slidesWithAnimation: Int
    let binaryResourcesChecked: Int; let checks: [String]; let issues: [String]; let needsReview: [String]
}

struct PackageVerifier {
    static func verify(sources: [PackageSnapshot], inputs: [URL], merged: URL) throws -> [FileValidation] {
        let ordered = try orderedParts(merged)
        var results: [FileValidation] = [], offset = 0
        let allFiles = packageFiles(merged), names = Set(allFiles)
        var broken: [String] = []

        var mergedSize: (Int64, Int64)? = nil
        let mergedPres = try xml(merged, "ppt/presentation.xml")
        if let presRoot = mergedPres.rootElement(),
           let sz = list(presRoot, "sldSz"),
           let w = Int64(attr(sz, "cx")), let h = Int64(attr(sz, "cy")), w > 0, h > 0 {
            mergedSize = (w, h)
        }

        // Verify ContentTypes coverage and ID uniqueness across the merged package
        var globalIssues: [String] = []
        if let ctRoot = (try? xml(merged, "[Content_Types].xml"))?.rootElement() {
            let defaults = Set(children(ctRoot, "Default").map { attr($0, "Extension").lowercased() })
            let overrides = Set(children(ctRoot, "Override").map { String(attr($0, "PartName").drop(while: { $0 == "/" })) })
            for file in allFiles where file != "[Content_Types].xml" {
                let ext = file.hasSuffix(".rels") ? "rels" : (file as NSString).pathExtension.lowercased()
                if !overrides.contains(file) && !defaults.contains(ext) {
                    globalIssues.append("Falta ContentType al paquet fusionat: \(file)")
                }
            }
        }
        if let presRoot = mergedPres.rootElement() {
            if let sldIds = list(presRoot, "sldIdLst") {
                let ids = children(sldIds, "sldId").map { attr($0, "id") }
                if Set(ids).count != ids.count { globalIssues.append("IDs de diapositiva (sldId) duplicats a presentation.xml.") }
            }
            var masterLayoutIDs: [String] = []
            if let masterIds = list(presRoot, "sldMasterIdLst") {
                masterLayoutIDs.append(contentsOf: children(masterIds, "sldMasterId").map { attr($0, "id") })
            }
            for file in allFiles where file.hasSuffix(".xml") && file.contains("/slideMasters/") {
                if let d = try? xml(merged, file), let nodes = try? d.nodes(forXPath: "//*[local-name() = 'sldLayoutId']") {
                    for case let e as XMLElement in nodes { masterLayoutIDs.append(attr(e, "id")) }
                }
            }
            if Set(masterLayoutIDs).count != masterLayoutIDs.count {
                globalIssues.append("IDs de patró o disseny (sldMasterId/sldLayoutId) duplicats al paquet fusionat.")
            }
        }

        for file in allFiles {
            if file.hasSuffix(".xml") || file.hasSuffix(".rels") { _ = try xml(merged, file) }
            guard file.hasSuffix(".rels") else { continue }
            let directory = (file as NSString).deletingLastPathComponent
            let owner = file == "_rels/.rels" ? "" : ((directory as NSString).deletingLastPathComponent as NSString).appendingPathComponent(String((file as NSString).lastPathComponent.dropLast(5)))
            let doc = try xml(merged, file)
            for relation in children(doc.rootElement()!, "Relationship") where attr(relation, "TargetMode") != "External" {
                let target = resolvePart(owner: owner, target: attr(relation, "Target"))
                if !names.contains(target) { broken.append(file + " → " + target) }
            }
        }

        for (i, source) in sources.enumerated() {
            let prefix = i == 0 ? "" : "pptMerger/source\(i)/"
            var issues: [String] = []
            if i == 0 { issues.append(contentsOf: globalIssues) }
            let expected = source.slides.map { prefix + $0 }
            let actual = Array(ordered.dropFirst(offset).prefix(expected.count))
            if actual != expected { issues.append("El nombre o l’ordre de les diapositives no coincideix amb l’original.") }
            for (part, original) in source.parts.sorted(by: { $0.key < $1.key }) {
                let destination = prefix + part
                guard names.contains(destination) else { issues.append("Falta la diapositiva, el disseny o el patró: \(part)"); continue }
                let imported = try PartFingerprint(xml(merged, destination).rootElement()!)
                let number = source.slides.firstIndex(of: part).map { "diapositiva \($0 + 1)" } ?? part
                let expectedTiming = original.expectedTiming(from: source.slideSize, to: mergedSize)
                if expectedTiming != imported.timing { issues.append("\(number): l’animació/temporització no coincideix amb l’original (absent o modificada).") }
                if original.transition != imported.transition { issues.append("\(number): transició o avançament automàtic absent/modificat.") }
                if original.text != imported.text { issues.append("\(number): text absent o modificat.") }
                if original.shapeIdentity != imported.shapeIdentity { issues.append("\(number): objectes, identificadors o accions interactives absents/modificats.") }
                let validOriginalTargets = original.timingTargetIDs.intersection(original.shapeIDs)
                if !validOriginalTargets.isSubset(of: imported.shapeIDs) {
                    issues.append("\(number): referència d’animació (spid) òrfena cap a un objecte inexistent.")
                }
            }
            for (part, links) in source.links.sorted(by: { $0.key < $1.key }) {
                guard names.contains(prefix + part) else { issues.append("Falten relacions de recursos: \(part)"); continue }
                let imported = children(try xml(merged, prefix + part).rootElement()!, "Relationship")
                for link in links where !link.type.hasSuffix("/notesMaster") {
                    guard let relation = imported.first(where: { attr($0, "Id") == link.id }) else { issues.append("Falta la relació \(link.id) a \(part)"); continue }
                    let expected = link.external ? link.target : prefix + resolvePart(owner: ownerOfRels(part), target: link.target)
                    let actual = link.external ? attr(relation, "Target") : resolvePart(owner: ownerOfRels(prefix + part), target: attr(relation, "Target"))
                    if actual != expected || attr(relation, "Type") != link.type || (attr(relation, "TargetMode") == "External") != link.external {
                        issues.append("Recurs enllaçat incorrectament: \(part), relació \(link.id)")
                    }
                }
            }
            for (part, original) in source.notes.sorted(by: { $0.key < $1.key }) {
                if try !names.contains(prefix + part) || semantic(xml(merged, prefix + part).rootElement()) != original { issues.append("Notes absents o modificades: \(part)") }
            }
            for (part, original) in source.binaries.sorted(by: { $0.key < $1.key }) {
                if try !names.contains(prefix + part) || checksum(Data(contentsOf: merged.appendingPathComponent(prefix + part))) != original { issues.append("Recurs multimèdia/font/objecte incrustat absent o modificat: \(part)") }
            }
            issues.append(contentsOf: broken.filter { i == 0 ? !$0.hasPrefix("pptMerger/") : $0.hasPrefix(prefix) }.map { "Relació interna trencada: " + $0 })
            if i == sources.count - 1 && ordered.count != sources.reduce(0, { $0 + $1.slides.count }) { issues.append("El recompte total de diapositives no coincideix.") }
            var review = ["No comprovat: aspecte visual, geometria final, fonts heretades, reproducció d’animacions/àudio/vídeo, edició i enllaços externs."]
            if source.hasVBA { review.append(i == 0 ? "VBA del primer fitxer conservat, però funcionament no comprovat." : "El projecte VBA d’aquest fitxer no s’ha integrat: importació incompleta si depèn de macros.") }
            results.append(FileValidation(
                file: inputs[i].lastPathComponent,
                status: issues.isEmpty ? "structural_checks_passed_needs_review" : "import_failed_validation",
                checkedSlides: source.slides.count,
                slidesWithAnimation: source.slides.filter { source.parts[$0]?.timing != "ABSENT" }.count,
                binaryResourcesChecked: source.binaries.count,
                checks: [
                    "Ordre i nombre de diapositives",
                    "Arbres complets d’animacions i temporitzacions (també patrons i dissenys) i integritat de spid",
                    "Transicions i avançaments automàtics",
                    "Text, objectes i accions",
                    "Contingut i patró de notes",
                    "Hash SHA-256 dels recursos multimèdia, fonts i objectes incrustats",
                    "XML, ContentTypes, unicitat d’IDs i relacions internes"
                ],
                issues: issues,
                needsReview: review
            ))
            offset += source.slides.count
        }
        return results
    }

    static func inspect(inputs: [URL], output: URL) throws -> [FileValidation] {
        let fm = FileManager.default, temp = fm.temporaryDirectory.appendingPathComponent("PPTCheck-" + UUID().uuidString)
        try fm.createDirectory(at: temp, withIntermediateDirectories: true); defer { try? fm.removeItem(at: temp) }
        var snapshots: [PackageSnapshot] = []
        for (i, input) in inputs.enumerated() {
            let dir = temp.appendingPathComponent("s\(i)"); try fm.createDirectory(at: dir, withIntermediateDirectories: true)
            try extractPackage(input, to: dir); snapshots.append(try PackageSnapshot.capture(dir))
        }
        let out = temp.appendingPathComponent("out"); try fm.createDirectory(at: out, withIntermediateDirectories: true)
        try extractPackage(output, to: out)
        return try verify(sources: snapshots, inputs: inputs, merged: out)
    }
}
