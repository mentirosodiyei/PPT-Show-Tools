import SwiftUI
import AppKit
import UniformTypeIdentifiers

@main
struct PPTMergerApp: App {
    init() {
        if CommandLine.arguments.count > 1 {
            do {
                let args=CommandLine.arguments.filter{$0 != "--fit-non169"}
                if args.count == 2, args[1] == "--office-select" {
                    _ = NSApplication.shared
                    NSApp.setActivationPolicy(.accessory)
                    NSApp.activate(ignoringOtherApps:true)
                    let panel=NSOpenPanel();panel.allowsMultipleSelection=true
                    panel.allowedContentTypes=[UTType(filenameExtension:"pptx")!,UTType(filenameExtension:"pptm")!]
                    let paths=panel.runModal() == .OK ? panel.urls.map{$0.path} : []
                    print(String(data:try JSONEncoder().encode(paths),encoding:.utf8)!);exit(0)
                }
                if args.count >= 4, ["--office-prepare","--office-verify"].contains(args[1]) {
                    let encoder=JSONEncoder();encoder.outputFormatting=[.prettyPrinted,.sortedKeys]
                    let inputs=args.dropFirst(3).map{URL(fileURLWithPath:$0)}
                    if args[1] == "--office-prepare" {
                        let prepared=try OfficePipeline.prepare(inputs:inputs,destination:URL(fileURLWithPath:args[2]),fit:CommandLine.arguments.contains("--fit-non169"))
                        print(String(data:try encoder.encode(prepared),encoding:.utf8)!)
                    } else {
                        let checks=try OfficePipeline.verify(inputs:inputs,output:URL(fileURLWithPath:args[2]))
                        print(String(data:try encoder.encode(checks),encoding:.utf8)!);exit(checks.allSatisfy{$0.issues.isEmpty} ? 0 : 2)
                    }
                    exit(0)
                }
                if args.count >= 5, args[1] == "--verify" {
                    let checks=try PackageVerifier.inspect(inputs:args.dropFirst(3).map{URL(fileURLWithPath:$0)},output:URL(fileURLWithPath:args[2]))
                    let encoder=JSONEncoder();encoder.outputFormatting=[.prettyPrinted,.sortedKeys];print(String(data:try encoder.encode(checks),encoding:.utf8)!);exit(checks.allSatisfy{$0.issues.isEmpty} ? 0 : 2)
                }
                if args.count == 4, ["--pdf-to-ppt","--to-169","--extract-notes"].contains(args[1]) {
                    let input=URL(fileURLWithPath:args[3]),output=URL(fileURLWithPath:args[2])
                    do {
                        let report:ToolReport
                        switch args[1] {
                        case "--pdf-to-ppt":report=try PresentationTools.pdfToPPT(input:input,output:output)
                        case "--to-169":report=try PresentationTools.widescreen(input:input,output:output)
                        default:report=try PresentationTools.extractNotes(input:input,output:output)
                        }
                        let encoder=JSONEncoder();encoder.outputFormatting=[.prettyPrinted,.sortedKeys]
                        print(String(data:try encoder.encode(report),encoding:.utf8)!);exit(0)
                    } catch {
                        let log=PresentationTools.logFailure(input:input,error:error)
                        fputs(error.localizedDescription+"\nRegistre: "+log+"\n",stderr);exit(1)
                    }
                }
                if args.count >= 5, args[1] == "--merge-powerpoint" {
                    let report=try PowerPointSave.merge(inputs:args.dropFirst(3).map{URL(fileURLWithPath:$0)},output:URL(fileURLWithPath:args[2]),fitNonWidescreen:CommandLine.arguments.contains("--fit-non169")) {fputs($0+"\n",stderr)}
                    print("\(report.powerPointSlides) diapositives desades per PowerPoint. Informe: \(args[2]).report.json"); exit(report.passed ? 0 : 2)
                }
                guard args.count >= 5, args[1] == "--merge" else { throw MergeFailure(message:"Ús: PPTMerger --merge sortida.pptm entrada1.pptm entrada2.pptm […]\n     PPTMerger --merge-powerpoint sortida.pptx entrada1.pptx entrada2.pptx […]") }
                let report=try OpenXMLMerger().merge(inputs:args.dropFirst(3).map{URL(fileURLWithPath:$0)},output:URL(fileURLWithPath:args[2]),fitNonWidescreen:CommandLine.arguments.contains("--fit-non169"))
                print("\(report.totalSlides) diapositives. Informe: \(args[2]).report.json"); exit(report.structuralValidationPassed ? 0 : 2)
            } catch { fputs(error.localizedDescription+"\n",stderr); exit(1) }
        }
    }
    var body: some Scene { WindowGroup { TabView { MergerView().tabItem {Text("Fusiona")}; ToolsView().tabItem {Text("Converteix i extreu")} } }.defaultSize(width:850,height:650) }
}
struct MergerView: View {
    @State private var files:[URL]=[]
    @State private var selected:URL?
    @State private var busy=false
    @State private var status="Afegeix els PowerPoint en l’ordre que vulguis fusionar-los."
    @State private var result:URL?
    @State private var lastReport:MergeReport?
    @State private var showChecks=false
    @State private var nativeValidation:[FileValidation]=[]
    var body: some View {
        VStack(alignment:.leading,spacing:16) {
            Text("PPT Merger").font(.largeTitle.bold())
            Text("Apple Silicon · Fusió directa Open XML (sense obrir PowerPoint)").foregroundStyle(.secondary)
            Text("La mida 16:9 de sortida s’adapta a la més gran de les entrades. Els altres formats es poden encaixar centrats sense ampliar ni retallar ni modificar-ne el contingut. Es conserven els màsters, dissenys, temes, patrons de notes, animacions, transicions, multimèdia i fonts incrustades de cada presentació.").font(.callout)
            List(selection:$selected) {
                ForEach(Array(files.enumerated()),id:\.element) { i,file in
                    Text("\(i+1). \(file.lastPathComponent)").tag(file)
                }
            }.onDrop(of:[UTType.fileURL],isTargeted:nil) { providers in
                // Preserve provider order even if file delivery is asynchronous.
                Task { @MainActor in
                    var urls:[URL]=[]
                    for provider in providers {
                        let data:Data? = await withCheckedContinuation { continuation in
                            _ = provider.loadDataRepresentation(forTypeIdentifier:UTType.fileURL.identifier) { d,_ in continuation.resume(returning:d) }
                        }
                        if let data, let url=URL(dataRepresentation:data,relativeTo:nil) { urls.append(url) }
                    }
                    add(urls)
                }; return true
            }
            HStack {
                Button("Afegeix…") { let p=NSOpenPanel(); p.allowsMultipleSelection=true; p.allowedContentTypes=[UTType(filenameExtension:"pptx")!,UTType(filenameExtension:"pptm")!]; if p.runModal() == .OK {add(p.urls)} }
                Button("Amunt") {move(-1)}.disabled(selected == nil)
                Button("Avall") {move(1)}.disabled(selected == nil)
                Button("Elimina") {files.removeAll{$0 == selected}; selected=nil}.disabled(selected == nil)
                Spacer()
                Button("Fusiona i desa amb PowerPoint…"){merge(powerPoint:true)}.disabled(files.count<2).help("Fusiona amb Open XML i fa que Microsoft PowerPoint obri i desi el resultat. Cal permís d’Automatització; no controla el teclat ni el ratolí.")
                Button("Fusiona presentacions…"){merge(powerPoint:false)}.disabled(files.count<2).keyboardShortcut(.defaultAction)
            }
            if busy {ProgressView("Fusionant…")}
            Text(status).font(.callout).textSelection(.enabled)
            if let result {
                HStack {
                    Button("Mostra el resultat") {NSWorkspace.shared.activateFileViewerSelecting([result])}
                    Button("Obre amb PowerPoint") {openPowerPoint(result)}
                    Button("Comprovacions per fitxer") {showChecks=true}
                    Button("Obre l’informe") {NSWorkspace.shared.open(URL(fileURLWithPath:result.path+".report.txt"))}
                }
            }
        }.presentationPageLayout().disabled(busy).sheet(isPresented:$showChecks) {
            VStack(alignment:.leading,spacing:12) {
                Text("Comprovacions per PowerPoint").font(.title2.bold())
                Text("El control és estructural. No certifica la fidelitat visual ni la reproducció a PowerPoint.").foregroundStyle(.secondary)
                ScrollView {
                    VStack(alignment:.leading,spacing:18) {
                        ForEach(Array((lastReport?.validation ?? nativeValidation).enumerated()),id:\.offset) { _,check in
                            VStack(alignment:.leading,spacing:6) {
                                Text(check.file).font(.headline).textSelection(.enabled)
                                Text(check.issues.isEmpty ? "Comprovació estructural superada · revisió pendent" : "IMPORTACIÓ NO CORRECTA").foregroundStyle(check.issues.isEmpty ? Color.primary : Color.red)
                                Text("\(check.checkedSlides) diapositives · \(check.slidesWithAnimation) amb animació · \(check.binaryResourcesChecked) recursos verificats")
                                ForEach(check.issues,id:\.self) {Text($0).foregroundStyle(.red)}
                                ForEach(check.needsReview,id:\.self) {Text($0).font(.caption)}
                            }
                            Divider()
                        }
                    }
                }
                Button("Tanca") {showChecks=false}
            }.padding(24).frame(width:700,height:520)
        }
    }
    private func add(_ urls:[URL]) {for url in urls where ["pptx","pptm"].contains(url.pathExtension.lowercased()) && !files.contains(url) {files.append(url)}}
    private func move(_ delta:Int) {guard let selected, let i=files.firstIndex(of:selected), files.indices.contains(i+delta) else{return}; files.swapAt(i,i+delta)}
    private func openPowerPoint(_ url:URL) {
        guard let app=NSWorkspace.shared.urlForApplication(withBundleIdentifier:"com.microsoft.Powerpoint") else {status="No s’ha trobat Microsoft PowerPoint.";return}
        NSWorkspace.shared.open([url],withApplicationAt:app,configuration:NSWorkspace.OpenConfiguration()) {_,error in
            if let error {Task { @MainActor in status=error.localizedDescription}}
        }
    }
    private func merge(powerPoint:Bool) {
        let isMacro = !powerPoint && files.first?.pathExtension.lowercased() == "pptm"
        let ext = isMacro ? "pptm" : "pptx"
        let p = NSSavePanel()
        p.directoryURL = files.first?.deletingLastPathComponent()
        p.nameFieldStringValue = "Presentacio_fusionada.\(ext)"
        p.allowedContentTypes = isMacro
            ? [UTType(filenameExtension: "pptm")!, UTType(filenameExtension: "pptx")!]
            : [UTType(filenameExtension: "pptx")!, UTType(filenameExtension: "pptm")!]
        if powerPoint { p.allowedContentTypes = [UTType(filenameExtension: "pptx")!] }
        guard p.runModal() == .OK, let output = p.url else { return }
        performMerge(inputs: files, output: output, fit: false, powerPoint: powerPoint)
    }
    private func performMerge(inputs:[URL],output:URL,fit:Bool,powerPoint:Bool) {
        busy=true; result=nil; lastReport=nil; nativeValidation=[]
        DispatchQueue.global(qos:.userInitiated).async {
            do {
                if powerPoint {
                    let report=try PowerPointSave.merge(inputs:inputs,output:output,fitNonWidescreen:fit) {message in DispatchQueue.main.async{status=message}}
                    DispatchQueue.main.async {
                        result=output;nativeValidation=report.validation;busy=false;showChecks=true
                        status=report.passed ? "\(report.powerPointSlides) diapositives desades per PowerPoint. Comprovació estructural superada; revisió visual i reproducció pendents." : "Importació NO correcta després de desar amb PowerPoint. Consulta les comprovacions abans d’utilitzar el resultat."
                        if !report.passed {
                            let alert=NSAlert();alert.alertStyle = .warning;alert.messageText="La importació no és correcta"
                            alert.informativeText=report.validation.filter{!$0.issues.isEmpty}.map{$0.file+":\n"+$0.issues.prefix(5).joined(separator:"\n")}.joined(separator:"\n\n")
                            alert.runModal()
                        }
                    }
                    return
                }
                let report=try OpenXMLMerger().merge(inputs:inputs,output:output,fitNonWidescreen:fit)
                DispatchQueue.main.async {
                    result=output;lastReport=report;busy=false;showChecks=true
                    status=report.structuralValidationPassed ? "\(report.totalSlides) diapositives. Comprovació estructural superada; revisió visual i reproducció pendents." : "Importació NO correcta: s’han detectat diferències. Consulta les comprovacions per fitxer abans d’utilitzar el resultat."
                    if !report.structuralValidationPassed {
                        let alert=NSAlert();alert.alertStyle = .warning;alert.messageText="La importació no és correcta"
                        alert.informativeText=report.validation.filter{!$0.issues.isEmpty}.map{$0.file+":\n"+$0.issues.prefix(5).joined(separator:"\n")}.joined(separator:"\n\n")
                        alert.runModal()
                    }
                }
            } catch let choice as FormatChoiceRequired {
                DispatchQueue.main.async {
                    busy=false
                    let alert=NSAlert();alert.messageText="Hi ha presentacions que no són 16:9";alert.informativeText=choice.message
                    alert.addButton(withTitle:"Atura i modifica manualment");alert.addButton(withTitle:"Encaixa sense ampliar")
                    if alert.runModal() == .alertSecondButtonReturn {performMerge(inputs:inputs,output:output,fit:true,powerPoint:powerPoint)}
                    else {status="Fusió aturada. Modifica els fitxers indicats a PowerPoint i torna-ho a provar. Els originals estan intactes."}
                }
            } catch {DispatchQueue.main.async {status=error.localizedDescription;busy=false}}
        }
    }
}
