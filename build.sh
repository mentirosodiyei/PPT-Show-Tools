#!/bin/zsh
# Builds PPT Show Tools for macOS as two separate apps and disk images:
#   dist/PPT Show Tools.app        + dist/PPTShowTools-<v>-AppleSilicon.dmg  (arm64 only)
#   dist/Intel/PPT Show Tools.app  + dist/PPTShowTools-<v>-Intel.dmg         (x86_64 only)
# The Apple Silicon app has no Intel code, so it never runs through Rosetta,
# which macOS 28 removes.
set -eu
cd "${0:A:h}"
VERSION=0.9.0
# Synced folders (iCloud Drive / File Provider) keep re-adding Finder metadata,
# which makes codesign fail. Build and sign in a private temp folder instead.
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/pptshowtools-build.XXXXXX")"
trap 'rm -rf -- "$STAGE"' EXIT
# SDK 27 lacks the SwiftUI macros this toolchain needs; prefer 26.5 when installed.
SDK=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
[[ -d "$SDK" ]] || SDK="$(xcrun --show-sdk-path)"
SOURCES=(Sources/Engine.swift Sources/SlideScaler.swift Sources/PicturePadding.swift Sources/VideoBackdrop.swift Sources/Verifier.swift Sources/OfficePipeline.swift Sources/PowerPointSave.swift Sources/ConversionTools.swift Sources/ImageDeck.swift Sources/MediaExtractor.swift Sources/ReportStore.swift Sources/MainView.swift Sources/App.swift)
mkdir -p dist

# build ARCH FINAL_APP DMG_NAME
build() {
  local arch=$1 final=$2 dmg="$PWD/dist/$3"
  local app="$STAGE/$arch/PPT Show Tools.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  # Module cache outside Documents, which iCloud syncs.
  xcrun swiftc -sdk "$SDK" -module-cache-path "$HOME/Library/Caches/PPTShowTools-mac-build/module-cache-$arch" -swift-version 5 -O -target "$arch-apple-macosx14.0" -parse-as-library "${SOURCES[@]}" -o "$app/Contents/MacOS/PPTShowTools"
  cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>PPTShowTools</string>
<key>CFBundleIdentifier</key><string>io.github.mentirosodiyei.pptshowtools</string>
<key>CFBundleName</key><string>PPT Show Tools</string>
<key>CFBundleDisplayName</key><string>PPT Show Tools</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>LSArchitecturePriority</key><array><string>$arch</string></array>
<key>NSHighResolutionCapable</key><true/>
<key>NSHumanReadableCopyright</key><string>© 2026 David Redondo García · github.com/mentirosodiyei/PPT-Show-Tools</string>
<key>NSAppleEventsUsageDescription</key><string>PPT Show Tools asks Microsoft PowerPoint to open, save and close the merged presentation.</string>
</dict></plist>
PLIST
  cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
  xattr -cr "$app"
  codesign --force --sign - "$app"
  codesign --verify --strict "$app"
  # Replace the previous build only once the new one is signed.
  mkdir -p "${final:h}"
  [[ -e "$final" ]] && rm -rf -- "$final"
  ditto --norsrc --noextattr "$app" "$final"
  # Disk image with the app and a link to Applications, for drag-and-drop install.
  local volume="$STAGE/dmg-$arch"
  mkdir -p "$volume"
  ditto --norsrc --noextattr "$app" "$volume/PPT Show Tools.app"
  ln -s /Applications "$volume/Applications"
  [[ -e "$dmg" ]] && rm -f -- "$dmg"
  hdiutil create -quiet -volname "PPT Show Tools" -srcfolder "$volume" -fs HFS+ -format UDZO -ov "$dmg"
  echo "$final"
  echo "$dmg"
}

build arm64 "$PWD/dist/PPT Show Tools.app" "PPTShowTools-$VERSION-AppleSilicon.dmg"
build x86_64 "$PWD/dist/Intel/PPT Show Tools.app" "PPTShowTools-$VERSION-Intel.dmg"
