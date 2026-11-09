import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Images to PowerPoint: one or several pictures per slide, never cropped or
/// stretched, with an optional transition and automatic advance. Same rules
/// as windows/src/images.rs.
enum ImageLayout: String, CaseIterable, Identifiable {
    case one, four, random
    var id: String { rawValue }
    var title: String {
        switch self { case .one: return "One per slide"; case .four: return "Four per slide"; case .random: return "Random" }
    }
}

enum SlideTransition: String, CaseIterable, Identifiable {
    case none, fade, push, wipe, split, cover, dissolve, zoom, random
    var id: String { rawValue }
    var title: String { self == .none ? "None" : rawValue.prefix(1).uppercased() + rawValue.dropFirst() }
    /// The p:transition element; `advance` in seconds, nil for “on click”.
    func xml(advance: Double?) -> String {
        let effect: String
        switch self {
        case .none: effect = ""
        case .push: effect = "<p:push dir=\"l\"/>"
        case .wipe: effect = "<p:wipe dir=\"l\"/>"
        case .split: effect = "<p:split orient=\"vert\" dir=\"out\"/>"
        case .cover: effect = "<p:cover dir=\"l\"/>"
        default: effect = "<p:\(rawValue)/>"
        }
        let time = advance.map { " advTm=\"\(Int(($0 * 1000).rounded()))\"" } ?? ""
        if effect.isEmpty && time.isEmpty { return "" }
        return effect.isEmpty ? "<p:transition\(time)/>" : "<p:transition spd=\"med\"\(time)>\(effect)</p:transition>"
    }
}

extension PresentationTools {
    /// Formats read today by cameras, phones and the web.
    static let imageExtensions: Set<String> = ["jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "avif", "tif", "tiff", "bmp"]
    static let maxImages = 2000

    /// Files and folders as dropped or chosen: folders give their images (not
    /// subfolders), sorted by name as Finder shows them. Hidden files are skipped.
    static func imageFiles(in urls: [URL]) -> [URL] {
        var out: [URL] = []
        for url in urls {
            var isFolder: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isFolder) else { continue }
            if isFolder.boolValue {
                let items = (try? FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: [.isRegularFileKey], options: [.skipsHiddenFiles])) ?? []
                out += items.filter { imageExtensions.contains($0.pathExtension.lowercased()) && (try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true }
                    .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
            } else if imageExtensions.contains(url.pathExtension.lowercased()) {
                out.append(url)
            }
        }
        return out
    }

    struct PreparedImage { let data: Data; let ext: String; let width: Double; let height: Double; let note: String? }

    /// JPEG, PNG and GIF go in unchanged; other formats, and photos stored
    /// sideways, are converted (JPEG, or PNG when they have transparency).
    static func prepareImage(_ url: URL) -> PreparedImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), CGImageSourceGetCount(source) > 0,
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let pw = props[kCGImagePropertyPixelWidth] as? Int, let ph = props[kCGImagePropertyPixelHeight] as? Int, pw > 0, ph > 0 else { return nil }
        let ext = url.pathExtension.lowercased(), orientation = props[kCGImagePropertyOrientation] as? Int ?? 1
        let turned = (5...8).contains(orientation)
        let (w, h) = turned ? (Double(ph), Double(pw)) : (Double(pw), Double(ph))
        if ["jpg", "jpeg", "png", "gif"].contains(ext) && orientation == 1, let data = try? Data(contentsOf: url) {
            return PreparedImage(data: data, ext: ext == "jpeg" ? "jpg" : ext, width: w, height: h, note: nil)
        }
        let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: max(pw, ph)]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        // Decoders often add an alpha channel to opaque photos: look at the pixels.
        let alpha = ![.none, .noneSkipFirst, .noneSkipLast].contains(image.alphaInfo) && hasTransparentPixels(image)
        let png = alpha || ext == "png" || ext == "gif"
        let result = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(result, (png ? UTType.png : UTType.jpeg).identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, image, png ? nil : [kCGImageDestinationLossyCompressionQuality: 0.92] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { return nil }
        let note = orientation != 1 && ["jpg", "jpeg", "png", "gif"].contains(ext) ? "turned upright" : "converted from \(ext.uppercased()) to \(png ? "PNG" : "JPEG")"
        return PreparedImage(data: result as Data, ext: png ? "png" : "jpg", width: Double(image.width), height: Double(image.height), note: note)
    }

    static func hasTransparentPixels(_ image: CGImage) -> Bool {
        let w = image.width, h = image.height
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let ctx = CGContext(data: buffer.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h)); return true
        }
        guard drawn else { return true }
        return stride(from: 3, to: pixels.count, by: 4).contains { pixels[$0] < 255 }
    }

    /// Spaces for `count` pictures on a `w` × `h` slide (points): one fills the
    /// slide; two side by side; three as one large and two stacked; four in a grid.
    static func cells(_ count: Int, _ w: Double, _ h: Double) -> [CGRect] {
        guard count > 1 else { return [CGRect(x: 0, y: 0, width: w, height: h)] }
        let m = 0.03 * min(w, h), iw = w - 2 * m, ih = h - 2 * m, cw = (iw - m) / 2, ch = (ih - m) / 2
        switch count {
        case 2: return [CGRect(x: m, y: m, width: cw, height: ih), CGRect(x: m + cw + m, y: m, width: cw, height: ih)]
        case 3: return [CGRect(x: m, y: m, width: cw, height: ih), CGRect(x: m + cw + m, y: m, width: cw, height: ch), CGRect(x: m + cw + m, y: m + ch + m, width: cw, height: ch)]
        default: return (0..<4).map { CGRect(x: m + Double($0 % 2) * (cw + m), y: m + Double($0 / 2) * (ch + m), width: cw, height: ch) }
        }
    }

    /// SplitMix64: the same random layouts on macOS and Windows for the same seed.
    struct SplitMix { var state: UInt64; mutating func next() -> UInt64 { state &+= 0x9E3779B97F4A7C15; var z = state; z = (z ^ (z >> 30)) &* 0xBF58476D1CE4E5B9; z = (z ^ (z >> 27)) &* 0x94D049BB133111EB; return z ^ (z >> 31) } }

    /// How many pictures go on each slide.
    static func groups(_ total: Int, _ layout: ImageLayout, seed: UInt64) -> [Int] {
        var out: [Int] = [], left = total, random = SplitMix(state: seed)
        while left > 0 {
            let n: Int
            switch layout {
            case .one: n = 1
            case .four: n = min(4, left)
            case .random: n = 1 + Int(random.next() % UInt64(min(4, left)))
            }
            out.append(n); left -= n
        }
        return out
    }

    static func imagesToPPT(inputs: [URL], output: URL, fixedSize: (Int64, Int64)? = nil, layout: ImageLayout = .one, transition: SlideTransition = .none, advance: Double? = nil) throws -> ToolReport {
        let files = imageFiles(in: inputs)
        guard !files.isEmpty else { try fail("No images found. Add JPEG, PNG, GIF, HEIC, WebP, AVIF, TIFF or BMP files, or a folder that contains them.") }
        guard files.count <= maxImages else { try fail("There are \(files.count) images; at most \(maxImages) can go into one presentation.") }
        guard output.pathExtension.lowercased() == "pptx" else { try fail("The output must be PPTX.") }
        guard !fm.fileExists(atPath: output.path), !files.contains(where: { $0.standardizedFileURL == output.standardizedFileURL }), !ReportStore.collides(output, [".report.json", ".report.log"]) else {
            try fail("The output or its report already exists. Choose a new name; the originals are not modified.")
        }
        if let advance { guard advance > 0, advance <= 3600 else { try fail("Automatic advance must be between 0.1 and 3600 seconds.") } }
        // Slide size from Settings; Automatic is 16:9 (1280 × 720 px, PowerPoint's standard).
        let width = fixedSize.map { Double($0.0) / 12700 } ?? 960, height = fixedSize.map { Double($0.1) / 12700 } ?? 540
        var prepared: [(URL, PreparedImage)] = [], skipped: [String] = []
        for file in files {
            if let image = autoreleasepool(invoking: { prepareImage(file) }) { prepared.append((file, image)) } else { skipped.append(file.lastPathComponent) }
        }
        guard !prepared.isEmpty else { try fail("None of the images could be read.") }
        let seed = UInt64(ProcessInfo.processInfo.environment["PPTSHOWTOOLS_RANDOM_SEED"] ?? "") ?? UInt64(Date().timeIntervalSince1970 * 1000)
        let plan = groups(prepared.count, layout, seed: seed)
        let root = try temp(), staging = try temp(); defer { try? fm.removeItem(at: root); try? fm.removeItem(at: staging) }
        let staged = staging.appendingPathComponent("result.pptx")
        let builder = try SimpleDeck(root: root, width: width, height: height)
        let transitionXML = transition.xml(advance: advance)
        var pages: [ToolPage] = [], hashes: [String: String] = [:], next = 0
        for (slide, count) in plan.enumerated() {
            var body = "", relations: [(String, String, String)] = [], warnings: [String] = []
            for (i, cell) in cells(count, width, height).enumerated() {
                let (file, image) = prepared[next]; next += 1
                let media = "ppt/media/image\(next).\(image.ext)"
                try image.data.write(to: root.appendingPathComponent(media))
                hashes[media] = checksum(image.data); builder.extensions.insert(image.ext)
                relations.append(("img\(i + 1)", "image", "../media/image\(next).\(image.ext)"))
                let scale = min(cell.width / image.width, cell.height / image.height), w = image.width * scale, h = image.height * scale
                let x = cell.minX + (cell.width - w) / 2, y = cell.minY + (cell.height - h) / 2
                body += "<p:pic><p:nvPicPr><p:cNvPr id=\"\(i + 2)\" name=\"\(builder.esc(file.lastPathComponent))\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"img\(i + 1)\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"\(builder.emu(x))\" y=\"\(builder.emu(y))\"/><a:ext cx=\"\(builder.emu(w))\" cy=\"\(builder.emu(h))\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>"
                if let note = image.note { warnings.append("\(file.lastPathComponent): \(note).") }
            }
            try builder.add(body: body, black: true, relations: relations, transition: transitionXML)
            pages.append(ToolPage(sourcePage: slide + 1, fontSize: nil, empty: false, warnings: warnings))
        }
        try builder.finish(output: staged)
        // Check the saved package: slide count, every picture byte for byte, transitions.
        let check = try temp(); defer { try? fm.removeItem(at: check) }
        try unpack(staged, check)
        let slides = try orderedParts(check)
        guard slides.count == plan.count else { try fail("The slide count does not match the plan.") }
        for (media, hash) in hashes {
            guard try checksum(Data(contentsOf: check.appendingPathComponent(media))) == hash else { try fail("The picture \(media) was not preserved.") }
        }
        if !transitionXML.isEmpty {
            for slide in slides { guard list(try xml(check, slide).rootElement()!, "transition") != nil else { try fail("A transition is missing on \(slide).") } }
        }
        var limitations = ["Pictures are never cropped or stretched: each one fits whole in its space, centred, on a black background.", "JPEG, PNG and GIF pictures are kept byte for byte; other formats, and photos stored sideways, are converted (JPEG, or PNG with transparency)."]
        if !skipped.isEmpty { limitations.append("Skipped (unreadable): " + skipped.joined(separator: ", ") + ".") }
        let source = inputs.count == 1 ? inputs[0].path : "\(inputs.count) items"
        let r = ToolReport(operation: "Images → PowerPoint (\(layout.title.lowercased()), transition: \(transition.title.lowercased())\(advance.map { ", every \(String(format: "%g", $0)) s" } ?? ""))", source: source, output: output.path, pages: pages, checks: ["\(prepared.count) pictures on \(plan.count) slides, in order", "Pictures verified by SHA-256 after saving", transitionXML.isEmpty ? "No transition" : "Transition on every slide"], limitations: limitations)
        try publish(staged, r); return r
    }
}
