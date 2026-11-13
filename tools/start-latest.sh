#!/bin/bash
# Starts the newest AppImage from target/local/bundle/appimage (npm run build:local:appimage) or
# target/release/bundle/appimage (npm run build:local), whichever was built last.
#   tools/start-latest.sh            start it
#   tools/start-latest.sh --which    only print which file it would start
# Gooti runs once: if it is already running, starting it again brings that window to the front.
set -euo pipefail
cd "$(dirname "$0")/.."

latest=$(ls -t target/local/bundle/appimage/*.AppImage target/release/bundle/appimage/*.AppImage \
  2>/dev/null | head -n 1 || true)
if [[ -z $latest ]]; then
  echo "No AppImage built yet. Build one with: npm run build:local:appimage" >&2
  exit 1
fi

if [[ ${1:-} == --which ]]; then
  echo "$latest"
  exit 0
fi

echo "Starting $latest (built $(date -r "$latest" '+%Y-%m-%d %H:%M'))"
exec "$latest"
