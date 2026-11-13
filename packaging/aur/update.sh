#!/bin/sh
# Points the AUR package at a release and refreshes the checksum and .SRCINFO.
# Usage: packaging/aur/update.sh 0.2.0 [path/to/Gooti_0.2.0_amd64.deb]
# Without a path, the .deb is downloaded from the GitHub release.
set -eu
version=${1:?usage: update.sh <version> [deb]}
here=$(cd "$(dirname "$0")/gooti-desktop-bin" && pwd)
deb=${2:-}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [ -z "$deb" ]; then
  deb="$work/Gooti_${version}_amd64.deb"
  curl -fsSL -o "$deb" "https://github.com/sam-hayes-org/gooti-desktop/releases/download/v${version}/Gooti_${version}_amd64.deb"
fi
sum=$(sha256sum "$deb" | cut -d' ' -f1)
cd "$here"
sed -i "s/^pkgver=.*/pkgver=$version/; s/^pkgrel=.*/pkgrel=1/; s/^sha256sums=.*/sha256sums=('$sum')/" PKGBUILD
makepkg --printsrcinfo > .SRCINFO
echo "PKGBUILD and .SRCINFO now point at $version. Push this folder to the AUR repository gooti-desktop-bin."
