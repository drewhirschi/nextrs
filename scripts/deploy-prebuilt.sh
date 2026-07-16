#!/bin/bash
# Prebuilt Vercel deploy: build on THIS machine, upload only artifacts.
# Skips Vercel's build infra entirely — no build queue, no 6-minute cloud
# compile, no per-account build-slot contention. Deploys in seconds.
#
#   scripts/deploy-prebuilt.sh site               # deploy the docs site
#   scripts/deploy-prebuilt.sh examples/react-todos
#
# Requirements (one-time):
#   - vercel CLI, logged in, project linked (<app>/.vercel/project.json)
#   - cargo-zigbuild (cargo install cargo-zigbuild)
#   - zig (any of: system zig, mise, or `pip install ziglang`)
#
# Docs: /docs/deploy-prebuilt on the docs site (site/content/docs/deploy-prebuilt.md).
set -euo pipefail

APP="${1:?usage: deploy-prebuilt.sh <app-dir> [--preview]}"
MODE="${2:---prod}"
[ "$MODE" = "--preview" ] && PROD_FLAGS=() || PROD_FLAGS=(--prod)
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Where to run `vercel build` depends on the project's rootDirectory setting
# (both learned the hard way — the wrong dir silently falls back to
# static-only output, no function, nothing to deploy):
#   - rootDirectory SET (e.g. nextrs-docs → "site"): build from the REPO
#     root; the CLI descends into the root directory itself.
#   - rootDirectory UNSET (e.g. nextrs-react-todos): the app dir IS the
#     project; build from there.
LINK="$ROOT/$APP/.vercel/project.json"
[ -f "$LINK" ] || { echo "ERROR: $APP is not linked (run vercel link in it)" >&2; exit 1; }
if python3 -c "import json,sys; sys.exit(0 if json.load(open('$LINK')).get('settings',{}).get('rootDirectory') else 1)"; then
  mkdir -p "$ROOT/.vercel"
  cp "$LINK" "$ROOT/.vercel/project.json"
  cd "$ROOT"
else
  cd "$ROOT/$APP"
  # Workspace members: cargo's default target dir lives at the WORKSPACE
  # root, which is outside this upload root — the function's filePathMap
  # would point at ../../target and the binary would silently not upload
  # (deployment errors with no message). Keep the build inside the app.
  export CARGO_TARGET_DIR="$PWD/target-vercel"
fi

echo "==> vercel pull (project settings + env)"
vercel pull --yes --environment=production > /dev/null

echo "==> vercel build ${PROD_FLAGS[*]:-(preview)} — local compile, incl. the Rust function"
vercel build "${PROD_FLAGS[@]}"

# Refuse to ship a function that silently failed to build (the classic
# cargo-zigbuild-missing failure mode: everything green, no binary).
if ! find .vercel/output/functions -name '*.func' -type d 2>/dev/null | grep -q .; then
  echo "ERROR: no function in .vercel/output — is cargo-zigbuild installed and zig reachable?" >&2
  exit 1
fi

echo "==> vercel deploy --prebuilt ${PROD_FLAGS[*]:-(preview)}"
vercel deploy --prebuilt "${PROD_FLAGS[@]}"
