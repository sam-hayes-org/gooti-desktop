#!/bin/sh
# Bundles a vector script against the extension's backup code and runs it with Node.
# Usage: tools/vectors/run.sh generate|check|e2e <args...>
set -eu
here=$(cd "$(dirname "$0")" && pwd)
ext=${GOOTI_EXTENSION:-"$here/../../../gooti-extension"}
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
NODE_PATH="$ext/node_modules" "$ext/node_modules/.bin/esbuild" "$here/$1.ts" --bundle --platform=node --format=esm \
  --alias:@ext="$ext/projects/common/src/lib" --outfile="$out/$1.mjs" --log-level=warning \
  --banner:js="import { createRequire } from 'module'; const require = createRequire(import.meta.url);"
script=$1
shift
NODE_PATH="$ext/node_modules" node "$out/$script.mjs" "$@"
