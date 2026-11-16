#!/bin/zsh
# Prepares a release for GitHub: the two macOS disk images, the Windows
# installer and portable zip, SHA256SUMS.txt and its signature
# (SHA256SUMS.txt.sig). The apps install an update only if that signature
# is valid, so every release must be made with this script.
#
#   ./build.sh && windows/build-windows.sh && tools/make-release.sh
#   gh release create vVERSION dist/release-VERSION/*
set -eu
cd "${0:A:h}/.."
VERSION=$(sed -n 's/^VERSION=\(.*\)/\1/p' build.sh | head -1)
WINDOWS=$(sed -n 's/^version = "\(.*\)"/\1/p' windows/Cargo.toml | head -1)
[[ "$VERSION" == "$WINDOWS" ]] || { echo "build.sh says $VERSION but windows/Cargo.toml says $WINDOWS." >&2; exit 1; }
OUT="dist/release-$VERSION"
FILES=("dist/PPTShowTools-$VERSION-AppleSilicon.dmg" "dist/PPTShowTools-$VERSION-Intel.dmg" "dist/windows/PPTShowTools-Setup-$VERSION.exe" "dist/windows/PPTShowTools-$VERSION-Windows-portable.zip")
for f in "${FILES[@]}"; do [[ -f "$f" ]] || { echo "Missing $f: build both apps first." >&2; exit 1; }; done
rm -rf -- "${OUT:?}"; mkdir -p "$OUT"
cp "${FILES[@]}" "$OUT/"
(cd "$OUT" && shasum -a 256 PPTShowTools-* > SHA256SUMS.txt)
swift tools/release-sign.swift sign "$OUT/SHA256SUMS.txt"
swift tools/release-sign.swift verify "$OUT/SHA256SUMS.txt"
echo "Ready in $OUT:"; ls -1 "$OUT"
