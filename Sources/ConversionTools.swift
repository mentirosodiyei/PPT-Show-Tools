import AppKit
import Foundation

struct ToolPage: Codable {
    let sourcePage: Int
    let fontSize: Double?
    let empty: Bool
    let warnings: [String]
}
struct ToolReport: Codable {
    let operation: String
    let source: String
    let output: String
    let pages: [ToolPage]
    let checks: [String]
    let limitations: [String]
}
struct PresentationTools {
    static let fm = FileManager.default
    static func temp() throws -> URL {
        let p=fm.temporaryDirectory.appendingPathComponent("PPTTools-"+UUID().uuidString)
        try fm.createDirectory(at:p,withIntermediateDirectories:true);return p
    }
    static func requireNew(_ input:URL,_ output:URL) throws {
        guard input.resolvingSymlinksInPath() != output.resolvingSymlinksInPath(),
              !fm.fileExists(atPath:output.path),!fm.fileExists(atPath:output.path+".report.json"),!fm.fileExists(atPath:output.path+".report.log") else {
            try fail("La sortida o l’informe ja existeix. Tria un nom nou; l’original no es modifica.")
        }
    }
    static func unpack(_ input:URL,_ root:URL) throws {
        guard ["pptx","pptm"].contains(input.pathExtension.lowercased()) else {try fail("Selecciona un PPTX o PPTM.")}
        try extractPackage(input,to:root)
        _=try orderedParts(root)
    }
    static func emit(_ report:ToolReport) throws {
        let enc=JSONEncoder();enc.outputFormatting=[.prettyPrinted,.sortedKeys]
        try enc.encode(report).write(to:URL(fileURLWithPath:report.output+".report.json"),options:.withoutOverwriting)
        let warnings=report.pages.flatMap{page in page.warnings.map{"Diapositiva \(page.sourcePage): "+$0}}
        if !warnings.isEmpty {
            try warnings.joined(separator:"\n").data(using:.utf8)!.write(to:URL(fileURLWithPath:report.output+".report.log"),options:.withoutOverwriting)
        }
    }
    // The result only reaches its final name after every check has passed.
    // copyItem refuses to replace a file created meanwhile by someone else.
    static func publish(_ staged:URL,_ report:ToolReport) throws {
        let output=URL(fileURLWithPath:report.output)
        try fm.copyItem(at:staged,to:output)
        do {try emit(report)} catch {try? fm.removeItem(at:output);try? fm.removeItem(at:URL(fileURLWithPath:report.output+".report.json"));throw error}
    }
    static func logFailure(input:URL,error:Error) -> String {
        let log=input.deletingLastPathComponent().appendingPathComponent("PPT-Tools-ERROR-"+UUID().uuidString+".log")
        do {try error.localizedDescription.write(to:log,atomically:true,encoding:.utf8);return log.path}
        catch {
            let fallback=fm.temporaryDirectory.appendingPathComponent(log.lastPathComponent)
            try? error.localizedDescription.write(to:fallback,atomically:true,encoding:.utf8)
            return "No s’ha pogut escriure al costat de l’original. Registre alternatiu: "+fallback.path
        }
    }
    static func pdfToPPT(input:URL,output:URL) throws -> ToolReport {
        try requireNew(input,output)
        guard output.pathExtension.lowercased()=="pptx" else {try fail("La sortida del PDF ha de ser PPTX.")}
        guard let document=CGPDFDocument(input as CFURL),document.isUnlocked,document.numberOfPages>0 else {try fail("El PDF no es pot llegir, està bloquejat o no té pàgines.")}
        guard let first=document.page(at:1) else {try fail("No es pot llegir la primera pàgina del PDF.")}
        var firstSize=first.getBoxRect(.cropBox).size
        if abs(first.rotationAngle)%180==90 {firstSize=NSSize(width:firstSize.height,height:firstSize.width)}
        guard firstSize.width>0,firstSize.height>0 else {try fail("Dimensions de pàgina PDF no vàlides.")}
        // Keep the first page's aspect ratio. Different page sizes are fitted without cropping.
        // 960 pt wide by default; tall pages are narrowed to stay within PowerPoint's 4032 pt limit.
        var width=960.0,height=width*firstSize.height/firstSize.width
        if height>4032 {width=width*4032/height;height=4032}
        guard height.isFinite,width>=72,height>=72 else {try fail("Proporció de pàgina PDF no compatible amb PowerPoint (cada costat ha de fer entre 2,54 i 142,24 cm).")}
        let root=try temp(),staging=try temp();defer{try? fm.removeItem(at:root);try? fm.removeItem(at:staging)}
        let staged=staging.appendingPathComponent("result.pptx")
        let builder=try SimpleDeck(root:root,width:width,height:height)
        var pages:[ToolPage]=[],hashes:[String]=[]
        for i in 0..<document.numberOfPages {
            let result: (Data,NSSize) = try autoreleasepool {
                guard let ref=document.page(at:i+1) else {try fail("No es pot llegir la pàgina PDF \(i+1).")}
                var size=ref.getBoxRect(.cropBox).size
                if abs(ref.rotationAngle)%180==90 {size=NSSize(width:size.height,height:size.width)}
                guard size.width>0,size.height>0 else {try fail("Pàgina PDF \(i+1): dimensions no vàlides.")}
                let scale=2560/max(size.width,size.height)
                let w=max(1,Int((size.width*scale).rounded())),h=max(1,Int((size.height*scale).rounded()))
                guard let ctx=CGContext(data:nil,width:w,height:h,bitsPerComponent:8,bytesPerRow:0,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue) else {try fail("No es pot dibuixar la pàgina \(i+1).")}
                ctx.setFillColor(CGColor(gray:1,alpha:1));ctx.fill(CGRect(x:0,y:0,width:w,height:h))
                // CGPDFPage.getDrawingTransform does not upscale on this OS.
                // Apply an explicit crop/rotation transform so the image fills its pixel canvas.
                let crop=ref.getBoxRect(.cropBox)
                let renderingScale=min(Double(w)/size.width,Double(h)/size.height)
                ctx.scaleBy(x:renderingScale,y:renderingScale)
                switch ((ref.rotationAngle % 360)+360)%360 {
                case 90:ctx.translateBy(x:0,y:crop.width);ctx.rotate(by: -.pi/2)
                case 180:ctx.translateBy(x:crop.width,y:crop.height);ctx.rotate(by: .pi)
                case 270:ctx.translateBy(x:crop.height,y:0);ctx.rotate(by: .pi/2)
                default:break
                }
                ctx.translateBy(x: -crop.minX,y: -crop.minY);ctx.clip(to:crop);ctx.drawPDFPage(ref)
                guard let image=ctx.makeImage(),let data=NSBitmapImageRep(cgImage:image).representation(using:.png,properties:[:]) else {try fail("No es pot generar la imatge de la pàgina \(i+1).")}
                return (data,size)
            }
            hashes.append(checksum(result.0))
            try builder.addImage(result.0,size:result.1)
            var warnings:[String]=[]
            if let dictionary=document.page(at:i+1)?.dictionary {
                var annotations:CGPDFArrayRef?
                if CGPDFDictionaryGetArray(dictionary,"Annots",&annotations),let annotations,CGPDFArrayGetCount(annotations)>0 {
                    warnings.append("Aquesta pàgina conté anotacions o camps de formulari. La imatge només representa el contingut base; revisa-la abans d’utilitzar el resultat.")
                }
            }
            pages.append(ToolPage(sourcePage:i+1,fontSize:nil,empty:false,warnings:warnings))
        }
        try builder.finish(output:staged)
        let check=try temp();defer{try? fm.removeItem(at:check)}
        try unpack(staged,check)
        guard try orderedParts(check).count==document.numberOfPages else {try fail("El nombre de diapositives no coincideix amb les pàgines PDF.")}
        for (i,hash) in hashes.enumerated() {
            guard try checksum(Data(contentsOf:check.appendingPathComponent("ppt/media/page\(i+1).png")))==hash else {try fail("La imatge de la pàgina \(i+1) no s’ha conservat.")}
        }
        let r=ToolReport(operation:"PDF → PowerPoint (imatges)",source:input.path,output:output.path,pages:pages,checks:["Una diapositiva per pàgina, en ordre","Imatges PNG verificades per SHA-256","Encaix proporcional sense retallar"],limitations:["Text i elements del PDF no editables; imatges de fins a 2560 píxels al costat llarg.","La proporció de sortida és la de la primera pàgina; les altres s’encaixen amb marges.","No es rasteritzen anotacions ni camps de formulari. No es converteixen enllaços ni multimèdia del PDF; les pàgines amb anotacions generen un avís."])
        try publish(staged,r);return r
    }
    static func notesText(root:URL,slide:String) throws -> String {
        let rel=(slide as NSString).deletingLastPathComponent+"/_rels/"+(slide as NSString).lastPathComponent+".rels"
        guard fm.fileExists(atPath:root.appendingPathComponent(rel).path) else{return ""}
        guard let relation=children(try xml(root,rel).rootElement()!,"Relationship").first(where:{attr($0,"Type").hasSuffix("/notesSlide") && attr($0,"TargetMode") != "External"}) else{return ""}
        let part=resolvePart(owner:slide,target:attr(relation,"Target")),doc=try xml(root,part)
        let shapes=(try doc.nodes(forXPath:"//*[local-name()='sp']")).compactMap{$0 as? XMLElement}.filter{$0.uri == pNS}
        var paragraphs:[String]=[]
        for shape in shapes {
            let ph=(try shape.nodes(forXPath:".//*[local-name()='ph']")).first as? XMLElement
            if let ph,["sldImg","sldNum","hdr","ftr","dt"].contains(attr(ph,"type")) {continue}
            // Keep the body and explicit additional note text; omit calculated placeholders.
            for body in children(shape,"txBody") {
                for p in children(body,"p") {
                    var text=""
                    for child in p.children ?? [] {
                        guard let e=child as? XMLElement else{continue}
                        if e.localName=="br" {text += "\n"}
                        else if ["r","fld"].contains(e.localName ?? "") {
                            for t in children(e,"t") {text += t.stringValue ?? ""}
                        }
                    }
                    paragraphs.append(text)
                }
            }
        }
        return paragraphs.joined(separator:"\n").trimmingCharacters(in:.whitespacesAndNewlines)
    }
    static func fittedSize(_ text:String) -> Double {
        guard !text.isEmpty else{return 0}
        func fits(_ size:Double) -> Bool {
            let style=NSMutableParagraphStyle();style.lineBreakMode = .byWordWrapping
            let attrs:[NSAttributedString.Key:Any]=[.font:NSFont(name:"Arial",size:size) ?? NSFont.systemFont(ofSize:size),.paragraphStyle:style]
            let bounds=(text as NSString).boundingRect(with:NSSize(width:888,height:1000000),options:[.usesLineFragmentOrigin,.usesFontLeading],attributes:attrs)
            return bounds.width<=888.5 && bounds.height<=420 // safety reserve for PowerPoint font metrics
        }
        // DrawingML font sizes must be 1–4000 pt; 0 means the text cannot fit at 1 pt.
        guard fits(1) else{return 0}
        var low=1.0,high=400.0
        for _ in 0..<22 {let mid=(low+high)/2;if fits(mid){low=mid}else{high=mid}}
        return floor(low*100)/100
    }
    static func extractNotes(input:URL,output:URL) throws -> ToolReport {
        try requireNew(input,output)
        guard output.pathExtension.lowercased()=="pptx" else {try fail("El PowerPoint de notes es desa en PPTX.")}
        let source=try temp(),root=try temp(),staging=try temp();defer{try? fm.removeItem(at:source);try? fm.removeItem(at:root);try? fm.removeItem(at:staging)}
        let staged=staging.appendingPathComponent("result.pptx")
        try unpack(input,source);let slides=try orderedParts(source)
        guard !slides.isEmpty else{try fail("La presentació no té diapositives.")}
        let builder=try SimpleDeck(root:root,width:960,height:540)
        var expected:[String]=[],pages:[ToolPage]=[]
        for (i,part) in slides.enumerated() {
            let text=try notesText(root:source,slide:part),size=fittedSize(text)
            if !text.isEmpty && size<1 {try fail("Les notes de la diapositiva \(i+1) són massa llargues per encabir-les en una pàgina sense perdre text.")}
            let displayText=text.isEmpty ? "No Notes on this slide":text
            let displaySize=text.isEmpty ? 24.0:size
            try builder.addNotes(displayText,fontSize:displaySize,centered:text.isEmpty)
            expected.append(displayText)
            pages.append(ToolPage(sourcePage:i+1,fontSize:displaySize,empty:text.isEmpty,warnings:!text.isEmpty && size<18 ? ["Notes molt llargues: mida de lletra \(size) pt. Revisa la llegibilitat."]:[]))
        }
        try builder.finish(output:staged)
        let check=try temp();defer{try? fm.removeItem(at:check)};try unpack(staged,check)
        let actual=try orderedParts(check)
        guard actual.count==slides.count else{try fail("No coincideix el nombre de diapositives de notes.")}
        for (i,part) in actual.enumerated() {
            let doc=try xml(check,part)
            let paras=try doc.nodes(forXPath:"//*[local-name()='txBody']/*[local-name()='p']")
            let text=try paras.map { p in try p.nodes(forXPath:".//*[local-name()='t']").map{$0.stringValue ?? ""}.joined() }.joined(separator:"\n")
            guard text==expected[i] else{try fail("Text de notes perdut o canviat a la diapositiva \(i+1).")}
        }
        let r=ToolReport(operation:"Notes → diapositives negres 16:9",source:input.path,output:output.path,pages:pages,checks:["N originals = N diapositives noves, mateix ordre","Text complet de les notes comparat després de desar","Fons negre, text blanc editable","Mida calculada per pàgina amb reserva de seguretat"],limitations:["Les diapositives sense notes mostren «No Notes on this slide» centrat a 24 pt.","Les notes s’extreuen com a text; no s’importen dibuixos ni el format original de les notes.","PowerPoint pot variar lleugerament les mètriques de fonts; cal revisar visualment el resultat."])
        try publish(staged,r);return r
    }
    static func widescreen(input:URL,output:URL) throws -> ToolReport {
        try requireNew(input,output)
        guard input.pathExtension.lowercased()==output.pathExtension.lowercased() else {try fail("Conserva l’extensió de l’original (PPTX o PPTM).")}
        let root=try temp();defer{try? fm.removeItem(at:root)};try unpack(input,root)
        let before=try PackageSnapshot.capture(root),pres=try xml(root,"ppt/presentation.xml")
        guard let sz=list(pres.rootElement()!,"sldSz"),let w=Int64(attr(sz,"cx")),let h=Int64(attr(sz,"cy")),w>0,h>0 else {try fail("No es poden llegir les dimensions.")}
        // Widen only. Never shrink the source page, even for an unusual aspect ratio.
        let unit=max((w+15)/16,(h+8)/9),target=(unit*16,unit*9)
        guard !SlideScaler.isWidescreen((w,h)) else {try fail("La presentació ja és 16:9. No cal convertir-la.")}
        try requireValidSlideSize(target)
        try SlideScaler.normalize(package:root,presentation:pres,from:(w,h),to:target,fitWithoutEnlarging:true)
        let archive=root.deletingLastPathComponent().appendingPathComponent(UUID().uuidString+".zip");defer{try? fm.removeItem(at:archive)}
        try run("/usr/bin/ditto",["-c","-k","--norsrc","--noextattr",root.path,archive.path])
        let check=try temp();defer{try? fm.removeItem(at:check)}
        try run("/usr/bin/ditto",["-x","-k",archive.path,check.path])
        let checks=try PackageVerifier.verify(sources:[before],inputs:[input],merged:check)
        let issues=checks.flatMap{$0.issues}
        guard issues.isEmpty else {try fail("La conversió no supera la comprovació: "+issues.joined(separator:"; "))}
        // Confirm notes, binary resources, slide order and the explicit 16:9 canvas.
        let outPres=try xml(check,"ppt/presentation.xml")
        guard let outSz=list(outPres.rootElement()!,"sldSz"),attr(outSz,"cx")==String(target.0),attr(outSz,"cy")==String(target.1) else{try fail("No s’ha conservat la mida 16:9 prevista.")}
        let r=ToolReport(operation:"PowerPoint → 16:9 (encaix conservador)",source:input.path,output:output.path,pages:before.slides.indices.map{ToolPage(sourcePage:$0+1,fontSize:nil,empty:false,warnings:[])},checks:["Llenç 16:9, contingut centrat sense ampliar, estirar ni retallar","Ordre, text, notes, animacions, transicions i recursos verificats després de desar"],limitations:["Es mantenen marges laterals quan l’original és 4:3; no es redissenya la composició per omplir-los.","No es pot certificar automàticament tota la reproducció ni l’aspecte d’OLE, SmartArt, fonts i efectes avançats. Revisa el resultat a PowerPoint."])
        try publish(archive,r);return r
    }
}

// Small native OOXML writer for image decks and plain editable notes. No Office dependency.
final class SimpleDeck {
    let root:URL,width:Double,height:Double
    var count=0
    var imagePages=Set<Int>()
    init(root:URL,width:Double,height:Double) throws {
        self.root=root;self.width=width;self.height=height
        for folder in ["_rels","ppt/_rels","ppt/slides/_rels","ppt/media","ppt/slideMasters/_rels","ppt/slideLayouts/_rels","ppt/theme"] {try FileManager.default.createDirectory(at:root.appendingPathComponent(folder),withIntermediateDirectories:true)}
    }
    func put(_ name:String,_ text:String) throws {try Data(text.utf8).write(to:root.appendingPathComponent(name))}
    func esc(_ text:String) -> String {text.replacingOccurrences(of:"&",with:"&amp;").replacingOccurrences(of:"<",with:"&lt;").replacingOccurrences(of:">",with:"&gt;").replacingOccurrences(of:"\"",with:"&quot;")}
    func emu(_ points:Double)->Int64 {Int64((points*12700).rounded())}
    var group:String {"<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"}
    var map:String {"<p:clrMap accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" bg1=\"lt1\" bg2=\"lt2\" folHlink=\"folHlink\" hlink=\"hlink\" tx1=\"dk1\" tx2=\"dk2\"/>"}
    func rel(_ items:[(String,String,String)]) -> String {"<Relationships xmlns=\"\(pkgNS)\">"+items.map{"<Relationship Id=\"\($0.0)\" Type=\"\(rNS)/\($0.1)\" Target=\"\(esc($0.2))\"/>"}.joined()+"</Relationships>"}
    func add(body:String,black:Bool) throws {
        count+=1
        try put("ppt/slides/slide\(count).xml","<p:sld xmlns:p=\"\(pNS)\" xmlns:a=\"\(SlideScaler.drawingNS)\" xmlns:r=\"\(rNS)\"><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"\(black ? "000000":"FFFFFF")\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>\(group)\(body)</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>")
        var relations=[("layout","slideLayout","../slideLayouts/slideLayout1.xml")]
        if imagePages.contains(count){relations.append(("image","image","../media/page\(count).png"))}
        try put("ppt/slides/_rels/slide\(count).xml.rels",rel(relations))
    }
    func addImage(_ data:Data,size:NSSize) throws {
        let n=count+1;imagePages.insert(n)
        try data.write(to:root.appendingPathComponent("ppt/media/page\(n).png"))
        let factor=min(width/size.width,height/size.height),w=size.width*factor,h=size.height*factor
        let body="<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Pàgina \(n)\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"image\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"\(emu((width-w)/2))\" y=\"\(emu((height-h)/2))\"/><a:ext cx=\"\(emu(w))\" cy=\"\(emu(h))\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>"
        try add(body:body,black:false)
    }
    func addNotes(_ text:String,fontSize:Double,centered:Bool=false) throws {
        let paragraphs=text.components(separatedBy:"\n").map {line in "<a:p><a:pPr algn=\"\(centered ? "ctr":"l")\"/><a:r><a:rPr lang=\"ca-ES\" sz=\"\(max(50,Int((fontSize*100).rounded())))\"><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill><a:latin typeface=\"Arial\"/><a:ea typeface=\"Arial\"/><a:cs typeface=\"Arial\"/></a:rPr><a:t xml:space=\"preserve\">\(esc(line))</a:t></a:r><a:endParaRPr sz=\"\(max(50,Int((fontSize*100).rounded())))\"/></a:p>"}.joined()
        let body=text.isEmpty ? "":"<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Notes\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"457200\" y=\"457200\"/><a:ext cx=\"11277600\" cy=\"5943600\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" rIns=\"0\" tIns=\"0\" bIns=\"0\" anchor=\"ctr\"><a:normAutofit/></a:bodyPr><a:lstStyle/>\(paragraphs)</p:txBody></p:sp>"
        try add(body:body,black:true)
    }
    func finish(output:URL) throws {
        guard count>0 else {try fail("No hi ha cap diapositiva per desar.")}
        let a=SlideScaler.drawingNS
        try put("_rels/.rels",rel([("office","officeDocument","ppt/presentation.xml")]))
        let ids=(1...count).map{"<p:sldId id=\"\(255+$0)\" r:id=\"slide\($0)\"/>"}.joined()
        try put("ppt/presentation.xml","<p:presentation xmlns:p=\"\(pNS)\" xmlns:r=\"\(rNS)\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"master\"/></p:sldMasterIdLst><p:sldIdLst>\(ids)</p:sldIdLst><p:sldSz cx=\"\(emu(width))\" cy=\"\(emu(height))\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>")
        try put("ppt/_rels/presentation.xml.rels",rel([("master","slideMaster","slideMasters/slideMaster1.xml")]+(1...count).map{("slide\($0)","slide","slides/slide\($0).xml")}))
        try put("ppt/slideMasters/slideMaster1.xml","<p:sldMaster xmlns:p=\"\(pNS)\" xmlns:a=\"\(a)\" xmlns:r=\"\(rNS)\"><p:cSld><p:spTree>\(group)</p:spTree></p:cSld>\(map)<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"layout\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>")
        try put("ppt/slideMasters/_rels/slideMaster1.xml.rels",rel([("layout","slideLayout","../slideLayouts/slideLayout1.xml"),("theme","theme","../theme/theme1.xml")]))
        try put("ppt/slideLayouts/slideLayout1.xml","<p:sldLayout xmlns:p=\"\(pNS)\" xmlns:a=\"\(a)\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>\(group)</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>")
        try put("ppt/slideLayouts/_rels/slideLayout1.xml.rels",rel([("master","slideMaster","../slideMasters/slideMaster1.xml")]))
        let colors=[("dk1","000000"),("lt1","FFFFFF"),("dk2","222222"),("lt2","EEEEEE"),("accent1","4472C4"),("accent2","ED7D31"),("accent3","A5A5A5"),("accent4","FFC000"),("accent5","5B9BD5"),("accent6","70AD47"),("hlink","0563C1"),("folHlink","954F72")].map{"<a:\($0.0)><a:srgbClr val=\"\($0.1)\"/></a:\($0.0)>"}.joined()
        let fill="<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>"
        let line="<a:ln w=\"12700\">\(fill)<a:prstDash val=\"solid\"/></a:ln>"
        try put("ppt/theme/theme1.xml","<a:theme xmlns:a=\"\(a)\" name=\"PPT Tools\"><a:themeElements><a:clrScheme name=\"Default\">\(colors)</a:clrScheme><a:fontScheme name=\"Arial\"><a:majorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Default\"><a:fillStyleLst>\(String(repeating:fill,count:3))</a:fillStyleLst><a:lnStyleLst>\(String(repeating:line,count:3))</a:lnStyleLst><a:effectStyleLst>\(String(repeating:"<a:effectStyle><a:effectLst/></a:effectStyle>",count:3))</a:effectStyleLst><a:bgFillStyleLst>\(String(repeating:fill,count:3))</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>")
        let overrides=[("ppt/presentation.xml","presentation.main"),("ppt/slideMasters/slideMaster1.xml","slideMaster"),("ppt/slideLayouts/slideLayout1.xml","slideLayout")]+(1...count).map{("ppt/slides/slide\($0).xml","slide")}
        let types=overrides.map{"<Override PartName=\"/\($0.0)\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.\($0.1)+xml\"/>"}.joined()
        try put("[Content_Types].xml","<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\(types)</Types>")
        try run("/usr/bin/ditto",["-c","-k","--norsrc","--noextattr",root.path,output.path])
    }
}
