import Foundation

/// Converts physical DrawingML dimensions, never percentages, IDs, rotations,
/// image crops, animation durations or custom-geometry coordinate systems.
struct SlideScaler {
    static let drawingNS = "http://schemas.openxmlformats.org/drawingml/2006/main"
    static let target: (Int64, Int64) = (12192000, 6858000) // 33.8667 × 19.05 cm
    static func isWidescreen(_ size: (Int64, Int64)) -> Bool {
        abs(Double(size.0) / Double(size.1) / (16.0 / 9.0) - 1) <= 0.001
    }
    static func sameAspect(_ a: (Int64, Int64), _ b: (Int64, Int64)) -> Bool {
        abs((Double(a.0) / Double(a.1)) / (Double(b.0) / Double(b.1)) - 1) <= 0.001
    }

    static func normalize(package: URL, presentation: XMLDocument, from size: (Int64, Int64), to target: (Int64, Int64), fitWithoutEnlarging: Bool = false) throws {
        guard size != target else { return }
        let factor = fitWithoutEnlarging ? min(1, min(Double(target.0) / Double(size.0), Double(target.1) / Double(size.1))) : min(Double(target.0) / Double(size.0), Double(target.1) / Double(size.1))
        let shiftX = (Double(target.0) - Double(size.0) * factor) / 2
        let shiftY = (Double(target.1) - Double(size.1) * factor) / 2
        let isUnitScale = abs(factor - 1.0) < 1e-9
        let rx = (factor * Double(size.0)) / Double(target.0)
        let ry = (factor * Double(size.1)) / Double(target.1)
        let shiftXNorm = shiftX / Double(target.0)
        let shiftYNorm = shiftY / Double(target.1)
        let fm = FileManager.default
        // Notes and handouts use a separate page coordinate system and must not
        // be scaled just because slide dimensions change. Image/media bytes and
        // embedded workbooks are also left untouched.
        for folder in ["slides", "slideLayouts", "slideMasters", "theme", "charts"] {
            let dir = package.appendingPathComponent("ppt/" + folder)
            guard fm.fileExists(atPath: dir.path), let files = fm.enumerator(at: dir, includingPropertiesForKeys: nil) else { continue }
            for case let file as URL in files where file.pathExtension == "xml" {
                let document = try XMLDocument(contentsOf: file, options: .nodePreserveAll)
                let isMaster = folder == "slideMasters" || folder == "theme"
                if !isUnitScale {
                    try scale(document, factor: factor, isMaster: isMaster)
                }
                if ["slides", "slideLayouts", "slideMasters"].contains(folder) {
                    if let tree = try document.nodes(forXPath: "//*[local-name() = 'spTree']").first as? XMLElement {
                        // Translate only top-level transforms (including inside mc:AlternateContent):
                        // nested group children already move with their parent.
                        translateContainerChildren(tree, dx: shiftX, dy: shiftY)
                    }
                    if let root = document.rootElement() {
                        scaleTimingAnimations(root, rx: rx, ry: ry, shiftXNorm: shiftXNorm, shiftYNorm: shiftYNorm)
                    }
                }
                try serializedXML(document).write(to: file, options: .atomic)
            }
        }
        // Presentation default text styles can contain absolute font sizes.
        if !isUnitScale, let style = list(presentation.rootElement()!, "defaultTextStyle") {
            try scaleNode(style, factor: factor, isMaster: true)
        }
        let slideSize = list(presentation.rootElement()!, "sldSz")!
        set(slideSize, "cx", String(target.0)); set(slideSize, "cy", String(target.1))
        // Existing type values such as screen4x3 would contradict the new size.
        slideSize.attribute(forName: "type")?.detach()
        try write(presentation, package, "ppt/presentation.xml")
    }

    private static func translateContainerChildren(_ container: XMLElement, dx: Double, dy: Double) {
        for shape in container.children ?? [] {
            guard let shape = shape as? XMLElement else { continue }
            let local = shape.localName ?? ""
            if local == "AlternateContent" {
                for branch in (shape.children ?? []).compactMap({ $0 as? XMLElement }) {
                    if ["Choice", "Fallback"].contains(branch.localName ?? "") {
                        translateContainerChildren(branch, dx: dx, dy: dy)
                    }
                }
                continue
            }
            guard ["sp", "pic", "grpSp", "cxnSp", "graphicFrame", "contentPart"].contains(local) else { continue }
            let holder = ["graphicFrame", "contentPart"].contains(local) ? shape : (list(shape, "spPr") ?? list(shape, "grpSpPr"))
            if let holder, let transform = list(holder, "xfrm"), let offset = list(transform, "off") {
                for (key, delta) in [("x", dx), ("y", dy)] {
                    if let value = Double(attr(offset, key)) { set(offset, key, String(Int64((value + delta).rounded()))) }
                }
            }
        }
    }

    static func scaleTimingAnimations(_ root: XMLElement, rx: Double, ry: Double, shiftXNorm: Double, shiftYNorm: Double) {
        guard abs(rx - 1.0) > 1e-9 || abs(ry - 1.0) > 1e-9 else { return }
        if let motions = try? root.nodes(forXPath: ".//*[local-name() = 'animMotion']") {
            for case let motion as XMLElement in motions {
                guard let pathAttr = motion.attribute(forName: "path"), let raw = pathAttr.stringValue, !raw.isEmpty else { continue }
                let originParent = attr(motion, "origin") == "parent"
                pathAttr.stringValue = scaleMotionPath(raw, rx: rx, ry: ry, shiftXNorm: shiftXNorm, shiftYNorm: shiftYNorm, originParent: originParent)
            }
        }
        if let anims = try? root.nodes(forXPath: ".//*[local-name() = 'anim']") {
            for case let anim as XMLElement in anims {
                let attrNames = (try? anim.nodes(forXPath: ".//*[local-name() = 'attrName']")) ?? []
                let names = attrNames.compactMap { ($0 as? XMLElement)?.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines) }
                let isX = names.contains("ppt_x") || names.contains("style.left")
                let isY = names.contains("ppt_y") || names.contains("style.top")
                let isW = names.contains("ppt_w") || names.contains("style.width")
                let isH = names.contains("ppt_h") || names.contains("style.height")
                guard let tavs = try? anim.nodes(forXPath: ".//*[local-name() = 'tav']") else { continue }
                for case let tav as XMLElement in tavs {
                    let hasFormula = tav.attribute(forName: "fmla") != nil
                    if let fmlaAttr = tav.attribute(forName: "fmla"), let fmla = fmlaAttr.stringValue {
                        if (isX || fmla.contains("ppt_x")) && abs(rx - 1.0) > 1e-9 {
                            fmlaAttr.stringValue = scaleFormulaOffsets(fmla, variable: "ppt_x", factor: rx, shiftNorm: shiftXNorm)
                        } else if (isY || fmla.contains("ppt_y")) && abs(ry - 1.0) > 1e-9 {
                            fmlaAttr.stringValue = scaleFormulaOffsets(fmla, variable: "ppt_y", factor: ry, shiftNorm: shiftYNorm)
                        } else if (isW || fmla.contains("ppt_w")) && abs(rx - 1.0) > 1e-9 {
                            fmlaAttr.stringValue = scaleFormulaOffsets(fmla, variable: "ppt_w", factor: rx, shiftNorm: 0)
                        } else if (isH || fmla.contains("ppt_h")) && abs(ry - 1.0) > 1e-9 {
                            fmlaAttr.stringValue = scaleFormulaOffsets(fmla, variable: "ppt_h", factor: ry, shiftNorm: 0)
                        }
                    }
                    if !hasFormula, let fltVals = try? tav.nodes(forXPath: ".//*[local-name() = 'fltVal']") {
                        for case let flt as XMLElement in fltVals {
                            guard let valAttr = flt.attribute(forName: "val"), let v = Double(valAttr.stringValue ?? "") else { continue }
                            if isX {
                                valAttr.stringValue = formatCoord(v * rx + shiftXNorm)
                            } else if isY {
                                valAttr.stringValue = formatCoord(v * ry + shiftYNorm)
                            } else if isW {
                                valAttr.stringValue = formatCoord(v * rx)
                            } else if isH {
                                valAttr.stringValue = formatCoord(v * ry)
                            }
                        }
                    }
                }
            }
        }
    }

    static func scaleFormulaOffsets(_ fmla: String, variable: String, factor: Double, shiftNorm: Double) -> String {
        let trimmed = fmla.trimmingCharacters(in: .whitespaces)
        if !trimmed.contains(variable) {
            if abs(shiftNorm) > 1e-9 {
                return "(\(trimmed))*\(formatCoord(factor))+\(formatCoord(shiftNorm))"
            }
            return "(\(trimmed))*\(formatCoord(factor))"
        }
        let pattern = "^(\(NSRegularExpression.escapedPattern(for: variable)))\\s*([+-])\\s*([0-9]+(?:\\.[0-9]+)?)$"
        if let regex = try? NSRegularExpression(pattern: pattern),
           let match = regex.firstMatch(in: trimmed, range: NSRange(trimmed.startIndex..., in: trimmed)),
           let opRange = Range(match.range(at: 2), in: trimmed),
           let numRange = Range(match.range(at: 3), in: trimmed),
           let num = Double(trimmed[numRange]) {
            return "\(variable)\(trimmed[opRange])\(formatCoord(num * factor))"
        }
        return fmla
    }

    static func scaleMotionPath(_ path: String, rx: Double, ry: Double, shiftXNorm: Double = 0, shiftYNorm: Double = 0, originParent: Bool = false) -> String {
        var tokens: [String] = []
        var current = ""
        for ch in path {
            if ch.isWhitespace || ch == "," {
                if !current.isEmpty { tokens.append(current); current = "" }
            } else if ch.isLetter && ch != "e" && ch != "E" {
                if !current.isEmpty { tokens.append(current); current = "" }
                tokens.append(String(ch))
            } else if (ch == "e" || ch == "E") {
                if let last = current.last, last.isNumber || last == "." {
                    current.append(ch)
                } else {
                    if !current.isEmpty { tokens.append(current); current = "" }
                    tokens.append(String(ch))
                }
            } else {
                current.append(ch)
            }
        }
        if !current.isEmpty { tokens.append(current) }

        var out: [String] = []
        var currentCmd: Character = "M"
        var coordIndex = 0
        for token in tokens {
            if token.count == 1, let ch = token.first, ch.isLetter {
                currentCmd = ch
                coordIndex = 0
                out.append(token)
            } else if let v = Double(token) {
                let isX = (coordIndex % 2 == 0)
                let isAbsoluteParent = originParent && currentCmd.isUppercase
                let scaled: Double
                if isX {
                    scaled = v * rx + (isAbsoluteParent ? shiftXNorm : 0.0)
                } else {
                    scaled = v * ry + (isAbsoluteParent ? shiftYNorm : 0.0)
                }
                out.append(formatCoord(scaled))
                coordIndex += 1
            } else {
                out.append(token)
            }
        }
        return out.joined(separator: " ")
    }

    static func formatCoord(_ value: Double) -> String {
        if abs(value) < 1e-12 { return "0" }
        let rounded = (value * 1_000_000).rounded() / 1_000_000
        if abs(rounded - rounded.rounded()) < 1e-9 {
            return String(Int64(rounded.rounded()))
        }
        var s = String(format: "%.6f", rounded)
        while s.hasSuffix("0") { s.removeLast() }
        if s.hasSuffix(".") { s.removeLast() }
        return s
    }

    private static func scale(_ document: XMLDocument, factor: Double, isMaster: Bool) throws {
        if let root = document.rootElement() { try scaleNode(root, factor: factor, isMaster: isMaster) }
    }

    private static func scaleNode(_ node: XMLElement, factor: Double, isMaster: Bool) throws {
        guard abs(factor - 1.0) > 1e-9 else { return }
        func length(_ attribute: String) {
            guard let a = node.attribute(forName: attribute), let v = Double(a.stringValue ?? "") else { return }
            a.stringValue = String(Int64((v * factor).rounded()))
        }
        let local = node.localName ?? "", ns = node.uri ?? ""
        if ns == drawingNS {
            let parent = (node.parent as? XMLElement)?.localName ?? ""
            if ["off", "chOff"].contains(local), parent == "xfrm" { length("x"); length("y") }
            if ["ext", "chExt"].contains(local), parent == "xfrm" { length("cx"); length("cy") }
            if ["rPr", "defRPr", "endParaRPr"].contains(local) {
                for key in ["sz", "kern", "spc"] { length(key) }
                if let a = node.attribute(forName: "sz"), let v = Int64(a.stringValue ?? "") { a.stringValue = String(min(400000, max(100, v))) }
            }
            if local == "bodyPr" {
                for (key, value) in [("lIns", 91440), ("rIns", 91440), ("tIns", 45720), ("bIns", 45720)] {
                    if isMaster && node.attribute(forName: key) == nil { set(node, key, String(value)) }
                    length(key)
                }
                length("spcCol")
            }
            if local == "tcPr" {
                for key in ["marL", "marR", "marT", "marB"] {
                    length(key)
                }
            }
            if local == "pPr" || local.range(of: "^lvl[1-9]pPr$", options: .regularExpression) != nil {
                for key in ["marL", "marR", "indent", "defTabSz"] { length(key) }
            }
            if local == "tab" { length("pos") }
            if ["spcPts", "buSzPts"].contains(local) { length("val") }
            if local == "gridCol" { length("w") }
            if local == "tr" { length("h") }
            if local == "ln" || local.hasPrefix("lnB") || ["lnL", "lnR", "lnT"].contains(local) { length("w") }
            if ["outerShdw", "innerShdw", "prstShdw"].contains(local) { length("dist"); length("blurRad") }
            if ["glow", "softEdge"].contains(local) { length("rad") }
            if ["blur", "reflection"].contains(local) { length("rad"); length("blurRad"); length("dist") }
            if ["bevelT", "bevelB"].contains(local) { length("w"); length("h") }
            if local == "sp3d" { length("z"); length("extrusionH"); length("contourW") }
            if local == "backdrop" {
                // DrawingML 3D backgrounds are uncommon; descendants retain
                // their own coordinate definitions and need visual review.
            }
        }
        if ns == pNS {
            if local == "oleObj" { length("imgW"); length("imgH") }
            // p:graphicFrame/p:xfrm contains DrawingML off/ext children, handled above.
        }
        for child in node.children ?? [] { if let element = child as? XMLElement { try scaleNode(element, factor: factor, isMaster: isMaster) } }
    }
}
