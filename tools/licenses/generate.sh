#!/bin/bash
# Writes src-tauri/licenses/THIRD-PARTY-NOTICES.txt: Gooti's own license, then the licenses of
# everything Gooti Desktop ships with it. Every bundle carries the file, and Info → Open-source
# licenses shows it.
#   tools/licenses/generate.sh           write the file (after a dependency change; commit it)
#   tools/licenses/generate.sh --check   only compare (CI): fails when the file is out of date
# Needs cargo-about 0.9 (cargo install cargo-about --locked --features cli). Builds the UI first,
# because Angular lists the UI's packages while it builds.
set -euo pipefail
cd "$(dirname "$0")/../.."
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

OUT=src-tauri/licenses/THIRD-PARTY-NOTICES.txt
command -v cargo-about >/dev/null ||
  { echo "Missing cargo-about: cargo install cargo-about --locked --features cli" >&2; exit 1; }

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

npm --prefix ui run build >/dev/null

rule() { printf '\n%s\n%s\n%s\n\n' "================================================================================" "$1" "================================================================================"; }

# A UI package that Angular does not list (styles and fonts): name, version, its license file.
style_package() {
  local dir="ui/node_modules/$1"
  local version license
  version=$(node -p "require('./$dir/package.json').version")
  license=$(node -p "require('./$dir/package.json').license")
  printf -- '--------------------------------------------------------------------------------\n'
  printf 'Package: %s %s\nLicense: %s\nUsed for: %s\n\n' "$1" "$version" "$license" "$2"
  cat "$dir/LICENSE"
  printf '\n'
}

{
  echo "Gooti Desktop: licenses"
  echo
  echo "Gooti Desktop is under the MIT License (first section). It ships with the open-source"
  echo "software listed below, under the licenses that follow each name."

  rule "Gooti Desktop"
  cat LICENSE

  rule "Linux system libraries in the AppImage"
  cat <<'EOF'
The AppImage also contains libraries of the Linux system it is built on (Ubuntu 22.04), among
them WebKitGTK, JavaScriptCore, GTK 3, libsoup 3, GLib, Cairo, Pango, HarfBuzz and libsecret.
Most of them are under the GNU Lesser General Public License, version 2.1 or later
(https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html); each library's own license and
copyright file is part of its Ubuntu package. The source code of each of them, in the version
the AppImage contains, is available from Ubuntu (https://packages.ubuntu.com/jammy/, "Download
Source Package") and from the projects themselves:
  WebKitGTK and JavaScriptCore  https://webkitgtk.org
  GTK, GLib                     https://www.gtk.org
  libsoup                       https://gitlab.gnome.org/GNOME/libsoup
  Cairo                         https://www.cairographics.org
  Pango                         https://gitlab.gnome.org/GNOME/pango
  HarfBuzz                      https://harfbuzz.github.io
  libsecret                     https://gitlab.gnome.org/GNOME/libsecret
You may replace these libraries with other versions: the deb, rpm and AUR packages use the
ones your system provides. The Windows version uses Microsoft Edge WebView2, which Windows
provides.
EOF

  rule "User interface: packages"
  cat ui/dist/gooti-ui/3rdpartylicenses.txt

  rule "User interface: styles, icons and fonts"
  style_package bootstrap "styles"
  style_package bootstrap-icons "icons"
  style_package @fontsource-variable/inter "the Inter font"

  rule "Rust crates"
  cargo about generate --fail tools/licenses/rust.hbs 2>/dev/null
} >"$tmp"

if [[ "${1:-}" == "--check" ]]; then
  if ! cmp -s "$tmp" "$OUT"; then
    echo "$OUT is out of date. Run tools/licenses/generate.sh and commit it." >&2
    diff <(grep -E '^(Package:|  [a-z0-9_-]+ [0-9])' "$OUT" 2>/dev/null) \
         <(grep -E '^(Package:|  [a-z0-9_-]+ [0-9])' "$tmp") | head -20 >&2 || true
    exit 1
  fi
  echo "$OUT is up to date."
else
  mkdir -p "$(dirname "$OUT")"
  cp "$tmp" "$OUT"
  chmod 644 "$OUT" # mktemp makes 600; installed as root, the file must stay readable for users
  echo "Wrote $OUT ($(wc -c <"$OUT") bytes)."
fi
