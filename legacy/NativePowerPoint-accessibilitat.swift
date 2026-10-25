import AppKit
import ApplicationServices
import Foundation

/// Drives the desktop clipboard workflow. Never falls back to a different paste format.
final class NativePowerPoint {
    private var host:AXUIElement!
    private var app:NSRunningApplication!
    private let fm=FileManager.default
    private var intendedWindow:AXUIElement?
    var progress:(String)->Void = {_ in}
    private var currentFile=""
    private var traceURL:URL?
    private func trace(_ message:String) throws {
        guard let url=traceURL else{return}
        let line="\(ISO8601DateFormatter().string(from:Date())) \(message)\n"
        let handle=try FileHandle(forWritingTo:url);defer{try? handle.close()}
        try handle.seekToEnd();try handle.write(contentsOf:Data(line.utf8))
    }
    private func read(_ e:AXUIElement,_ key:String)->CFTypeRef? {var v:CFTypeRef?;guard AXUIElementCopyAttributeValue(e,key as CFString,&v) == .success else{return nil};return v}
    private func string(_ e:AXUIElement,_ key:String)->String {read(e,key) as? String ?? ""}
    private func children(_ e:AXUIElement)->[AXUIElement] {read(e,kAXChildrenAttribute) as? [AXUIElement] ?? []}
    private func find(_ root:AXUIElement,depth:Int=0,_ match:(AXUIElement)->Bool)->AXUIElement? {
        if match(root){return root};if depth>22{return nil}
        for c in children(root) {if let x=find(c,depth:depth+1,match){return x}};return nil
    }
    private func wait(_ label:String,seconds:Double=180,_ probe:()->AXUIElement? ) throws -> AXUIElement {
        let end=Date().addingTimeInterval(seconds)
        while Date()<end {if let e=probe(){return e};Thread.sleep(forTimeInterval:0.2)}
        var diagnostics:[String]=[]
        if let host=host {
            _=find(host) {element in
                let values=[kAXTitleAttribute,kAXValueAttribute,kAXDescriptionAttribute].map{string(element,$0)}
                let text=values.joined(separator:" | ")
                if ["peg","paste","format","origen","source"].contains(where:{text.localizedCaseInsensitiveContains($0)}),diagnostics.count<24 {
                    diagnostics.append("role="+string(element,kAXRoleAttribute)+" title/value/description="+text+" selected="+String(describing:read(element,kAXSelectedAttribute)))
                }
                return false
            }
        }
        throw MergeFailure(message:"\(currentFile): temps excedit: \(label). No es reintenta el pegat per evitar duplicats.\nDiagnòstic de controls:\n"+diagnostics.joined(separator:"\n"))
    }
    private func requireFocusedDocument() throws {
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier,
              let intended=intendedWindow,let focused=read(host,kAXFocusedWindowAttribute),
              CFGetTypeID(focused)==AXUIElementGetTypeID(),CFEqual(focused,intended) else {
            try fail("La finestra activa no és la presentació prevista. Aturat abans d’enviar tecles o enganxar.")
        }
    }
    private func key(_ code:CGKeyCode,_ flags:CGEventFlags=[]) throws {
        try requireFocusedDocument()
        for down in [true,false] {let event=CGEvent(keyboardEventSource:nil,virtualKey:code,keyDown:down);event?.flags=flags;event?.post(tap:.cghidEventTap)}
        Thread.sleep(forTimeInterval:0.15)
    }
    private func click(_ e:AXUIElement,pointerOnly:Bool=false) throws {
        if !pointerOnly && AXUIElementPerformAction(e,kAXPressAction as CFString) == .success {return}
        guard let p=read(e,kAXPositionAttribute),let s=read(e,kAXSizeAttribute),CFGetTypeID(p)==AXValueGetTypeID(),CFGetTypeID(s)==AXValueGetTypeID() else {try fail("No es pot activar el control de PowerPoint.")}
        var point=CGPoint.zero,size=CGSize.zero
        AXValueGetValue(p as! AXValue,.cgPoint,&point);AXValueGetValue(s as! AXValue,.cgSize,&size)
        point.x+=size.width/2;point.y+=size.height/2
        guard size.width>0,size.height>0 else{try fail("Control de PowerPoint no visible.")}
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else{try fail("PowerPoint ha perdut el focus.")}
        CGEvent(mouseEventSource:nil,mouseType:.leftMouseDown,mouseCursorPosition:point,mouseButton:.left)?.post(tap:.cghidEventTap)
        CGEvent(mouseEventSource:nil,mouseType:.leftMouseUp,mouseCursorPosition:point,mouseButton:.left)?.post(tap:.cghidEventTap)
    }
    private func open(_ url:URL) throws -> AXUIElement {
        guard let appURL=NSWorkspace.shared.urlForApplication(withBundleIdentifier:"com.microsoft.Powerpoint") else {try fail("Cal instal·lar Microsoft PowerPoint.")}
        let semaphore=DispatchSemaphore(value:0);var failure:Error?
        DispatchQueue.main.async {
            let configuration=NSWorkspace.OpenConfiguration();configuration.activates=true
            NSWorkspace.shared.open([url],withApplicationAt:appURL,configuration:configuration) {_,error in failure=error;semaphore.signal()}
        }
        guard semaphore.wait(timeout:.now()+120) == .success else{try fail("No s’ha pogut obrir PowerPoint.")}
        if let failure {throw failure}
        guard let running=NSRunningApplication.runningApplications(withBundleIdentifier:"com.microsoft.Powerpoint").first else{try fail("PowerPoint no està disponible.")}
        app=running;host=AXUIElementCreateApplication(running.processIdentifier)
        let window=try wait("obrir \(url.lastPathComponent)") {
            (read(host,kAXWindowsAttribute) as? [AXUIElement] ?? []).first {
                let document=string($0,kAXDocumentAttribute)
                if let d=URL(string:document),d.standardizedFileURL==url.standardizedFileURL{return true}
                return document.isEmpty && string($0,kAXTitleAttribute)==url.deletingPathExtension().lastPathComponent
            }
        }
        AXUIElementPerformAction(window,kAXRaiseAction as CFString);running.activate()
        _=try wait("confirmar la finestra activa: \(url.lastPathComponent)",seconds:15) {
            guard NSWorkspace.shared.frontmostApplication?.processIdentifier==running.processIdentifier,
                  let focused=read(host,kAXFocusedWindowAttribute),CFEqual(focused,window) else{return nil}
            return window
        }
        intendedWindow=window
        try trace("Finestra confirmada: \(url.path)")
        return window
    }
    private func navigation(_ window:AXUIElement) throws -> AXUIElement {
        try wait("panell de miniatures") {find(window) {string($0,kAXIdentifierAttribute)=="PPT.SlideNavigationCollectionView"}}
    }
    private func focusSlides(_ window:AXUIElement,last:Bool=false) throws {
        let nav=try navigation(window)
        guard let section=children(nav).first,let thumbnail=children(section).first else{try fail("No es troben miniatures de diapositives.")}
        try click(thumbnail)
        if last {
            _=try wait("llegir la barra d’estat abans d’enganxar",seconds:15) {slideCount(window) != nil ? window:nil}
            guard let total=slideCount(window),total>0 else{try fail("No es pot llegir la barra d’estat de PowerPoint.")}
            try key(119) // End, verified in PowerPoint 16.114's thumbnail pane.
            _=try wait("seleccionar i confirmar l’última diapositiva",seconds:10) {
                find(window) {element in
                    guard string(element,kAXRoleAttribute)==kAXStaticTextRole else{return false}
                    return [kAXTitleAttribute,kAXValueAttribute,kAXDescriptionAttribute].contains {attribute in
                        NativeSlidePosition.isLast(string(element,attribute),total:total)
                    }
                }
            }
        }
    }
    private func slideCount(_ window:AXUIElement)->Int? {
        // PowerPoint exposes its status text through AXValue on some builds.
        // Thumbnails can be virtualized, so never derive the count from them.
        var count:Int?
        _=find(window) {element in
            guard string(element,kAXRoleAttribute)==kAXStaticTextRole else{return false}
            for attribute in [kAXValueAttribute,kAXTitleAttribute,kAXDescriptionAttribute] {
                if let position=NativeSlidePosition.parse(string(element,attribute)) {
                    count=position.total;return true
                }
            }
            return false
        }
        return count
    }
    private func pasteKeepingSource(_ destination:AXUIElement,before:Int,expected:Int) throws {
        guard slideCount(destination)==before else {try fail("La destinació no té les \(before) diapositives previstes. No s’enganxa res.")}
        try focusSlides(destination,last:true)
        try trace("Última diapositiva confirmada: \(before). Un únic enganxament; total esperat: \(expected).")
        try key(9,.maskCommand)
        _=try wait("recompte de diapositives després d’enganxar",seconds:600) {slideCount(destination)==expected ? destination:nil}
        try trace("Recompte després d’enganxar confirmat: \(expected)")
        func labelMatches(_ element:AXUIElement,_ labels:[String])->Bool {
            [kAXTitleAttribute,kAXValueAttribute,kAXDescriptionAttribute].contains {labels.contains(string(element,$0))}
        }
        let buttonLabels=["Opciones de pegado de diapositivas","Slide Paste Options","Opcions d’enganxament de diapositives"]
        let sourceLabels=["Mantener formato de origen","Keep Source Formatting","Mantén el format d’origen","Mantenir el format d’origen"]
        func sourceOption()->AXUIElement? {find(destination) {labelMatches($0,sourceLabels)}}
        let button=try wait("opcions de pegat",seconds:15) {find(destination) {labelMatches($0,buttonLabels)}}
        try click(button)
        var option:AXUIElement?
        let menuDeadline=Date().addingTimeInterval(2)
        repeat {option=sourceOption();if option != nil {break};Thread.sleep(forTimeInterval:0.1)} while Date()<menuDeadline
        // AXPress may report success without opening this floating PowerPoint menu.
        // Re-read the live control and click its current AX geometry. Never paste again.
        if option == nil {
            guard let liveButton=find(destination,{labelMatches($0,buttonLabels)}) else {
                try fail("Les diapositives ja estan enganxades, però ha desaparegut el botó d’opcions. Format d’origen NO confirmat. No es repeteix el pegat.")
            }
            try click(liveButton,pointerOnly:true)
        }
        let selectedOption=try wait("obrir i reconèixer el menú de format d’origen",seconds:10) {sourceOption()}
        try click(selectedOption)
        _=try wait("confirmar format d’origen",seconds:15) {
            guard let live=sourceOption(),let selected=read(live,kAXSelectedAttribute) as? Bool,selected else{return nil}
            return live
        }
        try trace("Mantenir el format d’origen: selecció confirmada per PowerPoint")
        try key(53);try key(1,.maskCommand)
    }
    private func saved(_ path:URL,slides:Int,timeout:Double=600) throws {
        let end=Date().addingTimeInterval(timeout);var previous:Int64 = -1;var stable=0
        while Date()<end {
            let attributes=try? fm.attributesOfItem(atPath:path.path)
            let size=(attributes?[.size] as? NSNumber)?.int64Value ?? 0
            if size>0 && size==previous {stable+=1}else{stable=0};previous=size
            if stable>=4,let count=try? Self.packageSlideCount(path),count==slides {return}
            Thread.sleep(forTimeInterval:0.5)
        };try fail("PowerPoint no ha desat una còpia completa amb \(slides) diapositives.")
    }
    private static func packageSlideCount(_ url:URL) throws -> Int {
        let process=Process();let pipe=Pipe();process.executableURL=URL(fileURLWithPath:"/usr/bin/unzip");process.arguments=["-p",url.path,"ppt/presentation.xml"];process.standardOutput=pipe;process.standardError=FileHandle.nullDevice
        try process.run();let data=pipe.fileHandleForReading.readDataToEndOfFile();process.waitUntilExit()
        guard process.terminationStatus==0 else{try fail("PowerPoint encara està desant.")}
        let doc=try XMLDocument(data:data);return try doc.nodes(forXPath:"//*[local-name()='sldId']").count
    }
    static func recordError(inputs:[URL],message:String) -> [String] {
        var paths:[String]=[]
        for folder in Set(inputs.map{$0.deletingLastPathComponent()}) {
            let log=folder.appendingPathComponent("PPT-Merger-ERROR-\(UUID().uuidString).log")
            let text="\(ISO8601DateFormatter().string(from:Date()))\nMotor: PowerPoint, porta-retalls natiu\n\(message)\nEntrades:\n"+inputs.map{$0.path}.joined(separator:"\n")
            do {try text.write(to:log,atomically:true,encoding:.utf8);paths.append(log.path)}catch {paths.append("No es pot escriure el registre a \(folder.path): \(error.localizedDescription)")}
        };return paths
    }
    func merge(inputs:[URL],fit:Bool) throws -> URL {
        guard AXIsProcessTrusted() else{try fail("Cal autoritzar PPT Merger a Configuració del Sistema → Privacitat i seguretat → Accessibilitat. El motor natiu necessita controlar només el flux de copiar/enganxar de PowerPoint.")}
        guard inputs.count>=2,let first=inputs.first else{try fail("Calen dues presentacions.")}
        guard inputs.allSatisfy({$0.pathExtension.lowercased()=="pptx"}) else {try fail("Motor natiu experimental: PPTM encara no validat; procés aturat.")}
        let folder=first.deletingLastPathComponent(),id=UUID().uuidString
        traceURL=folder.appendingPathComponent("PPT-Merger-PROCES-"+id+".log")
        try Data().write(to:traceURL!,options:.withoutOverwriting)
        try trace("Inici. Entrades en ordre: "+inputs.map{$0.lastPathComponent}.joined(separator:" → "))
        let staging=folder.appendingPathComponent(".PPT-Merger-treball-"+id)
        progress("Comprovant originals abans de convertir…")
        guard let resources=Bundle.main.resourceURL else{try fail("Falten recursos de comprovació.")}
        let preflight=resources.appendingPathComponent("OfficeAddin/preflight.py")
        guard fm.isReadableFile(atPath:preflight.path) else {
            try fail("Falten recursos de l’app. Tanca PPT Merger, torna a descomprimir el ZIP complet i obre la nova app. No moguis ni eliminis l’app mentre està oberta. Recurs absent: "+preflight.path)
        }
        let pipe=Pipe(),process=Process();process.executableURL=URL(fileURLWithPath:"/usr/bin/python3")
        process.arguments=[preflight.path]+inputs.map{$0.path};process.standardOutput=pipe;process.standardError=pipe
        try process.run();let diagnostic=pipe.fileHandleForReading.readDataToEndOfFile();process.waitUntilExit()
        guard process.terminationStatus==0 else{try fail(String(data:diagnostic,encoding:.utf8) ?? "Comprovació prèvia fallida.")}
        let preparation=try OfficePipeline.prepare(inputs:inputs,destination:staging,fit:fit)
        let prepared=preparation.files.map{URL(fileURLWithPath:$0.path)}
        let partial=folder.appendingPathComponent("PPT-Merger-PARCIAL-"+id+".pptx")
        try fm.copyItem(at:prepared[0],to:partial)
        var destination=try open(partial),count=preparation.files[0].slides
        for i in 1..<prepared.count {
            currentFile=inputs[i].lastPathComponent;progress("Copiant \(i+1)/\(inputs.count): \(currentFile)…")
            let source=try open(prepared[i])
            _=try wait("confirmar el recompte de l’origen",seconds:30) {slideCount(source)==preparation.files[i].slides ? source:nil}
            try trace("Origen \(currentFile): \(preparation.files[i].slides) diapositives confirmades")
            try focusSlides(source);try key(0,.maskCommand)
            let clipboardVersion=NSPasteboard.general.changeCount
            try key(8,.maskCommand)
            _=try wait("confirmar que PowerPoint ha actualitzat el porta-retalls",seconds:120) {
                NSPasteboard.general.changeCount != clipboardVersion ? source:nil
            }
            try requireFocusedDocument()
            try trace("Porta-retalls actualitzat des de la finestra d’origen confirmada")
            destination=try open(partial)
            try pasteKeepingSource(destination,before:count,expected:count+preparation.files[i].slides)
            count+=preparation.files[i].slides;try saved(partial,slides:count)
            progress("Comprovant la importació de \(currentFile) abans de continuar…")
            let checkpoint=try OfficePipeline.verify(inputs:Array(prepared.prefix(i+1)),output:partial)
            let checkpointIssues=checkpoint.flatMap{$0.issues}
            guard checkpointIssues.isEmpty else{try fail("La còpia parcial no coincideix amb les entrades. Aturat abans de continuar: "+checkpointIssues.joined(separator:"; "))}
            try trace("Comprovació estructural superada després de \(currentFile): \(count) diapositives")
            // Close only our temporary source; do not close the user's documents.
            let temporary=try open(prepared[i]);AXUIElementPerformAction(temporary,kAXRaiseAction as CFString);try key(13,.maskCommand)
        }
        progress("Comprovant text, animacions, notes i multimèdia…")
        let rawChecks=try OfficePipeline.verify(inputs:prepared,output:partial)
        let checks=rawChecks.enumerated().map {i,c in FileValidation(file:inputs[i].lastPathComponent,status:c.status,checkedSlides:c.checkedSlides,slidesWithAnimation:c.slidesWithAnimation,binaryResourcesChecked:c.binaryResourcesChecked,checks:c.checks,issues:c.issues,needsReview:c.needsReview)}
        let encoder=JSONEncoder();encoder.outputFormatting=[.prettyPrinted,.sortedKeys]
        try encoder.encode(checks).write(to:URL(fileURLWithPath:partial.path+".report.json"))
        let issues=checks.flatMap{check in check.issues.map{check.file+": "+$0}}
        guard issues.isEmpty else{try fail("La importació NO ha superat la comprovació:\n"+issues.joined(separator:"\n")+"\nCòpia parcial: "+partial.path)}
        // Copy rather than rename a document still open in PowerPoint.
        let output=folder.appendingPathComponent("PPT-Merger-resultat-"+id+".pptx");try fm.copyItem(at:partial,to:output)
        try encoder.encode(checks).write(to:URL(fileURLWithPath:output.path+".report.json"))
        try trace("Resultat verificat: \(output.path)")
        return output
    }
}

struct NativeSlidePosition {
    static func parse(_ label:String) -> (current:Int,total:Int)? {
        let regex=try! NSRegularExpression(pattern:"^(?:Diapositiva|Slide)\\s+([0-9]+)\\s+(?:de|of)\\s+([0-9]+)$",options:.caseInsensitive)
        let text=label.trimmingCharacters(in:.whitespacesAndNewlines)
        guard let match=regex.firstMatch(in:text,range:NSRange(text.startIndex...,in:text)),
              let a=Range(match.range(at:1),in:text),let b=Range(match.range(at:2),in:text),
              let current=Int(text[a]),let total=Int(text[b]),current>0,total>=current else{return nil}
        return (current,total)
    }
    static func isLast(_ label:String,total:Int) -> Bool {
        guard let position=parse(label),total>0 else{return false}
        return position.current==total && position.total==total
    }
}
