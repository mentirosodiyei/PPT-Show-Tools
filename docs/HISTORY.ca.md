# PPT Merger per a Windows 0.6.0 (25/10/2026)

Versió nativa de Windows creada a partir del projecte de macOS. És a `windows/`, en Rust, i es compila des del Mac amb `windows/build-windows.sh`. Els resultats van a `dist/windows/`. Els fitxers intermedis de compilació (més d’1 GB) van a `~/Library/Caches/PPTMerger-windows-target`, fora de Documents, perquè iCloud no els sincronitzi:

- `PPT-Merger-Setup-0.6.0.exe`: instal·lador per usuari, sense permisos d’administrador. Crea accessos directes al menú Inici i, si es vol, a l’escriptori, i inclou desinstal·lador.
- `PPT-Merger-0.6.0-Windows-portable.zip`: el mateix `PPTMerger.exe` sense instal·lar.
- `SHA256SUMS-0.6.0.txt`.

Té les mateixes funcions, textos i informes que la versió de macOS: fusió Open XML, PDF a PowerPoint, conversió a 16:9, extracció de notes i «Fusiona i desa amb PowerPoint». El motor s’ha traduït mòdul a mòdul. El paquet ZIP, el XML, l’escalat, la fusió, el verificador i les notes no depenen de Windows i passen les mateixes proves que l’app de Mac: `PPTMERGER_BIN=~/Library/Caches/PPTMerger-windows-target/release/PPTMerger python3 -m unittest …` (abans, `CARGO_TARGET_DIR=~/Library/Caches/PPTMerger-windows-target cargo build --release` a `windows/`) a `tests/` dona 31 de 33 proves superades, i les dues que fallen són les de PDF, perquè el renderitzador és de Windows. El verificador de l’app de Mac aprova les fusions del motor de Windows, i PowerPoint per a Mac n’obre la fusió i les notes.

Diferències pròpies de Windows:
- **PDF**: es fa servir el renderitzador integrat de Windows (`Windows.Data.Pdf`), sense biblioteques externes. No es detecten anotacions pàgina a pàgina; l’informe avisa que poden no aparèixer.
- **PowerPoint**: «Fusiona i desa amb PowerPoint» utilitza l’automatització COM de PowerPoint per a Windows (escriptori), per PowerShell i sense finestres. PowerPoint obre la fusió de només lectura, en comprova el nombre de diapositives, la desa amb «Desa com a» en PPTX i la tanca. Si PowerPoint no estava obert, el tanca en acabar. El botó queda desactivat si PowerPoint no hi és.
- **Notes**: la mida de la lletra es calcula amb les mètriques reals d’Arial (`C:\Windows\Fonts\arial.ttf`). Pot diferir unes dècimes de punt de la versió de Mac: 91,31 pt en comptes de 91,7 pt en la prova.
- L’executable no necessita el runtime de Visual C++ (CRT estàtic). Requereix Windows 10 o 11 de 64 bits.

**No s’ha provat en un Windows real.** Està comprovat que l’executable i l’instal·lador es generen correctament: PE de 64 bits, interfície gràfica, només DLL del sistema, icona i informació de versió incrustades. La interfície s’ha obert a macOS. Queden pendents de provar a Windows la interfície, el PDF, el desat amb PowerPoint i l’instal·lador. Windows SmartScreen avisarà que l’instal·lador no està signat.

---

# Actualització 0.6.0-test — motor amb PowerPoint per AppleScript (22/10/2026)

S’ha substituït el motor natiu basat en Accessibilitat. Aquell motor simulava el teclat i el ratolí, llegia la barra d’estat i feia servir el botó flotant d’opcions d’enganxament. El nou botó **«Fusiona i desa amb PowerPoint…»** i l’ordre CLI `--merge-powerpoint` funcionen així:

1. Fusionen amb el motor Open XML, que conserva el patró i el tema de cada presentació, i comproven el resultat contra cada original. Si la comprovació falla, no s’envia res a PowerPoint.
2. Per AppleScript (permís d’Automatització, no d’Accessibilitat), Microsoft PowerPoint obre la fusió, en comprova el nombre de diapositives, la desa com a PPTX final i la tanca. No es fa servir el teclat, el ratolí, el porta-retalls ni el focus de finestra, i el motor no depèn de l’idioma ni de la distribució del teclat.
3. El fitxer desat per PowerPoint es compara amb la fusió intermèdia: text, animacions, transicions, notes, patrons, recursos i mida. Hi ha un informe `.report.json`/`.report.txt` amb les dues comprovacions. El fitxer intermedi s’esborra.

**Proves a PowerPoint 16.114 (22/10/2026):** amb AppleScript es poden obrir, comptar, desar i tancar presentacions. En canvi, enganxar per AppleScript aplica sempre el format de destinació: una diapositiva vermella enganxada en una presentació blava queda blava. PowerPoint tampoc no exposa patrons ni dissenys. Per això «Mantenir el format d’origen» no es pot automatitzar sense controlar la interfície. Fusió de prova blava (2 diapositives) + vermella (3): 5 diapositives desades per PowerPoint en 27 s, 2 patrons, totes les comprovacions superades, inclosa una verificació independent contra els originals.

**Accés a carpetes:** PowerPoint és una app aïllada (sandbox). La primera vegada que treballa en una carpeta mostra «Concedeix accés». Cal seleccionar-hi la **carpeta**, no el fitxer, o una carpeta superior com Documents. Si s’hi selecciona el fitxer, el permís només val per a aquell fitxer. El motor envia l’ordre d’obrir una sola vegada i espera fins a 10 minuts: reenviar-la acumulava diàlogs. La primera vegada, macOS també demana permís perquè PPT Merger controli PowerPoint (Automatització).

**Límits:** només desa en PPTX; per a macros, cal la fusió Open XML en PPTM. No executa l’ordre d’enganxar de PowerPoint. També s’ha provat des de la interfície: botó, permís d’Automatització de l’app i finestra de comprovacions (les tres comprovacions superades). El motor antic queda a `legacy/` com a referència i no es compila.

---

# Actualització 0.5.3-test — revisió de seguretat i fiabilitat (22/10/2026)

- **Paquets no fiables**: abans d’extreure qualsevol PPTX/PPTM es comprova que sigui un ZIP vàlid, amb un màxim de 8 GiB descomprimits i 100.000 entrades. Es rebutgen els enllaços simbòlics i les entrades especials: abans la app els seguia en llegir i podia copiar fitxers de fora del paquet al resultat.
- **Sortides preparades a part**: PDF → PowerPoint i Extreu les notes generen el resultat en una carpeta temporal. Només el copien al nom definitiu quan s’han superat totes les comprovacions. Abans, si la comprovació fallava, quedava un PPTX sense verificar al costat de l’original.
- **Límits de PowerPoint**: les mides de diapositiva han de fer entre 1 i 56 polzades per costat. Els PDF molt alts s’estrenyen per no passar de 4032 pt. Els PDF fora de límits, i les fusions o conversions que en sortirien, s’aturen amb un missatge clar.
- **Notes**: la lletra mínima és 1 pt (`sz=100`). Abans es podia escriure `sz=50`, que no és vàlid segons l’esquema. Si el text no hi cap ni a 1 pt, s’atura.
- **Converteix a 16:9**: si la presentació ja és 16:9, s’avisa i no es crea una còpia idèntica.
- **Informes**: tampoc se sobreescriu un `.report.log` que ja existeixi.
- **Fusió**: el diàleg de desar proposa la carpeta del primer original. Hi ha una protecció contra el desbordament dels identificadors de diapositiva.
- **build.sh**: es compila i se signa en una carpeta temporal. Les carpetes sincronitzades (iCloud/File Provider) afegeixen metadades que feien fallar `codesign`. Si l’SDK 26.5 no hi és, s’usa el que hi hagi per defecte. Després de signar, es verifica la signatura.
- **Complement Office.js**: `/config` rebutja les peticions d’altres llocs (`Sec-Fetch-Site`). S’ha corregit l’ordre de la validació de l’índex a `/prepared/`.
- Proves noves a `tests/test_hardening.py`. En total passen 33 proves (`python3 -m unittest` a `tests/`).

---

# Actualització 0.5.1-test

En extreure notes, les diapositives sense text de notes mostren «No Notes on this slide», blanc sobre negre, centrat i a 24 pt. Es manté la correspondència N originals = N noves. El camp empty de l’informe continua indicant que l’original no tenia notes.

---

# PPT Merger 0.5.0-test — conversió i notes (21/10/2026)

S’ha mantingut el motor depurat de Gemini. La pestanya «Fusiona» conserva el flux existent. La pestanya nova «Converteix i extreu» afegeix tres funcions locals, sense obrir PowerPoint per convertir i sense serveis en línia.

- **PDF a PowerPoint**: una imatge PNG per pàgina, en ordre, amb fins a 2560 píxels al costat llarg. Es conserva la proporció de la primera pàgina; pàgines amb una altra proporció s’encaixen centrades sense retallar. Text no editable. Rotació del PDF respectada. Les anotacions i formularis no es rasteritzen; si es detecten anotacions s’avisa a l’informe i es crea un .report.log. PDFs bloquejats s’aturen.
- **Converteix a 16:9**: treballa amb un únic PPTX/PPTM; eixampla el llenç i centra el contingut sense ampliar-lo ni deformar-lo. Un 4:3 conserva marges laterals. Utilitza SlideScaler i el verificador depurats existents. Conserva l’extensió i el paquet original en una còpia. No és un redisseny per omplir tota l’amplada.
- **Extreu les notes**: crea un PPTX nou en 16:9, fons negre i text blanc editable Arial. N diapositives originals = N noves. Les que no tenen notes són negres i buides. S’exclouen els marcadors de número, data, capçalera, peu i miniatura; es conserven cos i text addicional de notes. S’ajusta la lletra per pàgina, amb reserva de seguretat i un avís si queda per sota de 18 pt. Les notes s’extreuen com a text, sense dibuixos ni format ric.

Els diàlegs proposen la carpeta de l’original. No se sobreescriuen fitxers. Cada resultat inclou .report.json; els avisos produeixen .report.log i els errors un PPT-Tools-ERROR-*.log al costat de l’entrada, o ruta alternativa si no s’hi pot escriure.

## Validació d’aquesta versió

- 25 proves Python superades (motor anterior + funcions noves).
- PDF de diverses mides i rotació de 90°: nombre i ordre, dimensions PNG, posició real de píxels, avisos d’anotacions i originals intactes. El test de píxels evita una regressió detectada en què CGPDFPage.getDrawingTransform no ampliava el contingut.
- Notes curtes, llargues, salts de línia, accents, ampersands, omissió del número calculat i pàgina sense notes. Text complet comparat després de desar. Rebuig de sobreescriptura i registre d’errors comprovats.
- Notes d’un LivePremier real: 159 diapositives d’entrada → 159 de sortida (101 sense text de notes). No s’han revisat visualment les 159.
- PowerPoint Microsoft 365 16.114: notes de prova i PDF corregit oberts sense reparar. Inspecció visual de notes curtes/grans, notes molt llargues i pàgina PDF de prova.
- Interfície de les dues pestanyes oberta i inspeccionada. No s’ha recorregut cada diàleg de conversió de principi a fi amb la UI; el mateix motor s’ha provat des de la CLI.

**Límit de les comprovacions:** no hi ha un certificador visual universal. El control estructural no garanteix reproducció, tipografia, SmartArt/OLE ni tots els efectes. En convertir 4:3 s’ha de revisar l’aspecte i la reproducció al programa de destinació. El text de notes pot tenir petites diferències de mètriques entre macOS i PowerPoint; hi ha una reserva d’espai i AutoFit, però cal revisar-lo.

## CLI nova

```sh
"dist/PPT Merger.app/Contents/MacOS/PPTMerger" --pdf-to-ppt sortida.pptx original.pdf
"dist/PPT Merger.app/Contents/MacOS/PPTMerger" --to-169 sortida.pptx original.pptx
"dist/PPT Merger.app/Contents/MacOS/PPTMerger" --extract-notes notes.pptx original.pptm
```

El nom es manté provisionalment pendent de decisió de l’usuari.

---

# Revisió 0.4.6-test — 20/10/2026

El motor Open XML fusiona sense obrir PowerPoint. Corregit el registre de fonts incrustades dels fitxers afegits (abans es copiaven els bytes però no la llista activa) i la pèrdua de salts de línia/tabulacions dins d’atributs XML, detectada al text alternatiu dels Thursday. Onze proves del motor i cinc de l’auditor superades.

Proves reals: onze Thursday → 232 diapositives en 35,71 s; verificador Swift superat i obertura a PowerPoint 16.114 sense reparar. L’auditor complementari exigeix REVIEW: el patró de notes/tema heretat difereix en 174 diapositives. No és una fusió certificada sense pèrdues. Tres LivePremier → 431 diapositives; verificador Swift i auditor reforçat PASS. No s’ha verificat tota la reproducció.

KeepSourceFormatting de Microsoft requereix PowerPoint obert. El complement Office.js continua sense completar el lot gran per l’error de memòria observat; el controlador natiu continua sense validació completa. Aquesta revisió no els declara resolts. Informe de comparació: `validation/alternatives/INVESTIGACIO.md`.

---

# Correccio 0.4.4

El boto flotant d'opcions de pegat pot retornar exit d'AXPress sense obrir el menu. Ara comprova que existeix l'opcio de format d'origen; si no, rellegeix el control viu i prova un clic a la seva geometria AX actual. Mai repeteix enganxar. Reconeix les etiquetes a AXTitle, AXValue i AXDescription. Exigeix l'estat seleccionat de format d'origen. Els temps excedits afegeixen al log els controls rellevants i els seus camps AX. La copia fallida observada tenia 38 diapositives (25+13); no s'ha confirmat el format. Pendent de provar el flux complet del controlador Swift.

# Correccio 0.4.3

El recompte es llegeix de la barra d'estat AXValue/AXTitle/AXDescription i no dels titols de miniatures virtualitzades. Espera fins a 15 segons que la barra estigui disponible. Continua exigint ultima diapositiva N de N abans del pegat. 11 comprovacions del parser i de la proteccio de posicio. Pendent de validar el controlador natiu complet.

# Correccio 0.4.2

Retirat el boto antic Motor PowerPoint que mostrava instruccions Office.js. El motor natiu ara utilitza End al panell de miniatures, comprovat visualment a PowerPoint 16.114 (33 de 33). Abans de cada pegat exigeix que la barra d'estat confirmi ultima diapositiva N de N. Si no pot confirmar-ho en 10 segons, atura i registra; no enganxa en una posicio dubtosa. No s'ha validat encara una fusio nativa completa dels onze Thursday.

# PPT Merger 0.4.0 - Motor natiu experimental

Obre l'app, afegeix els originals i prem **Fusiona amb PowerPoint natiu**. No cal el complement Office.js ni el servidor. Cal autoritzar PPT Merger a Configuracio del Sistema > Privacitat i seguretat > Accessibilitat. Aquest permis permet controlar la interficie del Mac. No utilitzis altres apps, teclat o ratoli mentre treballa.

El motor comprova els originals, prepara copies a la mida 16:9 mes gran, i copia/enganxa les diapositives dins de PowerPoint. Selecciona explicitament Mantener formato de origen i comprova que ha quedat seleccionat. No usa Base64. Espera el recompte i el desament despres de cada presentacio. Si no pot confirmar el format o perd el focus, s'atura sense repetir el pegat.

El resultat i l'informe JSON van al costat del primer original, amb nom unic. La copia de treball es diu PARCIAL fins que passa la comprovacio. Les copies de treball es conserven. Els errors detectats generen .log a cada carpeta d'entrada; si no pot escriure, ho comunica. PPTM encara bloquejat en aquest motor, pendent de validar.

## Validacio real

PowerPoint 16.114 (26101123), Apple Silicon: amb les eines de control de la interficie de la sessio s'han copiat les 32 diapositives de la copia preparada del Deepfake Interview (original de 600 MiB) a un desti d'1 diapositiva. S'ha seleccionat Mantener formato de origen i s'ha desat. Les 33 diapositives passen la comprovacio estructural: text, 29 diapositives amb animacio, transicions/avancament, notes i recursos multimedia accessibles.

**El controlador Swift nou encara no s'ha executat de principi a fi**: necessita el permis d'Accessibilitat propi de PPT Merger. La prova real s'ha fet amb les eines de control de la sessio. No s'ha provat la fusio nativa completa dels 11 Thursday ni dels 3 LivePremier. No es certifica fidelitat visual ni reproduccio total.

El text es compara per paragraf independentment de la divisio en fragments. S'ometen paragrafs buits i el marcador calculat sldNum de les notes. La perdua de text i notes continua fallant, amb prova de regressio.

## Windows

Preparacio i comprovador separats de NativePowerPoint.swift, que cal substituir per un controlador de PowerPoint Windows. Cal adaptar les eines de paquet. No hi ha motor Windows validat en aquest lliurament.

---

## Historic Office.js i Open XML

# PPT Merger 0.3.0 — prototip local Apple Silicon

Per al Mac amb Microsoft 365 PowerPoint 16.114 (26101123). App Swift + SwiftUI, sense Python ni Rosetta per executar-la. Signatura ad hoc local; no notaritzada. El motor de fusió encara és Open XML experimental. **El nou motor PowerPoint és un complement Office.js inclòs a l’app: prem «Motor PowerPoint…». El manifest ja s’ha instal·lat en aquest Mac, però PowerPoint el mostra desactivat: la prova real del motor Microsoft continua bloquejada.** El botó «Fusiona amb Open XML…» continua essent el motor anterior. Consulta `office-addin/README.md` per al flux, requisits i límits.

## Ús

1. Tanca la versió anterior. Descomprimeix el ZIP i obre PPT Merger.app.
2. Afegeix o arrossega PPTX/PPTM i ordena’ls amb Amunt/Avall.
3. Prem Fusiona i tria un nom nou. L’app no sobreescriu fitxers existents.
4. Si hi ha presentacions que no són 16:9, es mostren els noms i les dimensions. Tria **Atura i modifica manualment** o **Encaixa sense ampliar**.
5. En acabar s’obre la finestra **Comprovacions per PowerPoint**. Revisa cada fitxer. Les diferències detectades es mostren en vermell i amb un avís explícit d’importació no correcta.

Els originals són només de lectura; les transformacions es fan en còpies temporals. Si el primer fitxer conté macros, la sortida ha de ser PPTM.

## Mida comuna i encaix

La sortida és exactament 16:9. Es calcula el llenç 16:9 mínim amb una amplada i una alçada almenys iguals a les de totes les entrades. Amb entrades 16:9 equival a la mida de la més gran, amb arrodoniment mínim a unitats EMU. Si una entrada vertical o 4:3 és més alta, el llenç s’amplia també en amplada per mantenir 16:9 i poder-la contenir.

- Entrades 16:9: escala proporcional fins a la mida comuna, centrant si hi ha petites diferències d’arrodoniment. Tolerància de proporció: 0,1%.
- Altres proporcions: només amb elecció explícita de l’usuari. Es conserva la mida física original del contingut, sense ampliar, i es centra amb marges lliures. Els grups es desplacen una sola vegada, sense desplaçar de nou els seus fills.
- L’encaix conté la pàgina original, però no corregeix objectes que ja fossin fora dels marges ni certifica els límits visuals de rotacions, animacions, 3D o extensions de tercers.

La normalització actua sobre coordenades, dimensions, text explícit, marges, taules, línies i efectes DrawingML dels XML de diapositives, patrons, dissenys, temes i gràfics. No rasteritza diapositives, no recodifica imatges/vídeos/àudios i no modifica les durades d’animació o avançament. Notes i documents tenen el seu propi sistema de coordenades. Fonts implícites/heretades, SmartArt, OLE i efectes complexos necessiten revisió visual.

## Comprovacions automàtiques per fitxer

Es captura l’original abans de normalitzar-lo. Després de crear el ZIP final, s’extreu i es comprova el resultat real empaquetat contra aquella captura.

- Ordre i nombre de diapositives, amb correspondència font-sortida.
- Arbres complets de temporització/animació, també en patrons i dissenys. Es detecta tant l’absència com la modificació d’una animació; no es compara només el recompte.
- Transicions i atributs d’avançament automàtic.
- Text, identitat dels objectes i accions interactives.
- XML del contingut de notes.
- Hash SHA-256 de multimèdia, fonts i objectes incrustats.
- Correspondència de les relacions amb recursos: també detecta que un enllaç apunti a un altre recurs existent.
- XML llegible i destins de relacions internes presents.

Hi ha un informe llegible `sortida.pptm.report.txt` i un de detallat `sortida.pptm.report.json`. Cada entrada indica fitxer, diapositives comprovades, diapositives amb arbre d’animació, recursos binaris, comprovacions, incidències i revisió pendent. En cas de diferències, l’app NO mostra el resultat com a correcte: avisa **IMPORTACIÓ NO CORRECTA** i n’indica la font i la diapositiva o recurs. El fitxer es conserva per diagnosticar-lo.

**“Comprovació estructural superada” no vol dir “merge perfecte”.** No es comproven automàticament l’aspecte visual, la geometria final, les substitucions de fonts, la reproducció efectiva d’animacions/àudio/vídeo, l’edició ni la disponibilitat dels enllaços externs. Tampoc es garanteixen seccions, presentacions personalitzades, signatures digitals, IRM o extensions propietàries. Cal revisar el resultat a PowerPoint.

## Límits coneguts

- Només el projecte VBA del primer fitxer queda actiu. Els altres no s’integren; l’informe de cada font amb VBA ho assenyala expressament. Les macros no s’executen durant la fusió.
- Totes les notes comparteixen el primer patró de notes disponible. Es conserva el seu contingut per diapositiva, però el format heretat pot canviar.
- Les configuracions globals, seccions i presentacions personalitzades actives provenen del primer fitxer.
- Es conserven recursos no utilitzats; els projectes VBA secundaris es copien com a recursos inactius. El paquet pot ser més gran del necessari.
- Es renumeren identificadors de patrons/dissenys per evitar col·lisions; es conserven els identificadors dels objectes animats.
- Les extensions de ContentType no distingeixen majúscules i minúscules: `png` també cobreix `image32.PNG`. Noms i bytes originals es preserven.

## Validació de 0.2.0

Vuit tests automatitzats passats, incloent:

- sortida a la mida 16:9 més gran i originals intactes;
- elecció explícita abans d’encaixar 4:3 sense ampliar ni retallar la pàgina original;
- rebuig de sobreescriptura;
- recursos PNG amb extensió en majúscules;
- detecció d’animació eliminada i durada modificada;
- detecció de multimèdia absent, alterada o enllaçada al recurs equivocat.

Els tres LivePremier s’han tornat a fusionar: 159 + 200 + 72 = 431 diapositives. Tots tres han superat el control estructural incorporat. El resultat de 0.2.0 NO ha tingut una nova comprovació visual/reproducció completa a PowerPoint. L’obertura sense reparació que es va comprovar en 16.114 correspon al prototip anterior i no valida tots els canvis posteriors.

## Compilació i CLI

`./build.sh` compila amb Swift i l’SDK macOS 26.5 instal·lat en aquest Mac (l’SDK 27 d’aquesta màquina no disposa de les macros SwiftUI que requereix el seu toolchain). Desplegament macOS 14+, ARM64.

```sh
"dist/PPT Merger.app/Contents/MacOS/PPTMerger" --merge sortida.pptm primera.pptm segona.pptm
"dist/PPT Merger.app/Contents/MacOS/PPTMerger" --merge --fit-non169 sortida.pptm primera.pptm segona.pptm
"dist/PPT Merger.app/Contents/MacOS/PPTMerger" --verify sortida.pptm primera.pptm segona.pptm
python3 tests/test_guards.py
```

La CLI retorna 0 amb control estructural superat, 2 quan detecta diferències d’importació i 1 davant d’un error o una conversió no autoritzada. `--verify` està pensat per als paquets d’aquest motor: PowerPoint pot reorganitzar parts/identificadors en tornar a desar, i això necessita un futur comparador semàntic específic.

## Motor PowerPoint amb KeepSourceFormatting

La integració Office.js 0.3.0 inclou manifest, task pane, servidor HTTPS local, preparació de còpies, importació amb KeepSourceFormatting explícit, comprovació de base i exportació/comparació del resultat. Llegeix `office-addin/README.md`.

Implementat i provat amb host simulat; la instal·lació i validació real dins de PowerPoint 16.114 encara estan pendents. Els PPTM només s’accepten en mode experimental explícit. Les macros dels fitxers afegits no es fusionen. La normalització de mides prèvia continua utilitzant Open XML.

## Actualització 0.3.1: ubicacions i comprovació prèvia

Selecciona els originals amb el nou selector natiu des del complement. La base i el resultat comprovat es desen a la carpeta del primer original, amb noms nous que no sobreescriuen fitxers. El botó Obre la base la passa a PowerPoint sense navegar a un ZIP dins del panell.

Abans de convertir mides, es comproven tots els originals: CRC ZIP, rutes, XML, parts obligatòries, ContentType i relacions internes. Qualsevol incidència detectada atura la preparació. La comprovació final d’importació continua sent obligatòria. Les incidències generen .log en cadascuna de les carpetes d’entrada; si una carpeta no permet escriure, queda una còpia al directori de treball local i es comunica l’error. Sense servidor disponible no es pot garantir escriure un registre.

La revisió dels tres LivePremier originals ha detectat cadascun dos fitxers [trash]/*.dat sense ContentType. Amb el criteri estricte sol·licitat, es bloquegen i es registren: no s’ignoren ni s’esborren automàticament. No es modifica sources/.

Validat amb proves automàtiques del servidor, exportació, logs i JS; pendent de prova completa del nou flux dins de PowerPoint. Cap comprovador disponible certifica tots els errors visuals o de reproducció possibles.
