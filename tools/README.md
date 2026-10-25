# Auditor complementari del Work Kit

Origen: `PPT_Merger_Work_Kit.zip`, aportat per l’usuari el 20/10/2026.
La còpia original es conserva a `validation/work-kit-original/`.
No s’ha incorporat codi dels repositoris externs citats al document.

Execució des de l’arrel del projecte:

```sh
python3 PPTMerger/tools/pptx_audit.py --inputs sources/*.pptm --merged PPTMerger/validation/LivePremier_workkit_20261020.pptm --json PPTMerger/validation/livepremier-workkit-fresh.json
```

Canvis respecte de l’auditor aportat:
- Recompte de patrons, dissenys, temes i notes segons Content Types, independentment de la ubicació de les parts.
- Hash dels recursos també en subcarpetes importades.
- Entrades il·legibles o relacions d’entrada trencades impedeixen PASS.
- Comprovació per diapositiva del XML del patró de notes realment referenciat. Una diferència exigeix revisió; no certifica la igualtat dels temes heretats.
- Codis de sortida: 0 PASS, 1 REVIEW, 2 FAIL.

Proves a `tests/test_work_kit_audit.py`: parts reubicades, recurs eliminat, animació eliminada, entrada inexistent i canvi de patró de notes.

Aquest auditor complementa el verificador Swift existent; no el substitueix ni és un motor de fusió. Els recomptes no certifiquen ordre, seqüència d’animacions ni triggers. No certifica fonts, aspecte, reproducció, macros, edició ni tots els elements avançats. El motor Open XML encara unifica el patró de notes i conserva les opcions globals i el projecte VBA actiu del primer fitxer. El PASS de LivePremier no demostra que aquests comportaments siguin correctes per a altres presentacions.

Revisió posterior (20/10/2026): compara també temporització, transició i text per diapositiva; una modificació amb el mateix recompte exigeix revisió. Comprova el tema heretat del patró de notes i que les fonts incrustades continuïn registrades a la presentació. Cinc proves de regressió superades. La comparació XML pot generar avisos per reescriptures innocents; no substitueix la revisió visual.
