#!/bin/bash
# Starts the newest AppImage from target/release/bundle/appimage (npm run build:local makes one).
#   tools/start-latest.sh            start it
#   tools/start-latest.sh --which    only print which file it would start
# Gooti runs once: if it is already running, starting it again brings that window to the front.
set -euo pipefail
cd "$(dirname "$0")/.."

latest=$(ls -t target/release/bundle/appimage/*.AppImage 2>/dev/null | head -n 1 || true)
if [[ -z $latest ]]; then
  echo "No AppImage in target/release/bundle/appimage. Build one with: npm run build:local" >&2
  exit 1
fi

if [[ ${1:-} == --which ]]; then
  echo "$latest"
  exit 0
fi

echo "Starting $latest (built $(date -r "$latest" '+%Y-%m-%d %H:%M'))"
exec "$latest"
