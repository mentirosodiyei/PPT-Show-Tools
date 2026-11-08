import AVFoundation
import CoreImage
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Videos are never modified. When a video filled the slide and the new size
/// has another proportion, it keeps its own proportion, centred, and a copy is
/// placed behind it: scaled to cover the whole slide without distortion
/// (cropped only outside the slide), heavily blurred and silent. The copy plays
/// and pauses with the original. macOS makes the copy with AVFoundation and
/// Core Image, so nothing has to be installed. Same rules as windows/src/video.rs.
enum VideoBackdrop {
    static let name = "PPT Show Tools blurred background"
    static let videoRelationship = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/video"
    static let mediaRelationship = "http://schemas.microsoft.com/office/2007/relationships/media"

    static func isVideo(_ shape: XMLElement) -> Bool {
        shape.localName == "pic" && list(shape, "nvPicPr").flatMap { list($0, "nvPr") }.flatMap { list($0, "videoFile") } != nil
    }
    static func shapeID(_ shape: XMLElement) -> String {
        list(shape, "nvPicPr").flatMap { list($0, "cNvPr") }.map { attr($0, "id") } ?? ""
    }

    /// Ids of top-level videos that cover (almost) the whole slide.
    static func spanningVideos(_ tree: XMLElement, width: Double, height: Double) -> [String] {
        (tree.children ?? []).compactMap { $0 as? XMLElement }.filter(isVideo).compactMap { shape in
            guard let ext = list(shape, "spPr").flatMap({ list($0, "xfrm") }).flatMap({ list($0, "ext") }),
                  let cx = Double(attr(ext, "cx")), let cy = Double(attr(ext, "cy")), cx >= 0.9 * width, cy >= 0.9 * height else { return nil }
            return shapeID(shape)
        }
    }

    /// Size of the blurred copy: small, since it is blurred, in the slide's proportion.
    static func size(_ slide: (Double, Double)) -> (Int, Int) {
        let aspect = slide.0 / slide.1
        func even(_ v: Double) -> Int { max(Int((v / 2).rounded()) * 2, 16) }
        var (w, h) = (even(360 * aspect), 360)
        if w > 2560 { w = 2560; h = even(2560 / aspect) }
        return (w, h)
    }
    /// Blur strength: 15 % of the copy's height: no detail is left, only colours and movement.
    static func sigma(_ height: Int) -> Double { 0.15 * Double(height) }

    /// Scales and moves an image so it covers (0, 0, w, h), centred.
    static func cover(_ image: CIImage, _ w: CGFloat, _ h: CGFloat) -> CIImage {
        let e = image.extent, scale = max(w / e.width, h / e.height)
        let dx = (e.width * scale - w) / 2, dy = (e.height * scale - h) / 2
        return image.transformed(by: CGAffineTransform(translationX: -e.minX, y: -e.minY))
            .transformed(by: CGAffineTransform(scaleX: scale, y: scale))
            .transformed(by: CGAffineTransform(translationX: -dx, y: -dy))
    }
    static func blurred(_ image: CIImage, _ size: (Int, Int)) -> CIImage {
        let w = CGFloat(size.0), h = CGFloat(size.1)
        return cover(image, w, h).clampedToExtent().applyingGaussianBlur(sigma: sigma(size.1)).cropped(to: CGRect(x: 0, y: 0, width: w, height: h))
    }

    /// Poster frame scaled to cover `size`, blurred like the video, as PNG.
    static func blurredPoster(_ data: Data, _ size: (Int, Int)) -> Data? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil), let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { return nil }
        let output = blurred(CIImage(cgImage: image), size)
        guard let cg = CIContext().createCGImage(output, from: CGRect(x: 0, y: 0, width: size.0, height: size.1)) else { return nil }
        let result = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(result, UTType.png.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, cg, nil)
        return CGImageDestinationFinalize(destination) ? result as Data : nil
    }

    private final class Outcome: @unchecked Sendable { var ok = false }

    /// Blurred, silent copy of a video in the slide's proportion.
    static func blurredVideo(_ input: URL, _ output: URL, _ size: (Int, Int)) -> Bool {
        let outcome = Outcome(), done = DispatchSemaphore(value: 0)
        // Detached: the command line runs on the main thread, which then waits here.
        Task.detached {
            defer { done.signal() }
            let asset = AVURLAsset(url: input)
            guard let track = try? await asset.loadTracks(withMediaType: .video).first,
                  let (range, transform) = try? await track.load(.timeRange, .preferredTransform) else { return }
            let composition = AVMutableComposition()
            guard let copy = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid),
                  (try? copy.insertTimeRange(range, of: track, at: .zero)) != nil else { return }
            copy.preferredTransform = transform
            let video = AVMutableVideoComposition(asset: composition) { request in
                request.finish(with: blurred(request.sourceImage, size), context: nil)
            }
            video.renderSize = CGSize(width: size.0, height: size.1)
            guard let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetHighestQuality) else { return }
            export.videoComposition = video
            export.outputURL = output
            export.outputFileType = .mp4
            export.shouldOptimizeForNetworkUse = true
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in export.exportAsynchronously { continuation.resume() } }
            outcome.ok = export.status == .completed
        }
        done.wait()
        return outcome.ok && FileManager.default.fileExists(atPath: output.path)
    }

    static func maxID(_ e: XMLElement, _ local: String) -> Int {
        ((try? e.nodes(forXPath: ".//*[local-name() = '\(local)']")) ?? []).compactMap { ($0 as? XMLElement).flatMap { Int(attr($0, "id")) } }.max() ?? 0
    }

    /// Adds a blurred copy behind each video in `ids`; returns warnings for the report.
    static func addBackdrops(package: URL, document: XMLDocument, ids: [String], slide: (Double, Double), part: PicturePadding.PartPad, pad: PicturePadding.Pad) -> [String] {
        ids.compactMap { id in
            switch addBackdrop(package: package, document: document, id: id, slide: slide, part: part, pad: pad) {
            case true?: return nil
            case false?: return "A video now has a blurred still behind it; the moving blurred copy could not be made."
            case nil: return "A video keeps its proportion; no blurred background could be made for it (linked video or unreadable poster frame)."
            }
        }
    }

    /// true: moving copy; false: still; nil: nothing added.
    static func addBackdrop(package: URL, document: XMLDocument, id: String, slide: (Double, Double), part: PicturePadding.PartPad, pad: PicturePadding.Pad) -> Bool? {
        guard let root = document.rootElement(), let tree = (try? root.nodes(forXPath: "//*[local-name() = 'spTree']"))?.first as? XMLElement,
              let original = (tree.children ?? []).compactMap({ $0 as? XMLElement }).first(where: { isVideo($0) && shapeID($0) == id }),
              let nv = list(original, "nvPicPr").flatMap({ list($0, "nvPr") }) else { return nil }
        let nextShape = maxID(root, "cNvPr") + 1
        let mediaNode = (try? nv.nodes(forXPath: ".//*[local-name() = 'media']"))?.first as? XMLElement
        let mediaID = [mediaNode?.attribute(forLocalName: "embed", uri: rNS)?.stringValue, list(nv, "videoFile")?.attribute(forLocalName: "link", uri: rNS)?.stringValue].compactMap { $0 }.first { !$0.isEmpty }
        guard let mediaID, let video = part.target(mediaID),
              let posterID = list(original, "blipFill").flatMap({ list($0, "blip") })?.attribute(forLocalName: "embed", uri: rNS)?.stringValue,
              let poster = part.target(posterID) else { return nil }
        let size = size(slide)
        guard let data = try? Data(contentsOf: package.appendingPathComponent(poster)), let posterPNG = blurredPoster(data, size) else { return nil }
        let stem = UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased().prefix(8)
        let posterName = "ppt/media/pst-blur-\(stem).png", videoName = "ppt/media/pst-blur-\(stem).mp4"
        guard (try? posterPNG.write(to: package.appendingPathComponent(posterName))) != nil else { return nil }
        pad.extensions.insert("png")
        let moving = blurredVideo(package.appendingPathComponent(video), package.appendingPathComponent(videoName), size)
        guard let posterRel = part.addRelationship(pad, PicturePadding.imageRelationship, "/" + posterName) else { return nil }
        var rels: (String, String)? = nil
        if moving {
            pad.extensions.insert("mp4")
            guard let v = part.addRelationship(pad, videoRelationship, "/" + videoName), let m = part.addRelationship(pad, mediaRelationship, "/" + videoName) else { return nil }
            rels = (v, m)
        }
        let copy = original.copy() as! XMLElement
        // Identity: a new id and a name the verifier recognises; no click action.
        if let c = list(copy, "nvPicPr").flatMap({ list($0, "cNvPr") }) {
            set(c, "id", String(nextShape)); set(c, "name", name)
            c.removeAttribute(forName: "descr"); c.removeAttribute(forName: "title"); c.setChildren(nil)
        }
        if let nv = list(copy, "nvPicPr").flatMap({ list($0, "nvPr") }) {
            if let (videoRel, mediaRel) = rels {
                list(nv, "videoFile")?.attribute(forLocalName: "link", uri: rNS)?.stringValue = videoRel
                if let m = (try? nv.nodes(forXPath: ".//*[local-name() = 'media']"))?.first as? XMLElement {
                    m.attribute(forLocalName: "embed", uri: rNS)?.stringValue = mediaRel
                    for child in children(m, "trim") + children(m, "bmkLst") { child.detach() }
                }
            } else {
                nv.setChildren(nil)
            }
        }
        if let blip = list(copy, "blipFill").flatMap({ list($0, "blip") }) {
            blip.attribute(forLocalName: "embed", uri: rNS)?.stringValue = posterRel
            blip.setChildren(nil)
        }
        for style in children(copy, "style") { style.detach() }
        // Frame: the whole slide; only a plain rectangle, no border or effects.
        if let sp = list(copy, "spPr") {
            for child in (sp.children ?? []).compactMap({ $0 as? XMLElement }) where !["xfrm", "prstGeom"].contains(child.localName ?? "") { child.detach() }
            if let t = list(sp, "xfrm") {
                for key in ["rot", "flipH", "flipV"] { t.removeAttribute(forName: key) }
                if let off = list(t, "off") { set(off, "x", "0"); set(off, "y", "0") }
                if let ext = list(t, "ext") { set(ext, "cx", String(Int64(slide.0.rounded()))); set(ext, "cy", String(Int64(slide.1.rounded()))) }
            }
        }
        tree.insertChild(copy, at: original.index)
        if moving, let timing = list(root, "timing") {
            var next = maxID(timing, "cTn")
            follow(timing, original: id, copy: String(nextShape), next: &next)
        }
        return moving
    }

    static func targets(_ e: XMLElement, _ spid: String) -> Bool {
        ((try? e.nodes(forXPath: ".//*[local-name() = 'spTgt']")) ?? []).contains { ($0 as? XMLElement).map { attr($0, "spid") == spid } ?? false }
    }

    /// Every media command and media node of the original also drives the copy (muted).
    static func follow(_ e: XMLElement, original: String, copy: String, next: inout Int) {
        var i = 0
        while i < (e.children ?? []).count {
            guard let child = e.children?[i] as? XMLElement else { i += 1; continue }
            if ["cmd", "video", "audio"].contains(child.localName ?? ""), targets(child, original) {
                let clone = child.copy() as! XMLElement
                func visit(_ x: XMLElement) {
                    if x.localName == "spTgt", attr(x, "spid") == original { set(x, "spid", copy) }
                    if x.localName == "cTn" { next += 1; set(x, "id", String(next)) }
                    if x.localName == "cMediaNode" { set(x, "vol", "0"); set(x, "mute", "1") }
                    for c in x.children ?? [] { if let c = c as? XMLElement { visit(c) } }
                }
                visit(clone)
                e.insertChild(clone, at: i + 1)
                i += 2
            } else {
                follow(child, original: original, copy: copy, next: &next)
                i += 1
            }
        }
    }

    /// The verifier compares a part without the copies this module adds.
    static func withoutBackdrops(_ element: XMLElement) -> XMLElement {
        let marked = ((try? element.nodes(forXPath: ".//*[local-name() = 'cNvPr']")) ?? []).compactMap { $0 as? XMLElement }.filter { attr($0, "name") == name }
        guard !marked.isEmpty else { return element }
        let ids = marked.map { attr($0, "id") }
        let copy = element.copy() as! XMLElement
        func strip(_ e: XMLElement) {
            for child in (e.children ?? []).compactMap({ $0 as? XMLElement }) {
                let local = child.localName ?? ""
                if local == "pic", list(child, "nvPicPr").flatMap({ list($0, "cNvPr") }).map({ attr($0, "name") == name }) == true { child.detach(); continue }
                if ["cmd", "video", "audio"].contains(local), ids.contains(where: { targets(child, $0) }) { child.detach(); continue }
                strip(child)
            }
        }
        strip(copy)
        return copy
    }
}
