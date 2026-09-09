#!/bin/bash
# Build with the framework/CLI at this checkout, then upload prebuilt artifacts.
set -euo pipefail
APP="${1:?usage: deploy-prebuilt.sh <app-dir> [--preview]}"
MODE="${2:---prod}"
case "$MODE" in
  --preview) FLAGS=(--preview) ;;
  --prod) FLAGS=() ;;
  *) echo "unknown deployment mode: $MODE" >&2; exit 1 ;;
esac
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export NEXTRS_BUILD_REVISION="${NEXTRS_BUILD_REVISION:-$(git -C "$ROOT" rev-parse HEAD)}"
cd "$ROOT"
cargo run --locked --quiet -p cargo-nextrs --bin nextrs -- deploy --root "$ROOT/$APP" "${FLAGS[@]}"
