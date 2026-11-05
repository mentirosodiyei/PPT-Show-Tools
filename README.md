# Deckwright

Deckwright is a desktop app for macOS and Windows that works on PowerPoint files without opening PowerPoint. Every operation creates a new file, never changes the original, and checks the result against the source before reporting success.

## Functions

Everything is on one screen: choose a **Function** and follow the controls below it.

| Function | What it does |
|---|---|
| **Merge** | Combines presentations in the order shown. Each one keeps its own masters, themes, animations, transitions, media and embedded fonts. **Merge and save with PowerPoint…** also has Microsoft PowerPoint open and save the result (AppleScript on macOS, COM on Windows), without using your keyboard or mouse. |
| **PDF to PowerPoint** | One slide per PDF page, as an image. Slides take the size set in Settings (with *Automatic*, the first page's proportion); text is not editable. |
| **Resize slides** | Changes the slides to the size set in Settings (with *Automatic*, to 16:9) and adapts the layout to it (see *How slides are adapted*). |
| **Extract notes** | One black 16:9 slide per original slide, with its speaker notes as editable white text. Slides without notes show “No Notes on this slide”. |

## Settings: slide size

- **Automatic** (default): if every presentation has the same proportion (for example all 4800 × 1344 px), the largest of their sizes is used. With mixed proportions the largest 16:9 input is used (1920 × 1080 px if none is 16:9).
- **Fixed size**: 16:9 at 1280 × 720, 1920 × 1080 or 3840 × 2160 px, 4:3 at 1024 × 768 px, or a **Custom** size such as 4800 × 1344 px. Typing a custom width or height selects *Custom*. Merge, PDF to PowerPoint and Resize slides all use it.

### How slides are adapted

When a merge combines presentations of different sizes, each one is adapted to the output size on its own before they are joined. Adapting keeps the design instead of scaling the slide as one picture with empty bands:

- Text, fonts, groups, tables and charts scale by the same factor on both axes, so nothing is distorted.
- Text boxes and placeholders follow the new proportion, so titles and body text use the new width or height.
- Shapes and pictures that span the slide (bars, full-bleed photos) keep spanning it; pictures are cropped, not stretched.
- Everything else keeps its size and stays anchored where it was: a logo in a corner stays in that corner, a centred photo stays centred.
- Picture backgrounds fill the new slide and overflow on the sides that no longer fit, without stretching.
- Animations, motion paths and transitions are unchanged (PowerPoint measures them as fractions of the slide).

Pixels are measured as in PowerPoint, at 96 per inch (1 px = 9525 EMU). PowerPoint allows 96–5376 px per side. On macOS the setting is stored in the app's preferences; on Windows in `%APPDATA%\Deckwright\settings.json` (the portable version keeps it only while it is open).

## Checks and reports

Nothing is written next to the files you convert. Reports and error logs go to a logs folder and are deleted after 30 days:

| Version | Reports and logs |
|---|---|
| macOS | `~/Library/Logs/Deckwright` |
| Windows, installed | `%LOCALAPPDATA%\Deckwright\Logs` (removed by the uninstaller) |
| Windows, portable zip | None: it writes no reports, logs or settings file |

**Open report** in the app opens the report of the last conversion. Each report holds slide order and count, text, animation and timing trees, transitions, notes, SHA-256 of media/fonts/embedded objects, content types, unique IDs and internal relationships. Packages are treated as untrusted input: ZIP paths, symbolic links, duplicate entries and oversized archives are refused.

For scripts, `DECKWRIGHT_REPORTS_NEXT_TO_OUTPUT=1` writes `<output>.report.json` beside the output instead, and `DECKWRIGHT_LOG_DIR` changes the logs folder.

The checks are structural. They do not certify visual fidelity, playback, fonts or SmartArt/OLE rendering; review the result in PowerPoint.

## Building

**macOS** (Swift/SwiftUI, macOS 14+), as two separate apps:

```sh
./build.sh   # → dist/Deckwright-<version>-AppleSilicon.dmg (arm64) and dist/Deckwright-<version>-Intel.dmg (x86_64)
```

The Apple Silicon app contains no Intel code, so it never runs through Rosetta, which macOS 28 removes. Intel Macs are supported up to macOS 26. Each DMG holds the app and a link to Applications: open it and drag Deckwright onto Applications.

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
Deckwright --pdf-to-ppt [--size WxH] output.pptx input.pdf
Deckwright --extract-notes output.pptx input.pptx
```

`--size` takes pixels (`4800x1344`) or `auto`. `--fit-non169` is still accepted by older scripts and ignored.

## Tests

```sh
cd tests && python3 -m unittest test_guards test_conversion_tools test_scaler_fit test_office_text test_native_checkpoint test_work_kit_audit test_hardening test_slide_size test_audit_fixes test_adaptive_resize test_reports
```

`DECKWRIGHT_BIN=<path>` runs the same suite against another build, such as the Windows engine compiled for macOS. On macOS all 51 tests pass. The Windows engine passes all except the two PDF tests, which need the Windows PDF renderer. `cargo test` in `windows/` runs the Rust unit tests.

## Status

The Windows build has not yet been tested on a real Windows PC. The history of the project, from PPT Merger 0.2 to 0.6, is in [docs/HISTORY.ca.md](docs/HISTORY.ca.md) (Catalan).
