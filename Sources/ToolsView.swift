import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct ToolsView: View {
    enum Operation:String,CaseIterable {
        case pdf="PDF a PowerPoint",wide="Converteix a 16:9",notes="Extreu les notes"
        var explanation:String {
            switch self {
            case .pdf:return "Una pàgina PDF = una diapositiva amb una imatge. Es conserva la proporció de la primera pàgina; text no editable."
            case .wide:return "Eixampla el llenç a 16:9 i centra el contingut sense estirar-lo, ampliar-lo ni retallar-lo. Conserva els marges laterals de l’original 4:3."
            case .notes:return "N diapositives originals = N diapositives noves de notes en 16:9. Fons negre, text blanc editable i mida adaptada a cada pàgina. Sense notes: «No Notes on this slide» en mida discreta."
            }
        }
    }
    @State private var operation=Operation.pdf
    @State private var source:URL?
    @State private var busy=false
    @State private var report:ToolReport?
    @State private var status="Selecciona una funció i el fitxer original."
    var body:some View {
        VStack(alignment:.leading,spacing:16) {
            Text("Eines de presentacions").font(.largeTitle.bold())
            Picker("Funció",selection:$operation) {ForEach(Operation.allCases,id:\.self){Text($0.rawValue).tag($0)}}.pickerStyle(.segmented).onChange(of:operation){ source=nil;report=nil;status="Selecciona el fitxer original."}
            Text(operation.explanation).font(.callout).fixedSize(horizontal:false,vertical:true).frame(maxWidth:.infinity,minHeight:44,alignment:.topLeading)
            HStack {
                Button("Selecciona el fitxer…",action:choose)
                Text(source?.lastPathComponent ?? "Cap fitxer seleccionat").lineLimit(2).textSelection(.enabled)
            }
            Text("Es crea un fitxer nou. L’original es conserva. Les comprovacions es fan automàticament i es desen en un informe.").foregroundStyle(.secondary).font(.callout)
            Button(operation.rawValue+"…",action:start).disabled(source==nil).keyboardShortcut(.defaultAction)
            if busy {ProgressView("Convertint i comprovant…")}
            Text(status).font(.callout).textSelection(.enabled)
            if let report {
                HStack {
                    Button("Mostra el resultat"){NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath:report.output)])}
                    Button("Obre el resultat"){NSWorkspace.shared.open(URL(fileURLWithPath:report.output))}
                    Button("Obre l’informe"){NSWorkspace.shared.open(URL(fileURLWithPath:report.output+".report.json"))}
                }
                ScrollView {
                    VStack(alignment:.leading,spacing:8) {
                        ForEach(report.checks,id:\.self){Text("✓ "+$0)}
                        ForEach(report.pages.filter{!$0.warnings.isEmpty},id:\.sourcePage){page in Text("Diapositiva \(page.sourcePage): "+page.warnings.joined(separator:" ")).foregroundStyle(.orange)}
                        ForEach(report.limitations,id:\.self){Text($0).font(.caption).foregroundStyle(.secondary)}
                    }.frame(maxWidth:.infinity,alignment:.leading)
                }
            }
            Spacer(minLength:0)
        }.presentationPageLayout().disabled(busy)
    }
    private func choose() {
        let panel=NSOpenPanel();panel.allowedContentTypes=operation == .pdf ? [.pdf]:[UTType(filenameExtension:"pptx")!,UTType(filenameExtension:"pptm")!]
        if panel.runModal() == .OK {source=panel.url;report=nil;status="Preparat per crear una còpia nova."}
    }
    private func start() {
        guard let source else{return}
        let panel=NSSavePanel(),ext=operation == .wide ? source.pathExtension.lowercased():"pptx"
        let suffix=operation == .notes ? "-notes":operation == .wide ? "-16x9":"-presentacio"
        panel.directoryURL=source.deletingLastPathComponent();panel.nameFieldStringValue=source.deletingPathExtension().lastPathComponent+suffix+"."+ext
        panel.allowedContentTypes=[UTType(filenameExtension:ext)!]
        guard panel.runModal() == .OK,let output=panel.url else{return}
        let task=operation;busy=true;report=nil;status="Convertint i comprovant…"
        DispatchQueue.global(qos:.userInitiated).async {
            do {
                let result:ToolReport
                switch task {
                case .pdf:result=try PresentationTools.pdfToPPT(input:source,output:output)
                case .wide:result=try PresentationTools.widescreen(input:source,output:output)
                case .notes:result=try PresentationTools.extractNotes(input:source,output:output)
                }
                DispatchQueue.main.async {report=result;busy=false;status="\(result.pages.count) diapositives creades. Comprovacions superades; revisa l’aspecte i els avisos de l’informe."}
            } catch {
                let log=PresentationTools.logFailure(input:source,error:error)
                DispatchQueue.main.async {busy=false;status="ATURAT: "+error.localizedDescription+"\nRegistre: "+log}
            }
        }
    }
}

// Every main page occupies the available tab area with the same outer margins.
extension View {
    func presentationPageLayout() -> some View {
        self.frame(maxWidth:.infinity,maxHeight:.infinity,alignment:.topLeading).padding(24)
    }
}
