## Estat actual (20 octubre 2026)

Les notes d’activació de més avall són històriques: posteriorment el complement es va executar en PowerPoint 16.114 i el lot Thursday va arribar a 200 diapositives abans d’un error «Out of memory» en importar l’últim fitxer, d’uns 600 MB. No s’ha completat ni validat el lot de 232 diapositives amb Office.js.

Revisió dels pegats proporcionats: integrat el tractament dels identificadors amb #, el manifest 1.0.0.1 sense FunctionFile i el servidor amb fils, handshake diferit i timeout de 60 segons. Afegit un bloqueig de l’estat compartit: les operacions sobre la feina continuen sent seqüencials. Proves: 6 JavaScript i 13 Python, inclosa una connexió TLS inacabada simultània amb una petició HTTPS vàlida. El certificat de la prova és temporal i no s’instal·la al clauer.

No s’ha aplicat ni executat el nou Setup-Mac.command proporcionat: elimina certificats abans de demanar confirmació i instal·la una CA al clauer del Sistema. Es manté l’instal·lador anterior. No s’han substituït els recursos de l’app compilada ni reiniciat el servidor en ús. Aquests canvis de codi no resolen ni validen l’error de memòria del fitxer gran.

---

# PPT Merger Office.js 0.3.0

Complement local per al PowerPoint de Microsoft 365 al Mac. El codi d’inserció crida realment:

```js
context.presentation.insertSlidesFromBase64(base64, {
  formatting: 'KeepSourceFormatting',
  targetSlideId: lastSlideId + '#'
});
```

**Implementat, però encara no validat dins de PowerPoint 16.114.** Les proves automatitzades utilitzen un host simulat i comproven opcions, ordre, recompte, errors i exportació. No demostren fidelitat del motor Microsoft.

## Instal·lació al Mac

1. Tanca la versió anterior de PPT Merger i obre la 0.3.0. Prem **Motor PowerPoint…** per mostrar els dos scripts inclosos dins de l’app.
2. Executa **Setup-Mac.command** una vegada. Explica els canvis i demana escriure SI abans d’instal·lar un certificat HTTPS de servidor per a `localhost`, limitat a la política SSL d’aquest host, i el manifest a la carpeta `wef` de PowerPoint.
3. Desa el treball obert i reinicia PowerPoint quan et convingui. No es tanca PowerPoint automàticament.
4. Executa **Start-Mac.command** i mantén la finestra oberta. Requereix `/usr/bin/python3` (disponible amb les eines de desenvolupament d’aquest Mac). El servidor només escolta a `127.0.0.1:43187`.
5. Dins de PowerPoint: **Inici → Complements → PPT Merger · Format d’origen**. Accepta el permís per llegir i modificar el document si el PowerPoint el demana. El complement només s’ha de provar en còpies.

La biblioteca Office.js es carrega del CDN oficial de Microsoft. El codi local no envia les presentacions a cap servidor remot. Les càrregues, còpies preparades, exportacions de comprovació i informes es desen a `~/Library/Application Support/PPTMergerOffice/jobs`. Es comproven Host, Origin i un token de sessió per a les operacions; no hi ha una API d’accés arbitrari al sistema de fitxers.

El certificat i la clau es generen individualment a la instal·lació; la clau no s’inclou als ZIP distribuïts. El certificat dura 90 dies. No s’han de saltar avisos de certificat: cal instal·lar-ne la confiança correctament.

## Flux dins del complement

1. Tria les presentacions i ordena-les amb les fletxes.
2. Si n’hi ha que no són 16:9, escull expressament encaixar sense ampliar, o deixa l’opció desmarcada per aturar la preparació i modificar-les manualment.
3. Prepara les còpies a la mida 16:9 més gran. Aquest pas continua utilitzant el normalitzador Open XML experimental; **la importació posterior la fa Microsoft PowerPoint**.
4. Descarrega la **BASE** preparada i obre-la a PowerPoint. És una còpia de la primera presentació amb la mida comuna, no un document en blanc. Torna a obrir el complement dins d’aquesta còpia. La feina continua disponible mentre el servidor està obert.
5. Prem **Importa amb el format d’origen**. Primer s’exporta i comprova el document obert per evitar modificar una destinació equivocada. Després s’importen les presentacions restants, una a una, amb `KeepSourceFormatting` explícit.
6. Es comproven les diapositives noves i que les existents no canviïn d’ordre. Al final, s’exporta una còpia comprimida amb l’API de Microsoft per comparar-la amb les còpies preparades.
7. Desa el resultat amb PowerPoint. Descarrega l’informe JSON del complement. La còpia utilitzada per comprovar-lo també es desa dins de `jobs`.

Si hi ha un error durant la importació, no es reintenta automàticament sobre la còpia parcial: torna a preparar una feina i obre la BASE neta. Si es tanca el servidor, la sessió del complement s’ha de tornar a preparar.

## PPTM i macros

La documentació d’inserció de Microsoft exemplifica PPTX. Els PPTM estan bloquejats per defecte. Es pot activar una **prova experimental amb PPTM**, amb advertiment explícit: s’envien els bytes PPTM a la mateixa API sense canviar-ne silenciosament l’extensió, sense convertir-los a PPTX i sense executar macros des del codi. El host pot rebutjar-los; cal provar-los en PowerPoint 16.114. Mantén les macros deshabilitades durant les proves.

Les macros dels fitxers afegits **no s’integren**. Si la primera BASE és PPTM, conserva el seu projecte, subjecte a com PowerPoint l’obri/desi. No s’ha comprovat que `getFileAsync(Compressed)` conservi VBA. L’informe estructural no certifica macros.

## Comprovacions i límits

El comparador Office.js s’adapta a la renumeració de parts del paquet per PowerPoint: compara diapositives per ordre, text, arbres d’animació/temporització, transicions, avançaments, text de notes, recursos multimèdia/objectes incrustats accessibles i dimensions. També comprova animacions/transicions del disseny i patró.

Les diferències es notifiquen, sense presentar-les com a importació correcta. PowerPoint pot canviar identificadors interns de l’animació o transformar un recurs legítimament; això pot produir una diferència que necessita revisió, no una prova automàtica de pèrdua. No es validen l’aspecte visual, la reproducció, les fonts, tots els atributs de format de notes, totes les propietats de forma, l’edició ni els enllaços externs. `KeepSourceFormatting` no és una garantia de fidelitat total.

La destinació del motor Open XML i la del motor Office.js estan separades: el botó **Fusiona amb Open XML…** de l’app continua utilitzant el motor anterior; **Motor PowerPoint…** mostra la instal·lació del complement.

## Windows

`web/` i el manifest són reutilitzables en Windows. Cal adaptar la instal·lació del certificat, el catàleg de complements i l’executable local de preparació/comprovació; el binari Swift ARM64 d’aquest paquet és només macOS. No s’ha validat cap desplegament Windows.

## Fonts oficials

- Inserció i KeepSourceFormatting: https://learn.microsoft.com/en-us/office/dev/add-ins/powerpoint/insert-slides-into-presentation
- Càrrega lateral al Mac: https://learn.microsoft.com/en-us/office/dev/add-ins/testing/sideload-an-office-add-in-on-mac
- Exportació de documents: https://learn.microsoft.com/en-us/office/dev/add-ins/develop/get-the-whole-document-from-an-add-in-for-powerpoint-or-word


## Resultat de la instal·lació en aquest Mac (19/10/2026)

El certificat s’ha instal·lat amb autorització de l’usuari i macOS retorna `Cert Verify Result: No error.`. El manifest s’ha copiat amb el Finder a la carpeta oficial `wef` perquè macOS en bloquejava la còpia des del procés d’instal·lació. El servidor local s’ha iniciat.

PowerPoint 16.114 reconeix «PPT Merger · Format d’origen» com a complement de desenvolupador, però l’ha mostrat desactivat i no s’ha aconseguit executar el panell. S’han provat un manifest mínim amb comprovació d’API en temps d’execució i un botó de cinta ShowTaskpane. No s’ha executat encara cap crida real `insertSlidesFromBase64` en aquest PowerPoint. La integració està implementada, però NO es considera operativa/validada en el host fins que s’hagi resolt aquesta activació.

El navegador integrat també ha rebutjat el certificat local, tot i la verificació correcta del clauer del Mac; aquesta observació no identifica per si sola la causa del bloqueig de PowerPoint. No s’ha saltat cap avís de certificat ni s’ha desactivat cap protecció.

El manifest comprova PowerPointApi 1.2 des del codi abans d’importar, de manera que pot mostrar un diagnòstic en lloc de dependre només del filtre de requisits del manifest. La seva compatibilitat real continua pendent de mesurar al host.

### Diagnòstic addicional

El validador oficial `office-addin-manifest` ha detectat que la versió inicial 0.3.0.2 era inferior al mínim 1.0. S’ha corregit a **1.0.0.0** (versió del manifest, independent de l’app 0.3.0) i ara passa la validació oficial. Reinstal·lat amb Finder i reiniciat PowerPoint, el complement continua desactivat. Les experiències connectades s’han comprovat activades a la interfície sense canviar-les. La causa pendent no s’atribueix al número de versió.

macOS bloqueja activar `CEFRuntimeLoggingFile` des de l’agent, inclús amb elevació. Per obtenir el diagnòstic, es pot executar des del Terminal propi de l’usuari: `defaults write com.microsoft.Powerpoint CEFRuntimeLoggingFile -string PPTMerger-runtime.txt`, reiniciar PowerPoint i consultar `~/Library/Containers/com.microsoft.Powerpoint/Data/PPTMerger-runtime.txt`. Després: `defaults delete com.microsoft.Powerpoint CEFRuntimeLoggingFile`. No cal donar accés complet al disc per aquest procediment.

## Actualització 0.3.1: ubicacions i comprovació prèvia

Selecciona els originals amb el nou selector natiu des del complement. La base i el resultat comprovat es desen a la carpeta del primer original, amb noms nous que no sobreescriuen fitxers. El botó Obre la base la passa a PowerPoint sense navegar a un ZIP dins del panell.

Abans de convertir mides, es comproven tots els originals: CRC ZIP, rutes, XML, parts obligatòries, ContentType i relacions internes. Qualsevol incidència detectada atura la preparació. La comprovació final d’importació continua sent obligatòria. Les incidències generen .log en cadascuna de les carpetes d’entrada; si una carpeta no permet escriure, queda una còpia al directori de treball local i es comunica l’error. Sense servidor disponible no es pot garantir escriure un registre.

La revisió dels tres LivePremier originals ha detectat cadascun dos fitxers [trash]/*.dat sense ContentType. Amb el criteri estricte sol·licitat, es bloquegen i es registren: no s’ignoren ni s’esborren automàticament. No es modifica sources/.

Validat amb proves automàtiques del servidor, exportació, logs i JS; pendent de prova completa del nou flux dins de PowerPoint. Cap comprovador disponible certifica tots els errors visuals o de reproducció possibles.

## Correcció 0.3.2

La comprovació ContentType compara les extensions sense distingir majúscules i minúscules. Corregit el fals positiu image32.PNG quan hi ha Default png. Els 11 Thursday passen ara la comprovació prèvia (no equival a validar la importació). Un error retornat pel servidor mostra el registre existent en lloc de crear-ne un duplicat.
