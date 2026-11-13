#!/bin/bash
# Checks the Windows code on Linux: clippy for Windows, then every test as a Windows binary under Wine.
# The real Windows build (MSVC) runs on GitHub (develop.yml); this catches most of it earlier.
#   tools/check-windows.sh              clippy and the tests
#   tools/check-windows.sh --ignored    also the OS keychain test (Wine's Credential Manager)
# Needs: rustup target add x86_64-pc-windows-gnu, mingw-w64-gcc, wine.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-gnu
export WINEPREFIX="$PWD/target/wine" WINEDEBUG=-all
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine

cargo clippy --workspace --all-targets --target "$TARGET" -- -D warnings
cargo test --workspace --target "$TARGET" --no-run
# The GNU build loads WebView2Loader.dll at start (MSVC links it in). Put it next to the tests.
cp "target/$TARGET/debug/WebView2Loader.dll" "target/$TARGET/debug/deps/"
cargo test --workspace --target "$TARGET"
if [[ "${1:-}" == "--ignored" ]]; then
  cargo test -p gooti-desktop --target "$TARGET" -- --ignored
fi
