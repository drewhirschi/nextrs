#!/usr/bin/env bash
# Rebuild crates/nextrs/vendor/foresight/ — the single-file ForesightJS bundle
# the framework embeds (include_str!) and splices into every app shell.
#
# Vendored rather than an npm dependency so existing apps get intent preload
# on their next rebuild without touching their package.json. To upgrade: bump
# the versions below, run this script, review the diff, and run e2e/hover.mjs.
#
# Usage: scripts/vendor-foresight.sh
set -euo pipefail

FORESIGHT=4.2.1
POSITION_OBSERVER=1.0.3
TABBABLE=6.5.2
ESBUILD=0.25.10

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/crates/nextrs/vendor/foresight"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cd "$work"
cat > package.json <<JSON
{
  "private": true,
  "dependencies": { "js.foresight": "$FORESIGHT" },
  "overrides": { "position-observer": "$POSITION_OBSERVER", "tabbable": "$TABBABLE" }
}
JSON
npm install --ignore-scripts --no-audit --no-fund --silent

mkdir -p "$dest"
banner="/*! Vendored by nextrs (scripts/vendor-foresight.sh). Do not edit.
 * js.foresight $FORESIGHT, position-observer $POSITION_OBSERVER, tabbable $TABBABLE — all MIT.
 * License texts: crates/nextrs/vendor/foresight/LICENSES.md */"
echo 'export { ForesightManager } from "js.foresight";' > entry.mjs
npx --yes "esbuild@$ESBUILD" entry.mjs --bundle --format=esm --target=es2020 \
  --legal-comments=none --banner:js="$banner" --outfile="$dest/foresight.mjs" --log-level=warning

{
  echo "# Third-party licenses for the vendored ForesightJS bundle"
  echo
  echo "\`foresight.mjs\` in this directory bundles the packages below, unmodified."
  for pkg in js.foresight position-observer tabbable; do
    version="$(node -p "require('./node_modules/$pkg/package.json').version")"
    license="$(ls "node_modules/$pkg" | grep -i -m1 '^licen[sc]e')"
    echo
    echo "## $pkg $version"
    echo
    echo '```'
    cat "node_modules/$pkg/$license"
    echo '```'
  done
} > "$dest/LICENSES.md"

echo "wrote $dest/foresight.mjs ($(wc -c < "$dest/foresight.mjs") bytes, $(gzip -c "$dest/foresight.mjs" | wc -c) gzipped)"
