#!/bin/zsh
set -eu
cd "${0:A:h}"
VERSION=0.7.0
FINAL="$PWD/dist/Deckwright.app"
# Synced folders (iCloud Drive / File Provider) keep re-adding Finder metadata,
# which makes codesign fail. Build and sign in a private temp folder instead.
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/deckwright-build.XXXXXX")"
trap 'rm -rf -- "$STAGE"' EXIT
APP="$STAGE/Deckwright.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# SDK 27 lacks the SwiftUI macros this toolchain needs; prefer 26.5 when installed.
SDK=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
[[ -d "$SDK" ]] || SDK="$(xcrun --show-sdk-path)"
# Module cache outside Documents, which iCloud syncs.
xcrun swiftc -sdk "$SDK" -module-cache-path "$HOME/Library/Caches/Deckwright-mac-build/module-cache" -swift-version 5 -O -target arm64-apple-macosx14.0 -parse-as-library Sources/Engine.swift Sources/SlideScaler.swift Sources/Verifier.swift Sources/OfficePipeline.swift Sources/PowerPointSave.swift Sources/ConversionTools.swift Sources/MainView.swift Sources/App.swift -o "$APP/Contents/MacOS/Deckwright"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Deckwright</string>
<key>CFBundleIdentifier</key><string>io.github.mentirosodiyei.deckwright</string>
<key>CFBundleName</key><string>Deckwright</string>
<key>CFBundleDisplayName</key><string>Deckwright</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSAppleEventsUsageDescription</key><string>Deckwright asks Microsoft PowerPoint to open, save and close the merged presentation.</string>
</dict></plist>
PLIST
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
xattr -cr "$APP"
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"
# Replace the previous build only once the new one is signed.
mkdir -p dist
[[ -e "$FINAL" ]] && rm -rf -- "$FINAL"
ditto --norsrc --noextattr "$APP" "$FINAL"

echo "$FINAL"
