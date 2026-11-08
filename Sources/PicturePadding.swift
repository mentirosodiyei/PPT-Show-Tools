import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Pictures that fill the slide (backgrounds, full-bleed photos) are never
/// cropped or stretched when the proportion changes. The whole picture is
/// placed, centred, on a new canvas of the new proportion; the margins take
/// the colour of the nearest edge of the picture, row by row (or column by
/// column) and softened, so the join is hard to see. The new image is a new
/// media part; the original stays untouched for anything else that uses it.
/// Same algorithm as windows/src/padding.rs.
enum PicturePadding {
    static let imageRelationship = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"
    /// Canvas limit: big enough for 8K walls, small enough for memory.
    static let maxPixels = 36_000_000.0

    /// State shared by every part of one package.
    final class Pad {
        var made: [String: String] = [:]
        var extensions = Set<String>()
        var next = 0
    }

    /// One part (slide, layout or master) and its relationships.
    final class PartPad {
        let package: URL, part: String
        private var rels: XMLDocument?
        private var dirty = false
        init(package: URL, part: String) { self.package = package; self.part = part }
        private var relsPart: String {
            ((part as NSString).deletingLastPathComponent as NSString).appendingPathComponent("_rels/" + (part as NSString).lastPathComponent + ".rels")
        }

        /// Replaces the picture of `blipFill` by a copy with margins added on each side,
        /// as fractions of the picture's own width or height: [left, right, top, bottom].
        /// The picture itself is unchanged and undistorted. False when that is not
        /// possible (other formats, existing crops, tiles).
        func pad(_ state: Pad, _ blipFill: XMLElement, _ sides: [Double]) -> Bool {
            if sides.allSatisfy({ abs($0) <= 1e-6 }) { return true }
            if list(blipFill, "tile") != nil || hasOffsets(list(blipFill, "srcRect")) || hasOffsets(list(blipFill, "stretch").flatMap { list($0, "fillRect") }) { return false }
            guard let blip = list(blipFill, "blip"), let embed = blip.attribute(forLocalName: "embed", uri: rNS)?.stringValue, !embed.isEmpty else { return false }
            if rels == nil { rels = try? xml(package, relsPart) }
            guard let rels, let root = rels.rootElement(),
                  let rel = children(root, "Relationship").first(where: { attr($0, "Id") == embed && attr($0, "TargetMode") != "External" }) else { return false }
            let media = resolvePart(owner: part, target: attr(rel, "Target"))
            let ext = (media as NSString).pathExtension.lowercased()
            guard ["png", "jpg", "jpeg"].contains(ext) else { return false }
            let key = media + sides.map { String(format: "|%.6f", $0) }.joined()
            let name: String
            if let known = state.made[key] {
                name = known
            } else {
                guard let data = try? Data(contentsOf: package.appendingPathComponent(media)),
                      let bytes = PicturePadding.padded(data, jpeg: ext != "png", sides) else { return false }
                name = "ppt/media/pst-pad-\(UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased().prefix(8)).\(ext)"
                guard (try? bytes.write(to: package.appendingPathComponent(name))) != nil else { return false }
                state.extensions.insert(ext); state.made[key] = name
            }
            var id: String
            repeat { state.next += 1; id = "rIdPad\(state.next)" } while children(root, "Relationship").contains { attr($0, "Id") == id }
            let e = XMLElement(name: "Relationship", uri: pkgNS)
            set(e, "Id", id); set(e, "Type", PicturePadding.imageRelationship); set(e, "Target", "/" + name)
            root.addChild(e); dirty = true
            if let a = blip.attribute(forLocalName: "embed", uri: rNS) { a.stringValue = id }
            return true
        }
        private func loadRels() -> XMLElement? {
            if rels == nil { rels = try? xml(package, relsPart) }
            return rels?.rootElement()
        }
        /// Target part of a relationship of this part (internal ones only).
        func target(_ id: String) -> String? {
            guard let root = loadRels(), let rel = children(root, "Relationship").first(where: { attr($0, "Id") == id && attr($0, "TargetMode") != "External" }) else { return nil }
            return resolvePart(owner: part, target: attr(rel, "Target"))
        }
        /// Adds a relationship and returns its new id.
        func addRelationship(_ state: Pad, _ kind: String, _ target: String) -> String? {
            guard let root = loadRels() else { return nil }
            var id: String
            repeat { state.next += 1; id = "rIdPad\(state.next)" } while children(root, "Relationship").contains { attr($0, "Id") == id }
            let e = XMLElement(name: "Relationship", uri: pkgNS)
            set(e, "Id", id); set(e, "Type", kind); set(e, "Target", target)
            root.addChild(e); dirty = true
            return id
        }
        func save() throws {
            if dirty, let rels { try write(rels, package, relsPart) }
        }
    }

    static func hasOffsets(_ rect: XMLElement?) -> Bool {
        guard let rect else { return false }
        return ["l", "t", "r", "b"].contains { abs(Double(attr(rect, $0)) ?? 0) > 0 }
    }

    /// When a picture background cannot be padded, it is shown whole (inset) rather than cropped.
    static func insetFill(_ blipFill: XMLElement, _ gx: Double, _ gy: Double) {
        let k = gx / gy
        guard abs(k - 1) > 1e-6, let stretch = list(blipFill, "stretch") else { return }
        var rect = list(stretch, "fillRect")
        if rect == nil { let node = XMLElement(name: "a:fillRect", uri: SlideScaler.drawingNS); stretch.addChild(node); rect = node }
        guard let rect else { return }
        func v(_ key: String) -> Double { Double(attr(rect, key)) ?? 0 }
        let (first, second, share) = k > 1 ? ("l", "r", 1 - 1 / k) : ("t", "b", 1 - k)
        let room = 100000 - v(first) - v(second)
        let values = [first, second].map { v($0) + share / 2 * room }
        set(rect, first, String(Int64(values[0].rounded()))); set(rect, second, String(Int64(values[1].rounded())))
    }

    /// Registers the formats of the new images in [Content_Types].xml.
    static func register(_ package: URL, _ state: Pad) throws {
        guard !state.extensions.isEmpty else { return }
        let ct = try xml(package, "[Content_Types].xml")
        guard let root = ct.rootElement() else { return }
        for ext in state.extensions.sorted() where !children(root, "Default").contains(where: { attr($0, "Extension").lowercased() == ext }) {
            let d = XMLElement(name: "Default", uri: root.uri)
            set(d, "Extension", ext); set(d, "ContentType", ext == "png" ? "image/png" : ext == "mp4" ? "video/mp4" : "image/jpeg")
            root.addChild(d)
        }
        try write(ct, package, "[Content_Types].xml")
    }

    /// Equal margins on both sides for a box whose width grew by gx and height by gy
    /// (a picture background, which always fills the slide).
    static func centred(_ gx: Double, _ gy: Double) -> [Double] {
        let s = min(gx, gy), mx = (gx / s - 1) / 2, my = (gy / s - 1) / 2
        return [mx, mx, my, my]
    }

    /// Canvas size, picture size and picture position for a `w` × `h` picture with `sides` margins.
    static func canvas(_ w: Int, _ h: Int, _ sides: [Double]) -> (Int, Int, Int, Int, Int, Int) {
        let cw = Double(w) * (1 + sides[0] + sides[1]), ch = Double(h) * (1 + sides[2] + sides[3])
        let f = min(1, (maxPixels / (cw * ch)).squareRoot())
        func px(_ v: Double) -> Int { max(1, Int((v * f).rounded())) }
        let (c, d, i, j) = (px(cw), px(ch), px(Double(w)), px(Double(h)))
        let ox = min(Int((Double(w) * sides[0] * f).rounded()), max(c - i, 0)), oy = min(Int((Double(h) * sides[2] * f).rounded()), max(d - j, 0))
        return (c, d, i, j, ox, oy)
    }

    /// Average colour of each row (or column) of an edge strip `stripDiv` deep,
    /// softened along the edge over `blurDiv` of its length. `pixels` is RGBA, row 0 at the top.
    static func edgeColours(_ pixels: [UInt8], _ w: Int, _ h: Int, horizontal: Bool, far: Bool, stripDiv: Int, blurDiv: Int) -> [[Double]] {
        let (lines, depth) = horizontal ? (h, w) : (w, h)
        let strip = max(depth / stripDiv, 1)
        var raw: [[Double]] = []
        raw.reserveCapacity(lines)
        for line in 0..<lines {
            var sum = [0.0, 0, 0, 0]
            for d0 in 0..<strip {
                let d = far ? depth - 1 - d0 : d0
                let (x, y) = horizontal ? (d, line) : (line, d)
                let i = (y * w + x) * 4
                for c in 0..<4 { sum[c] += Double(pixels[i + c]) }
            }
            raw.append(sum.map { $0 / Double(strip) })
        }
        let radius = max(lines / blurDiv, 1)
        return (0..<lines).map { i in
            let a = max(i - radius, 0), b = min(i + radius, lines - 1)
            var sum = [0.0, 0, 0, 0]
            for j in a...b { for c in 0..<4 { sum[c] += raw[j][c] } }
            return sum.map { $0 / Double(b - a + 1) }
        }
    }

    static func rgba(_ image: CGImage, _ w: Int, _ h: Int) -> [UInt8]? {
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let ok = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let ctx = CGContext(data: buffer.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            ctx.interpolationQuality = .high
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        return ok ? pixels : nil
    }

    /// The padded image, encoded as JPEG (photos) or PNG.
    static func padded(_ data: Data, jpeg: Bool, _ sides: [Double]) -> Data? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil), let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { return nil }
        let w = image.width, h = image.height
        guard w > 0, h > 0, let original = rgba(image, w, h) else { return nil }
        let (cw, ch, iw, ih, ox, oy) = canvas(w, h, sides)
        let horizontal = cw > iw
        // Next to the picture the margin takes its exact edge; further out it fades
        // into a soft wash of the same colours, so the join does not show.
        let sharp = [false, true].map { edgeColours(original, w, h, horizontal: horizontal, far: $0, stripDiv: 200, blurDiv: 200) }
        let soft = [false, true].map { edgeColours(original, w, h, horizontal: horizontal, far: $0, stripDiv: 25, blurDiv: 8) }
        let fade = max(0.4 * Double(horizontal ? iw : ih), 1)
        func mix(_ side: Int, _ line: Int, _ distance: Int) -> [Double] {
            // Ease-out: sharp details of the edge dissolve quickly.
            let line = min(line, sharp[side].count - 1), linear = min(Double(distance) / fade, 1), t = linear * (2 - linear)
            return (0..<4).map { sharp[side][line][$0] * (1 - t) + soft[side][line][$0] * t }
        }
        guard let picture = (iw, ih) == (w, h) ? original : rgba(image, iw, ih) else { return nil }
        var out = [UInt8](repeating: 0, count: cw * ch * 4)
        func put(_ x: Int, _ y: Int, _ c: [Double]) {
            let i = (y * cw + x) * 4
            for k in 0..<4 { out[i + k] = UInt8(max(0, min(255, c[k].rounded()))) }
        }
        for y in 0..<ch {
            for x in 0..<cw {
                let insideX = x >= ox && x < ox + iw, insideY = y >= oy && y < oy + ih
                if insideX && insideY {
                    let s = ((y - oy) * iw + (x - ox)) * 4, d = (y * cw + x) * 4
                    out[d] = picture[s]; out[d + 1] = picture[s + 1]; out[d + 2] = picture[s + 2]; out[d + 3] = picture[s + 3]
                } else if horizontal {
                    let line = Int((Double(min(max(y, oy), oy + ih - 1) - oy) + 0.5) * Double(h) / Double(ih))
                    put(x, y, x < ox ? mix(0, line, ox - x) : mix(1, line, x + 1 - (ox + iw)))
                } else {
                    let line = Int((Double(min(max(x, ox), ox + iw - 1) - ox) + 0.5) * Double(w) / Double(iw))
                    put(x, y, y < oy ? mix(0, line, oy - y) : mix(1, line, y + 1 - (oy + ih)))
                }
            }
        }
        let result = NSMutableData()
        let made: Bool = out.withUnsafeMutableBytes { buffer in
            guard let ctx = CGContext(data: buffer.baseAddress, width: cw, height: ch, bitsPerComponent: 8, bytesPerRow: cw * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
                  let padded = ctx.makeImage(),
                  let destination = CGImageDestinationCreateWithData(result, (jpeg ? UTType.jpeg : UTType.png).identifier as CFString, 1, nil) else { return false }
            CGImageDestinationAddImage(destination, padded, jpeg ? [kCGImageDestinationLossyCompressionQuality: 0.9] as CFDictionary : nil)
            return CGImageDestinationFinalize(destination)
        }
        return made ? result as Data : nil
    }
}
