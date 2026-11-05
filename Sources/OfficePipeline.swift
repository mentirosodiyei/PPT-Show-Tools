import Foundation
struct OfficePreparedFile:Codable {let name:String;let path:String;let slides:Int;let macroEnabled:Bool}
struct OfficePreparation:Codable {let width:Int64;let height:Int64;let files:[OfficePreparedFile]}
struct OfficePipeline {
    static func prepare(inputs:[URL],destination:URL) throws -> OfficePreparation {
        let fm=FileManager.default
        guard inputs.count>=2,!fm.fileExists(atPath:destination.path) else {try fail("Two inputs and a new preparation folder are required.")}
        let temp=fm.temporaryDirectory.appendingPathComponent("Deckwright-Office-"+UUID().uuidString)
        try fm.createDirectory(at:temp,withIntermediateDirectories:true);defer{try? fm.removeItem(at:temp)}
        var roots:[URL]=[],docs:[XMLDocument]=[],sizes:[(Int64,Int64)]=[]
        for (i,input) in inputs.enumerated() {
            guard ["pptx","pptm"].contains(input.pathExtension.lowercased()) else {try fail("Unsupported input format.")}
            let root=temp.appendingPathComponent("s\(i)");try fm.createDirectory(at:root,withIntermediateDirectories:true)
            try extractPackage(input,to:root);let doc=try xml(root,"ppt/presentation.xml")
            guard doc.rootElement()?.uri == pNS,let size=list(doc.rootElement()!,"sldSz"),let w=Int64(attr(size,"cx")),let h=Int64(attr(size,"cy")),(minSlideEMU...maxSlideEMU).contains(w),(minSlideEMU...maxSlideEMU).contains(h) else{try fail("Cannot read the size of \(input.lastPathComponent)")}
            roots.append(root);docs.append(doc);sizes.append((w,h))
        }
        let target=try planSlideSize(sizes,fixed:nil)
        try fm.createDirectory(at:destination,withIntermediateDirectories:true)
        var files:[OfficePreparedFile]=[]
        do {
            for i in inputs.indices {
                try SlideScaler.normalize(package:roots[i],presentation:docs[i],from:sizes[i],to:target)
                let path=destination.appendingPathComponent("source\(i)."+inputs[i].pathExtension.lowercased())
                try run("/usr/bin/ditto",["-c","-k","--norsrc","--noextattr",roots[i].path,path.path])
                files.append(OfficePreparedFile(name:inputs[i].lastPathComponent,path:path.path,slides:try orderedParts(roots[i]).count,macroEnabled:fm.fileExists(atPath:roots[i].appendingPathComponent("ppt/vbaProject.bin").path)))
            }
        } catch {try? fm.removeItem(at:destination);throw error}
        return OfficePreparation(width:target.0,height:target.1,files:files)
    }
    // PowerPoint renames parts and relationship IDs when it imports/saves.
    // Compare ordered slide features and reachable resources, not ZIP paths.
    static func verify(inputs:[URL],output:URL) throws -> [FileValidation] {
        let fm=FileManager.default,temp=fm.temporaryDirectory.appendingPathComponent("Deckwright-OfficeCheck-"+UUID().uuidString)
        try fm.createDirectory(at:temp,withIntermediateDirectories:true);defer{try? fm.removeItem(at:temp)}
        let merged=temp.appendingPathComponent("merged");try fm.createDirectory(at:merged,withIntermediateDirectories:true)
        try extractPackage(output,to:merged);let outputSlides=try orderedParts(merged)
        func related(_ root:URL,_ owner:String,_ suffix:String) throws -> String? {
            let rel=(owner as NSString).deletingLastPathComponent+"/_rels/"+(owner as NSString).lastPathComponent+".rels"
            guard fm.fileExists(atPath:root.appendingPathComponent(rel).path) else{return nil}
            if let r=children(try xml(root,rel).rootElement()!,"Relationship").first(where:{attr($0,"Type").hasSuffix(suffix) && attr($0,"TargetMode") != "External"}) {return resolvePart(owner:owner,target:attr(r,"Target"))};return nil
        }
        // Office can merge/split runs while leaving the paragraph text unchanged.
        // Slide-number placeholders are computed fields, not presenter notes.
        func semanticText(_ document:XMLDocument,notes:Bool=false) throws -> [String] {
            func elements(_ node:XMLNode) -> [XMLElement] {
                var found:[XMLElement]=[]
                if let e=node as? XMLElement {found.append(e)}
                for child in node.children ?? [] {found.append(contentsOf:elements(child))}
                return found
            }
            var result:[String]=[]
            for paragraph in elements(document) where paragraph.localName == "p" {
                if notes {
                    var ancestor=paragraph.parent
                    var computed=false
                    while let node=ancestor {
                        if let shape=node as? XMLElement,shape.localName == "sp",
                           elements(shape).contains(where:{$0.localName == "ph" && attr($0,"type")=="sldNum"}) {computed=true;break}
                        ancestor=node.parent
                    }
                    if computed {continue}
                }
                let text=elements(paragraph).filter{$0.localName == "t"}.map{$0.stringValue ?? ""}.joined()
                if !text.isEmpty {result.append(text)}
            }
            return result
        }
        func textOfNotes(_ root:URL,_ slide:String) throws -> [String] {
            guard let part=try related(root,slide,"/notesSlide") else{return []}
            return try semanticText(xml(root,part),notes:true)
        }
        func media(_ root:URL,_ slides:[String]) throws -> Set<String> {
            var seen=Set<String>(),pending=slides,hashes=Set<String>()
            while let owner=pending.popLast() {
                guard seen.insert(owner).inserted else{continue}
                guard fm.fileExists(atPath:root.appendingPathComponent(owner).path) else{try fail("Broken internal relationship: \(owner)")}
                if owner.lowercased().contains("/media/") || owner.lowercased().contains("/embeddings/") {hashes.insert(try checksumFile(root.appendingPathComponent(owner)))}
                let rel=(owner as NSString).deletingLastPathComponent+"/_rels/"+(owner as NSString).lastPathComponent+".rels"
                if fm.fileExists(atPath:root.appendingPathComponent(rel).path) {
                    for r in children(try xml(root,rel).rootElement()!,"Relationship") where attr(r,"TargetMode") != "External" {
                        // A notes slide points back to its slide; seen handles cycles.
                        pending.append(resolvePart(owner:owner,target:attr(r,"Target")))
                    }
                }
            };return hashes
        }
        var results:[FileValidation]=[],offset=0
        for (i,input) in inputs.enumerated() {
            let source=temp.appendingPathComponent("s\(i)");try fm.createDirectory(at:source,withIntermediateDirectories:true)
            try extractPackage(input,to:source);let originals=try orderedParts(source)
            let imported=Array(outputSlides.dropFirst(offset).prefix(originals.count));var issues:[String]=[],animated=0
            if imported.count != originals.count {issues.append("Wrong slide count.")}
            for j in 0..<min(originals.count,imported.count) {
                let before=try PartFingerprint(xml(source,originals[j]).rootElement()!),after=try PartFingerprint(xml(merged,imported[j]).rootElement()!)
                if before.timing != "ABSENT" {animated+=1}
                if try semanticText(xml(source,originals[j])) != semanticText(xml(merged,imported[j])) {issues.append("Slide \(j+1): text/order differs.")}
                if before.timing != after.timing {issues.append("Slide \(j+1): animation/timing missing or different. PowerPoint may also have reassigned identifiers; review needed.")}
                if before.transition != after.transition {issues.append("Slide \(j+1): transition/advance missing or different.")}
                if try textOfNotes(source,originals[j]) != textOfNotes(merged,imported[j]) {issues.append("Slide \(j+1): notes text differs.")}
                for suffix in ["/slideLayout","/slideMaster"] {
                    var a=try related(source,originals[j],"/slideLayout"),b=try related(merged,imported[j],"/slideLayout")
                    if suffix == "/slideMaster" {a=try a.flatMap{try related(source,$0,suffix)};b=try b.flatMap{try related(merged,$0,suffix)}}
                    if let a,let b {
                        let af=try PartFingerprint(xml(source,a).rootElement()!),bf=try PartFingerprint(xml(merged,b).rootElement()!)
                        if af.timing != bf.timing || af.transition != bf.transition {issues.append("Slide \(j+1): master or layout animation/transition differs.")}
                    } else if a != nil {issues.append("Slide \(j+1): source master/layout missing.")}
                }
            }
            let originalMedia=try media(source,originals),importedMedia=try media(merged,imported)
            if !originalMedia.isSubset(of:importedMedia) {issues.append("Media/embedded objects identical to the originals are missing (or PowerPoint transformed them).")}
            let size=try xml(source,"ppt/presentation.xml"),outSize=try xml(merged,"ppt/presentation.xml")
            if let a=list(size.rootElement()!,"sldSz"),let b=list(outSize.rootElement()!,"sldSz"), (attr(a,"cx") != attr(b,"cx") || attr(a,"cy") != attr(b,"cy")) {issues.append("The output size does not match the prepared copies.")}
            results.append(FileValidation(file:input.lastPathComponent,status:issues.isEmpty ? "structural_checks_passed_needs_review" : "import_failed_validation",checkedSlides:imported.count,slidesWithAnimation:animated,binaryResourcesChecked:originalMedia.count,checks:["Slides in order, text, animations, transitions, notes, size and reachable resources"],issues:issues,needsReview:["Import with PowerPoint source formatting. Appearance, fonts, editing and playback are not certified; macros from added files are not merged.","PowerPoint identifiers can change and cause differences that need review. This is not binary equivalence."]))
            offset+=originals.count
        }
        if offset != outputSlides.count,let last=results.popLast() {results.append(FileValidation(file:last.file,status:"import_failed_validation",checkedSlides:last.checkedSlides,slidesWithAnimation:last.slidesWithAnimation,binaryResourcesChecked:last.binaryResourcesChecked,checks:last.checks,issues:last.issues+["Wrong total count."],needsReview:last.needsReview))}
        return results
    }
}
