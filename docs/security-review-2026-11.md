# Security review — November 2026 (PPT Show Tools 0.9.0)

Scope: all of the macOS app (Swift), the Windows app (Rust), the build scripts and the Windows installer. Each finding was reproduced or confirmed in the code before it was fixed. Regression tests are in `tests/test_security.py`.

## Threat model

The files a user opens (PPTX, PPTM, PDF) may come from anyone. Opening one must never:

- run code
- write or read outside the app's own temporary folder and the output the user chose
- crash the app or exhaust the computer
- leave a result that carries hidden content the user did not expect

## Findings and fixes

| # | Severity | Platform | Finding | Fix |
|---|---|---|---|---|
| 1 | High | Windows | Programs (`powershell.exe`, `reg.exe`, `explorer.exe`) were started by name. Windows looks first in the app's own folder, so a file of the same name planted next to the portable app (for example in Downloads) would run. | Started by full path from the Windows and System32 folders. |
| 2 | High | Windows | DLLs that load at run time (graphics) were also searched for in the app's folder: DLL planting. | `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` at start-up. |
| 3 | Medium | Windows | Advisories in the `zip` library (parser differentials, "zip cloaking") and in `quick-xml` (quadratic time with duplicate attributes; unbounded namespace declarations). The app's own extraction was not affected by the symbolic-link ones, but both libraries read untrusted files. | `zip` 9.0.5 and `quick-xml` 0.41. |
| 4 | Medium | macOS | A slide with very deep XML nesting crashed the app (stack overflow). | Nesting is measured on the bytes before parsing; more than 256 levels is refused, as on Windows. |
| 5 | Medium | Both | A merge carried other presentations' document properties (authors, company, thumbnail) and VBA projects (macro code) into the result, unused but present. | Removed from every presentation except the first; the report says when macros were removed. The first file's macros stay only in a PPTM output, as before. |
| 6 | Low | Both | XML parts declaring another encoding (UTF-16, UTF-7, ISO-8859-1) were read differently from how Office reads them, so a DTD could hide behind the encoding check. | Only UTF-8 is accepted; DTDs are refused in any letter case. |
| 7 | Low | Both | Linked pictures, media and OLE links to files or addresses outside the presentation (for example `\\server\share`) were kept silently. PowerPoint may load them when the file opens. | They are kept, since they belong to the original, and listed in the report as a warning. Hyperlinks are not listed: they only open when clicked. |
| 8 | Low | Windows | PowerPoint opened through COM allows macros by default. | `AutomationSecurity = msoAutomationSecurityForceDisable` before opening. |
| 9 | Low | Windows | ZIP entries named after Windows devices (`CON`, `NUL`, `COM1`…), ending in a dot or space, or using forbidden characters were not refused up front. | Refused. |
| 10 | Low | Both | A ZIP file entry also used as a folder (the shape of an archive that writes through a link) and folder entries with `..` relied on `ditto`/the extractor to stop them. Tested: `ditto` already refuses both. | Refused explicitly as a second layer. |
| 11 | Low | Both | A PDF with an unlimited number of pages could take unlimited time and disk space. | At most 2,000 pages per conversion. |

## Checked and found safe

- **Paths in ZIP packages:** absolute paths, `..`, backslashes, duplicates (also differing only in letter case) and symbolic links are refused, and the bytes really written are counted (up to 8 GB and 100,000 entries).
- **Paths in relationships:** a target such as `../../../../Users/x/secret` stays inside the extracted package on both engines, so no file outside it is read.
- **XML:** external entities are never loaded, DTDs are refused, and text written into new slides (notes) is escaped.
- **PowerPoint automation:** file paths are passed as arguments, never written into the AppleScript or PowerShell code, so a file name cannot inject commands.
- **Outputs:**
  - an existing file is never overwritten
  - temporary folders have random names and are removed afterwards
  - the error log and reports stay in the user's logs folder; the portable app writes none
- **Settings:** a small JSON file (Windows) or the app's preferences (macOS), read with strict parsers.
- **Remaining dependency advisories:** "unmaintained" notices only. `paste` is used only while compiling. `ttf-parser` reads only fonts installed on Windows, never fonts from presentations. `zvariant`/`zbus` and the old `quick-xml` 0.30 appear only in Linux builds, which are not made.

## Added after the review: blurred video backgrounds

- **macOS:** the blurred copy of a video is made with AVFoundation and Core Image, which are part of the system.
- **Windows:** FFmpeg is used if the user installed it.
  - It is looked for only in `PPTSHOWTOOLS_FFMPEG`, in PATH folders (never the app's own folder or the current folder) and in the usual install folders (WinGet, Chocolatey, Scoop, Program Files).
  - It is looked for next to the app only when the app is installed.
  - It is started with an argument list, never through a shell, and stopped after 30 minutes.
  - FFmpeg reads the untrusted video, so its own security matters: keep it up to date.
- **Original video:** never modified; the copy is a new file inside the presentation.

## Not covered by code

- **Signing:**
  - the Mac app is ad hoc signed and not notarised
  - the Windows installer and exe are not signed
  - users therefore see Gatekeeper and SmartScreen warnings and cannot verify who built the file, apart from `SHA256SUMS.txt`
  - signing needs an Apple Developer ID and a Windows code-signing certificate
- **The operating system's PDF renderers** (Core Graphics on macOS, Windows.Data.Pdf on Windows) read PDF files, so they are part of the attack surface. They are kept up to date by the system.
- **Windows-only checks not yet run on a real PC:** the program paths, the DLL directory call, PowerPoint `AutomationSecurity` and the device-name refusal. They are built with the Windows target, but the test suite runs on a Mac.
