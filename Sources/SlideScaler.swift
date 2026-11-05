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

    /// Adapts every slide, layout and master to a new size without distorting
    /// anything and without leaving empty bands:
    /// - text, pictures, charts, tables and groups keep their proportions, scaled
    ///   by s = min(sx, sy), the factor that fits the old page in the new one;
    /// - text boxes, placeholders and connectors take the new proportion (their
    ///   text reflows at the scaled font size);
    /// - shapes and pictures spanning the page keep spanning it; pictures are
    ///   cropped instead of stretched, and so are picture backgrounds;
    /// - other objects keep their place relative to the nearest edge, or to the
    ///   centre when they sit in the middle third.
    /// When the proportion does not change this is plain proportional scaling.
    /// Animation coordinates are fractions of the slide, so they stay valid.
    static func normalize(package: URL, presentation: XMLDocument, from size: (Int64, Int64), to target: (Int64, Int64)) throws {
        guard size != target else { return }
        let page = Adaptation(from: size, to: target)
        let fm = FileManager.default
        // Notes and handouts use a separate page coordinate system and must not
        // be scaled just because slide dimensions change. Image/media bytes and
        // embedded workbooks are also left untouched.
        for folder in ["slides", "slideLayouts", "slideMasters", "theme", "charts"] {
            let dir = package.appendingPathComponent("ppt/" + folder)
            guard fm.fileExists(atPath: dir.path), let files = fm.enumerator(at: dir, includingPropertiesForKeys: nil) else { continue }
            for case let file as URL in files where file.pathExtension == "xml" {
                let document = try parseXML(at: file)
                let isMaster = folder == "slideMasters" || folder == "theme"
                // Frames are read before the uniform pass rescales them.
                var frames: [ShapeFrame] = []
                if ["slides", "slideLayouts", "slideMasters"].contains(folder),
                   let tree = try document.nodes(forXPath: "//*[local-name() = 'spTree']").first as? XMLElement {
                    collectFrames(tree, into: &frames)
                }
                if !page.isUnit { try scale(document, factor: page.s, isMaster: isMaster) }
                for frame in frames { page.place(frame) }
                if let root = document.rootElement(), let bg = try root.nodes(forXPath: ".//*[local-name() = 'bgPr']/*[local-name() = 'blipFill']").first as? XMLElement {
                    // A picture background always fills the slide: overflow it to the new proportion.
                    overflowFill(bg, widthGrowth: page.sx, heightGrowth: page.sy)
                }
                try serializedXML(document).write(to: file, options: .atomic)
            }
        }
        // Presentation default text styles can contain absolute font sizes.
        if !page.isUnit, let style = list(presentation.rootElement()!, "defaultTextStyle") {
            try scaleNode(style, factor: page.s, isMaster: true)
        }
        let slideSize = list(presentation.rootElement()!, "sldSz")!
        set(slideSize, "cx", String(target.0)); set(slideSize, "cy", String(target.1))
        // Existing type values such as screen4x3 would contradict the new size.
        slideSize.attribute(forName: "type")?.detach()
        try write(presentation, package, "ppt/presentation.xml")
    }

    /// A top-level object and its frame on the original page.
    struct ShapeFrame {
        let shape: XMLElement, off: XMLElement, ext: XMLElement
        let x: Double, y: Double, cx: Double, cy: Double
        var kind: String { shape.localName ?? "" }
    }

    private static func collectFrames(_ container: XMLElement, into frames: inout [ShapeFrame]) {
        for case let shape as XMLElement in container.children ?? [] {
            let local = shape.localName ?? ""
            if local == "AlternateContent" {
                for branch in children(shape, "Choice") + children(shape, "Fallback") { collectFrames(branch, into: &frames) }
                continue
            }
            guard ["sp", "pic", "grpSp", "cxnSp", "graphicFrame", "contentPart"].contains(local) else { continue }
            let holder = ["graphicFrame", "contentPart"].contains(local) ? shape : (list(shape, "spPr") ?? list(shape, "grpSpPr"))
            guard let holder, let transform = list(holder, "xfrm"), let off = list(transform, "off"), let ext = list(transform, "ext"),
                  let x = Double(attr(off, "x")), let y = Double(attr(off, "y")), let cx = Double(attr(ext, "cx")), let cy = Double(attr(ext, "cy")) else { continue }
            frames.append(ShapeFrame(shape: shape, off: off, ext: ext, x: x, y: y, cx: cx, cy: cy))
        }
    }

    /// Crops a picture fill (a:srcRect, in 1/1000 %) so a box whose width grew by
    /// `widthGrowth` and height by `heightGrowth` shows it without distortion.
    static func cropPicture(_ blipFill: XMLElement, widthGrowth: Double, heightGrowth: Double) {
        let k = widthGrowth / heightGrowth
        guard abs(k - 1) > 1e-6, list(blipFill, "tile") == nil else { return }
        var rect = list(blipFill, "srcRect")
        if rect == nil {
            let node = XMLElement(name: "a:srcRect", uri: drawingNS)
            let index = (blipFill.children ?? []).firstIndex { ($0 as? XMLElement)?.localName == "blip" }.map { $0 + 1 } ?? 0
            blipFill.insertChild(node, at: index)
            rect = node
        }
        guard let rect else { return }
        func value(_ key: String) -> Double { Double(attr(rect, key)) ?? 0 }
        if k > 1 {
            // Wider box: show less height.
            let visible = 1 - (value("t") + value("b")) / 100000, extra = (visible - visible / k) / 2 * 100000
            set(rect, "t", String(Int64((value("t") + extra).rounded()))); set(rect, "b", String(Int64((value("b") + extra).rounded())))
        } else {
            let visible = 1 - (value("l") + value("r")) / 100000, extra = (visible - visible * k) / 2 * 100000
            set(rect, "l", String(Int64((value("l") + extra).rounded()))); set(rect, "r", String(Int64((value("r") + extra).rounded())))
        }
    }

    /// Backgrounds are cropped the way PowerPoint stores picture-fill offsets:
    /// a:stretch/a:fillRect extends past the slide (negative values, 1/1000 %)
    /// on the sides that no longer fit, so the picture keeps its proportion.
    static func overflowFill(_ blipFill: XMLElement, widthGrowth: Double, heightGrowth: Double) {
        let k = widthGrowth / heightGrowth
        guard abs(k - 1) > 1e-6, let stretch = list(blipFill, "stretch") else { return }
        var rect = list(stretch, "fillRect")
        if rect == nil { let node = XMLElement(name: "a:fillRect", uri: drawingNS); stretch.addChild(node); rect = node }
        guard let rect else { return }
        func value(_ key: String) -> Double { Double(attr(rect, key)) ?? 0 }
        let (first, second, extra) = k > 1 ? ("t", "b", (k - 1) / 2 * 100000) : ("l", "r", (1 / k - 1) / 2 * 100000)
        set(rect, first, String(Int64((value(first) - extra).rounded()))); set(rect, second, String(Int64((value(second) - extra).rounded())))
    }

    /// Geometry of one resize; see `normalize`.
    struct Adaptation {
        let width: Double, height: Double, newWidth: Double, newHeight: Double
        let sx: Double, sy: Double, s: Double
        var isUnit: Bool { abs(s - 1) < 1e-9 }
        init(from: (Int64, Int64), to: (Int64, Int64)) {
            width = Double(from.0); height = Double(from.1); newWidth = Double(to.0); newHeight = Double(to.1)
            sx = newWidth / width; sy = newHeight / height; s = min(sx, sy)
        }
        /// New start and length on one axis.
        func axis(start: Double, length: Double, page: Double, newPage: Double, factor: Double, stretch: Bool) -> (Double, Double) {
            if stretch { return (start * factor, length * factor) }
            let size = length * s, centre = (start + length / 2) / page
            var position: Double
            if centre < 1.0 / 3 { position = start * s }                                   // near the start edge
            else if centre > 2.0 / 3 { position = newPage - (page - start - length) * s - size } // near the end edge
            else { position = centre * newPage - size / 2 }                                // middle third: keep centred
            if start >= 0 && start + length <= page { position = min(max(position, 0), max(newPage - size, 0)) }
            return (position, size)
        }
        func place(_ f: ShapeFrame) {
            let picture = f.kind == "pic" || (f.kind == "sp" && list(f.shape, "spPr").flatMap { list($0, "blipFill") } != nil)
            let text = f.kind == "sp" && !picture && ((try? f.shape.nodes(forXPath: ".//*[local-name() = 'ph']"))?.isEmpty == false || (try? f.shape.nodes(forXPath: ".//*[local-name() = 't']"))?.contains { !($0.stringValue ?? "").isEmpty } == true)
            let spansX = f.cx >= 0.9 * width, spansY = f.cy >= 0.9 * height
            let stretchX: Bool, stretchY: Bool
            switch f.kind {
            case "cxnSp": (stretchX, stretchY) = (true, true)
            case "sp" where text: (stretchX, stretchY) = (true, true)
            case "sp", "pic": (stretchX, stretchY) = (spansX, spansY)
            default: (stretchX, stretchY) = (false, false)  // groups, charts, tables, SmartArt: rigid
            }
            let (x, w) = axis(start: f.x, length: f.cx, page: width, newPage: newWidth, factor: sx, stretch: stretchX)
            let (y, h) = axis(start: f.y, length: f.cy, page: height, newPage: newHeight, factor: sy, stretch: stretchY)
            set(f.off, "x", String(Int64(x.rounded()))); set(f.off, "y", String(Int64(y.rounded())))
            set(f.ext, "cx", String(Int64(w.rounded()))); set(f.ext, "cy", String(Int64(h.rounded())))
            if picture, f.cx > 0, f.cy > 0, stretchX || stretchY {
                let fill = f.kind == "pic" ? list(f.shape, "blipFill") : list(f.shape, "spPr").flatMap { list($0, "blipFill") }
                if let fill { cropPicture(fill, widthGrowth: w / f.cx, heightGrowth: h / f.cy) }
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
