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

# Monorepo rule (learned the hard way): when the Vercel project sets a
# rootDirectory, `vercel build` must run from the REPO root, or the CLI
# never engages the app's builders and falls back to static-only output.
# The app dir holds the canonical link; mirror it to the root for the build.
if [ -f "$ROOT/$APP/.vercel/project.json" ]; then
  mkdir -p "$ROOT/.vercel"
  cp "$ROOT/$APP/.vercel/project.json" "$ROOT/.vercel/project.json"
fi
cd "$ROOT"

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
