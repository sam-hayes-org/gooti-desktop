#!/bin/sh
# Sets the app version everywhere it is written. Usage: tools/release/set-version.sh 0.2.0
set -eu
version=${1:?usage: set-version.sh <version>}
case "$version" in
  *[!0-9.]* | "" ) echo "A version looks like 0.2.0" >&2; exit 1 ;;
esac
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
# The workspace version is the app version (tauri.conf.json takes it from Cargo).
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
npm pkg set version="$version" >/dev/null
npm install --package-lock-only --ignore-scripts --silent
cargo update -p gooti-core -p gooti-desktop --quiet
echo "Version $version. Next: in CHANGELOG.md, rename Unreleased to $version, commit \"Release $version\" (docs/RELEASING.md)."
