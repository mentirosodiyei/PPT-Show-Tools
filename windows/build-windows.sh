#!/bin/zsh
# Builds PPT Show Tools for Windows from macOS: .exe (cargo-xwin), installer (NSIS)
# and a portable zip, in ../dist/windows.
set -eu
cd "${0:A:h}"
# Homebrew's makensis aborts (std::bad_alloc) without a UTF-8 locale.
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
# Build output outside Documents, which iCloud syncs (over 1 GB).
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/Library/Caches/PPTShowTools-windows-target}"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT="$PWD/../dist/windows"
mkdir -p "$OUT"
cargo xwin build --release --target x86_64-pc-windows-msvc
EXE="$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release/PPTShowTools.exe"
SETUP="$OUT/PPTShowTools-Setup-$VERSION.exe"
PORTABLE="$OUT/PPTShowTools-$VERSION-Windows-portable.zip"
rm -f -- "$SETUP" "$PORTABLE"
makensis -V2 -DVERSION="$VERSION" -DEXE="$EXE" -DOUTFILE="$SETUP" installer/installer.nsi
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/pptshowtools-win.XXXXXX")"
trap 'rm -rf -- "$STAGE"' EXIT
mkdir -p "$STAGE/PPT Show Tools"
cp "$EXE" "$STAGE/PPT Show Tools/PPTShowTools.exe"
(cd "$STAGE" && zip -qr "$PORTABLE" "PPT Show Tools")
(cd "$OUT" && shasum -a 256 "${SETUP:t}" "${PORTABLE:t}" > "SHA256SUMS-$VERSION.txt")
echo "$SETUP"
echo "$PORTABLE"
