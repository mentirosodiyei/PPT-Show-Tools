#!/bin/zsh
set -eu
cd "${0:A:h}"
FINAL="$PWD/dist/PPT Merger.app"
# Synced folders (iCloud Drive / File Provider) keep re-adding Finder metadata,
# which makes codesign fail. Build and sign in a private temp folder instead.
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/pptmerger-build.XXXXXX")"
trap 'rm -rf -- "$STAGE"' EXIT
APP="$STAGE/PPT Merger.app"
mkdir -p "$APP/Contents/MacOS"
# SDK 27 lacks the SwiftUI macros this toolchain needs; prefer 26.5 when installed.
SDK=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
[[ -d "$SDK" ]] || SDK="$(xcrun --show-sdk-path)"
# Module cache outside Documents, which iCloud syncs.
xcrun swiftc -sdk "$SDK" -module-cache-path "$HOME/Library/Caches/PPTMerger-mac-build/module-cache" -swift-version 5 -O -target arm64-apple-macosx14.0 -parse-as-library Sources/Engine.swift Sources/SlideScaler.swift Sources/Verifier.swift Sources/OfficePipeline.swift Sources/PowerPointSave.swift Sources/ConversionTools.swift Sources/ToolsView.swift Sources/App.swift -o "$APP/Contents/MacOS/PPTMerger"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>PPTMerger</string>
<key>CFBundleIdentifier</key><string>local.pptmerger.prototype</string>
<key>CFBundleName</key><string>PPT Merger</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.6.0-test</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSAppleEventsUsageDescription</key><string>PPT Merger demana a Microsoft PowerPoint que obri, desi i tanqui la presentació fusionada.</string>
</dict></plist>
PLIST
mkdir -p "$APP/Contents/Resources/OfficeAddin/web"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp office-addin/preflight.py office-addin/server.py office-addin/manifest.xml office-addin/Setup-Mac.command office-addin/Start-Mac.command office-addin/README.md "$APP/Contents/Resources/OfficeAddin/"
cp office-addin/web/* "$APP/Contents/Resources/OfficeAddin/web/"
xattr -cr "$APP"
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"
# Replace the previous build only once the new one is signed.
mkdir -p dist
[[ -e "$FINAL" ]] && rm -rf -- "$FINAL"
ditto --norsrc --noextattr "$APP" "$FINAL"

echo "$FINAL"
