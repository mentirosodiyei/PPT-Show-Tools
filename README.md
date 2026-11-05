# Deckwright

Deckwright is a desktop app for macOS and Windows that works on PowerPoint files without opening PowerPoint. Every operation creates a new file, never changes the original, and checks the result against the source before reporting success.

## Functions

Everything is on one screen: choose a **Function** and follow the controls below it.

| Function | What it does |
|---|---|
| **Merge** | Combines presentations in the order shown. Each one keeps its own masters, themes, animations, transitions, media and embedded fonts. **Merge and save with PowerPoint…** also has Microsoft PowerPoint open and save the result (AppleScript on macOS, COM on Windows), without using your keyboard or mouse. |
| **PDF to PowerPoint** | One slide per PDF page, as an image. Slides use the first page's proportion; text is not editable. |
| **Resize slides** | Changes the slide size set in Settings, scaling the content proportionally and centring it. With *Automatic* it widens the slides to 16:9 and keeps the content at its size. |
| **Extract notes** | One black 16:9 slide per original slide, with its speaker notes as editable white text. Slides without notes show “No Notes on this slide”. |

## Settings: slide size

- **Automatic** (default): if every presentation has the same size or proportion (for example all 4800 × 1344 px), that size is kept, at the largest size. With mixed proportions the output is 16:9 and Deckwright asks before fitting the other presentations without enlarging them.
- **Fixed size**: 16:9 at 1280 × 720, 1920 × 1080 or 3840 × 2160 px, 4:3 at 1024 × 768 px, or a **Custom** size such as 4800 × 1344 px. Every slide is scaled proportionally and centred on that size; content is never stretched or cropped.

Pixels are measured as in PowerPoint, at 96 per inch (1 px = 9525 EMU). PowerPoint allows 96–5376 px per side. On macOS the setting is stored in the app's preferences; on Windows in `%APPDATA%\Deckwright\settings.json`.

## Checks and reports

Each output gets a `.report.json` (and a `.report.txt` for merges): slide order and count, text, animation and timing trees, transitions, notes, SHA-256 of media/fonts/embedded objects, content types, unique IDs and internal relationships. Packages are treated as untrusted input: ZIP paths, symbolic links, duplicate entries and oversized archives are refused.

The checks are structural. They do not certify visual fidelity, playback, fonts or SmartArt/OLE rendering; review the result in PowerPoint.

## Building

**macOS** (Swift/SwiftUI, macOS 14+, universal: Apple Silicon and Intel):

```sh
./build.sh            # → dist/Deckwright.app and dist/Deckwright-<version>.dmg
```

The DMG holds the app and a link to Applications: open it and drag Deckwright onto Applications.

**Windows** (Rust, built on the Mac with `cargo-xwin` and NSIS):

```sh
windows/build-windows.sh   # → dist/windows/Deckwright-Setup-<version>.exe and a portable zip
```

Build caches live in `~/Library/Caches/Deckwright-*`, outside synced folders.

## Command line

Both builds accept the same commands (used by the test suite):

```sh
Deckwright --merge [--size WxH] output.pptx input1.pptx input2.pptx […]
Deckwright --merge-powerpoint [--size WxH] output.pptx input1.pptx input2.pptx […]
Deckwright --resize [--size WxH] output.pptx input.pptx
Deckwright --pdf-to-ppt output.pptx input.pdf
Deckwright --extract-notes output.pptx input.pptx
```

`--size` takes pixels (`4800x1344`) or `auto`; `--fit-non169` answers the 16:9 question with “fit”.

## Tests

```sh
cd tests && python3 -m unittest test_guards test_conversion_tools test_scaler_fit test_office_text test_native_checkpoint test_work_kit_audit test_hardening test_slide_size
```

`DECKWRIGHT_BIN=<path>` runs the same suite against another build, such as the Windows engine compiled for macOS. On macOS all 40 tests pass. The Windows engine passes all except the two PDF tests, which need the Windows PDF renderer.

## Status

The Windows build has not yet been tested on a real Windows PC. The history of the project, from PPT Merger 0.2 to 0.6, is in [docs/HISTORY.ca.md](docs/HISTORY.ca.md) (Catalan).
