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
# FFmpeg (LGPL build) ships with the app for the blurred video backgrounds.
# Pinned to one dated release and checked against its published checksum.
FFMPEG_RELEASE="autobuild-2026-11-08-12-35"
FFMPEG_ZIP="ffmpeg-n9.0.2-48-gd4070a14ce-win64-lgpl-9.0.zip"
FFMPEG_ZIP_SHA256="${FFMPEG_ZIP_SHA256:-}"
FFMPEG_CACHE="$HOME/Library/Caches/PPTShowTools-ffmpeg"
mkdir -p "$FFMPEG_CACHE"
if [[ ! -f "$FFMPEG_CACHE/$FFMPEG_ZIP" ]]; then
  curl -fsSL -o "$FFMPEG_CACHE/checksums.sha256" "https://github.com/BtbN/FFmpeg-Builds/releases/download/$FFMPEG_RELEASE/checksums.sha256"
  curl -fsSL -o "$FFMPEG_CACHE/$FFMPEG_ZIP.part" "https://github.com/BtbN/FFmpeg-Builds/releases/download/$FFMPEG_RELEASE/$FFMPEG_ZIP"
  mv "$FFMPEG_CACHE/$FFMPEG_ZIP.part" "$FFMPEG_CACHE/$FFMPEG_ZIP"
fi
EXPECTED=$(awk -v f="$FFMPEG_ZIP" '$2 == f || $2 == "*" f {print $1}' "$FFMPEG_CACHE/checksums.sha256")
ACTUAL=$(shasum -a 256 "$FFMPEG_CACHE/$FFMPEG_ZIP" | cut -d' ' -f1)
if [[ -z "$EXPECTED" || "$EXPECTED" != "$ACTUAL" ]]; then
  echo "FFmpeg download does not match its published checksum." >&2; exit 1
fi
FFMPEG_DIR="$FFMPEG_CACHE/${FFMPEG_ZIP:r}"
if [[ ! -f "$FFMPEG_DIR/ffmpeg.exe" ]]; then
  rm -rf -- "$FFMPEG_DIR"; mkdir -p "$FFMPEG_DIR"
  unzip -q -j "$FFMPEG_CACHE/$FFMPEG_ZIP" "${FFMPEG_ZIP:r}/bin/ffmpeg.exe" "${FFMPEG_ZIP:r}/LICENSE.txt" -d "$FFMPEG_DIR"
fi
FFMPEG_EXE="$FFMPEG_DIR/ffmpeg.exe"
cp ../FFMPEG-NOTICE.txt "$FFMPEG_DIR/FFMPEG-NOTICE.txt"
# The app accepts the ffmpeg.exe next to it only if it is exactly this file.
export PPTSHOWTOOLS_BUNDLED_FFMPEG_SHA256=$(shasum -a 256 "$FFMPEG_EXE" | cut -d' ' -f1)
cargo xwin build --release --target x86_64-pc-windows-msvc
EXE="$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release/PPTShowTools.exe"
SETUP="$OUT/PPTShowTools-Setup-$VERSION.exe"
PORTABLE="$OUT/PPTShowTools-$VERSION-Windows-portable.zip"
rm -f -- "$SETUP" "$PORTABLE"
makensis -V2 -INPUTCHARSET UTF8 -DVERSION="$VERSION" -DEXE="$EXE" -DFFMPEG="$FFMPEG_DIR" -DOUTFILE="$SETUP" installer/installer.nsi
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/pptshowtools-win.XXXXXX")"
trap 'rm -rf -- "$STAGE"' EXIT
mkdir -p "$STAGE/PPT Show Tools"
cp "$EXE" "$STAGE/PPT Show Tools/PPTShowTools.exe"
cp "$FFMPEG_EXE" "$STAGE/PPT Show Tools/ffmpeg.exe"
cp "$FFMPEG_DIR/LICENSE.txt" "$STAGE/PPT Show Tools/FFMPEG-LICENSE.txt"
cp "$FFMPEG_DIR/FFMPEG-NOTICE.txt" "$STAGE/PPT Show Tools/FFMPEG-NOTICE.txt"
(cd "$STAGE" && zip -qr "$PORTABLE" "PPT Show Tools")
(cd "$OUT" && shasum -a 256 "${SETUP:t}" "${PORTABLE:t}" > "SHA256SUMS-$VERSION.txt")
echo "$SETUP"
echo "$PORTABLE"
