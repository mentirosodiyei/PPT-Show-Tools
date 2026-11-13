# pptx_audit.py

A stand-alone structural check of a merged presentation against its originals, in Python with no third-party packages. It complements the verifier built into the app; it is not a merger.

```sh
python3 tools/pptx_audit.py --inputs first.pptx second.pptx --merged merged.pptx --json audit.json
```

It compares, per original: the number of slides, masters, layouts, themes and notes (counted from the content types, wherever the parts are), each slide's timing, transition and text, the notes master each slide uses, embedded fonts, and the SHA-256 of media, fonts and embedded objects. Unreadable entries or broken internal relationships prevent a pass.

Exit codes: 0 pass, 1 review, 2 fail.

It does not certify appearance, fonts, playback, macros or editing, and counts alone do not prove the order of animations or triggers. Its XML comparison can warn about harmless rewrites. Tests: `tests/test_work_kit_audit.py`.
