#!/bin/bash
# Builds the Windows installer on Linux, without update files (like build:local), for trying it in
# a Windows VM. The release installer still comes from GitHub (release.yml).
# Result: target/x86_64-pc-windows-msvc/release/bundle/nsis/Gooti_<version>_x64-setup.exe
#
# Needs, once:
#   rustup target add x86_64-pc-windows-msvc
#   cargo install cargo-xwin --locked
#   clang, lld and llvm (llvm-rc), wine
# The first build downloads Microsoft's C runtime and Windows SDK into ~/.cache/cargo-xwin, and the
# official NSIS for Windows into ~/.cache/gooti-desktop (checked against its SHA-1). It runs under
# Wine: the Arch package nsis makes 64-bit installers only, and Tauri's installer needs 32-bit NSIS.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-msvc
# cargo install puts cargo-xwin here, which is not always on PATH.
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

missing=()
command -v cargo-xwin >/dev/null || missing+=("cargo-xwin (cargo install cargo-xwin --locked)")
for tool in clang lld-link llvm-rc; do
  command -v "$tool" >/dev/null || missing+=("$tool (clang, lld, llvm)")
done
command -v wine >/dev/null || missing+=("wine")
rustup target list --installed | grep -qx "$TARGET" ||
  missing+=("the Rust target (rustup target add $TARGET)")
if ((${#missing[@]})); then
  printf 'Missing:\n' >&2
  printf '  %s\n' "${missing[@]}" >&2
  exit 1
fi

NSIS_VERSION=3.13
NSIS_SHA1=db14f8af2ea346b786c6ae19343f61aaf5e09b8e
NSIS_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/gooti-desktop"
export GOOTI_NSIS_DIR="$NSIS_CACHE/nsis-$NSIS_VERSION"
if [[ ! -f "$GOOTI_NSIS_DIR/makensis.exe" ]]; then
  mkdir -p "$NSIS_CACHE"
  zip="$NSIS_CACHE/nsis-$NSIS_VERSION.zip"
  curl -fsSL -o "$zip" \
    "https://downloads.sourceforge.net/project/nsis/NSIS%203/$NSIS_VERSION/nsis-$NSIS_VERSION.zip"
  echo "$NSIS_SHA1  $zip" | sha1sum --check --quiet
  unzip -q -o "$zip" -d "$NSIS_CACHE"
  rm "$zip"
fi
export PATH="$PWD/tools/windows:$PATH"
export WINEPREFIX="$PWD/target/wine" WINEDEBUG=-all

npx tauri build --runner cargo-xwin --target "$TARGET" --config src-tauri/tauri.local.conf.json "$@"
ls -1 "target/$TARGET/release/bundle/nsis/"*.exe
