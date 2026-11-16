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
- **Windows:** FFmpeg ships with the app.
  - It is pinned to one dated release and verified against the checksum published with it before packaging.
  - The app runs the `ffmpeg.exe` next to it only if its SHA-256 matches the one compiled into the app, so a file planted next to the portable app is never run.
  - Otherwise it falls back to an FFmpeg the user installed.
  - It is looked for only in `PPTSHOWTOOLS_FFMPEG`, in PATH folders (never the app's own folder or the current folder) and in the usual install folders (WinGet, Chocolatey, Scoop, Program Files).
  - It is started with an argument list, never through a shell, and stopped after 30 minutes.
  - FFmpeg reads the untrusted video, so its own security matters: keep it up to date.
- **Original video:** never modified; the copy is a new file inside the presentation.

## Added after the review: Images to PowerPoint

- **Picture decoding:** pictures are untrusted input.
  - macOS decodes them with ImageIO, part of the system.
  - Windows uses the `image` library (JPEG, PNG, GIF, WebP, TIFF, BMP), which applies its default memory limits. HEIC/HEIF and AVIF go through the bundled FFmpeg, found and started as for videos.
- **Folders:** only the folder itself is read, not subfolders; hidden files are skipped; at most 2,000 pictures.
- **Originals:** JPEG, PNG and GIF are copied byte for byte and checked by SHA-256 after saving; nothing is written next to the originals, and an existing output is never replaced.

## Added after the review: updates

- **What is installed:** an update is installed only if the release's `SHA256SUMS.txt` has a valid Ed25519 signature from the author's key, whose public half is built into both apps, and the download matches its checksum. A file replaced on GitHub, or by anyone in between, is refused, even with a valid TLS connection.
- **Where it comes from:** downloads are HTTPS from GitHub only. Redirects are followed one by one and each new address is checked against the same list, with a size limit.
- **macOS:** the disk image must contain PPT Show Tools with the expected bundle identifier and version. The running app is moved aside and replaced, without quarantine attributes (the update was already verified). The old copy is deleted at the next launch. An app still on its disk image, or translocated, is not replaced: the release page is opened instead.
- **Windows:** the installed app runs the verified installer silently (per user, no administrator rights). The portable app takes only `PPTShowTools.exe`, `ffmpeg.exe` and the FFmpeg notices from the zip's `PPT Show Tools/` folder, so a crafted zip cannot write elsewhere.
- **Tests:** the suite plays GitHub on 127.0.0.1 with a throw-away key. That test server and key are accepted only for 127.0.0.1.
- **The private key** is the new crown jewel. It is outside the repository, readable only by its owner; if it leaked, someone with access to the GitHub account could push an update. Keep it backed up and private.

## Second audit (external, November 2026)

An external audit of 0.9.0 reported 13 findings. Each one was checked in the code and fixed on both engines. Regression tests are in `tests/test_audit_2026_11b.py`.

| # | Severity | Platform | Finding | Fix |
|---|---|---|---|---|
| 1 | High | macOS | The ZIP names were checked with `zipinfo`, which reads only the central directory and rewrites control characters and accents, while `ditto` extracts from the local headers. A renamed or hidden local entry could get past the checks. `ditto` also treated `__MACOSX/._*` entries as resource forks. | The app reads the central directory itself and requires every local header to match its entry, in one unbroken run. Names with control characters are refused. After extraction, the files on disk must be exactly the checked ones. `ditto` runs with `--norsrc --noextattr`. |
| 2 | Medium | Both | When merging, an imported package's relationship could reach the first presentation's parts: an absolute target with `..`, or a relative one climbing above its root (for example from a theme). The verifier only looked at slide, layout, master and notes relationships. | Targets are resolved inside their own package. Absolute targets, and relative ones that would climb out, are rewritten under `pptMerger/sourceN/`. The verifier checks every relationship part. |
| 3 | Medium | Both | The 30-day cleanup deleted any old file in `PPTSHOWTOOLS_LOG_DIR`. | Only the app's own reports and error logs are deleted. |
| 4 | Medium | Both | Very large pictures were decoded into full-size RGBA buffers. HEIC/AVIF went through FFmpeg with no time limit, no protocol limit and output held in memory. | Pictures over 36 million pixels are reduced before padding; over 128 million they are not decoded. Transparency is checked in strips. FFmpeg gets an absolute path, `-protocol_whitelist file`, a time limit and a temporary output of bounded size. |
| 5 | Low | Both | Macros were found only as `ppt/vbaProject.bin`. `customXml` from imported files was kept. | Macro parts are found by name, content type and relationship type, with their relationship parts. `customXml` is dropped from imported files. |
| 6 | Low | macOS | `resolvePart` used `standardizingPath`, which consults the Mac's own folders (`/var`, `/tmp`); an invalid `%` kept the `#fragment`. | Normalization works on the text alone, as on Windows; the fragment is always dropped. |
| 7 | Bug | Both | A failed blurred copy left an unreferenced `.mp4`, which could break the content-type check. | The file is deleted; the still is used. |
| 8 | Bug | Both | Resizing twice stacked blurred backgrounds; Extract media returned them. | Earlier backgrounds are removed and made again for their video; Extract media leaves them out. |
| 9 | Bug | Both | `Override PartName` was compared with letter case and without `%XX` decoding. | Compared as OPC requires. |
| 10 | Bug | Both | Notes with `&#13;` failed Extract notes' own check. | Carriage returns become line breaks. |
| 11 | Bug | Both | Windows: PowerPoint's macro setting was not restored and "closed" was reported before closing. Both: a first file with macros gave a contradictory message with "Merge and save with PowerPoint…". | The setting is restored; the file is closed before reporting. The message now explains that this command cannot keep macros. |
| 12 | Bug | Both | Animation formulas written `#ppt_x+0.1`, and motion paths such as `0.5-0.25`, were not scaled. | Both forms are scaled. |
| 13 | Bug | Both | Function switch kept the Merge selection (macOS); a duplicate drop gave the wrong message; Images had no status on Windows; `--office-prepare` and `--verify` ignored `--size`; a `<!DOCTYPE` inside a comment was refused (macOS); the serializer could misread a delimiter. | All fixed. |

## Not covered by code

- **Signing:**
  - the Mac app is ad hoc signed and not notarised
  - the Windows installer and exe are not signed
  - users therefore see Gatekeeper and SmartScreen warnings and cannot verify who built the file, apart from `SHA256SUMS.txt`
  - signing needs an Apple Developer ID and a Windows code-signing certificate
- **The operating system's PDF renderers** (Core Graphics on macOS, Windows.Data.Pdf on Windows) read PDF files, so they are part of the attack surface. They are kept up to date by the system.
- **Windows-only checks not yet run on a real PC:** the program paths, the DLL directory call, PowerPoint `AutomationSecurity` (now also restored afterwards) and the device-name refusal. They are built with the Windows target, but the test suite runs on a Mac.
