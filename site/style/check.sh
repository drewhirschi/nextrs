#!/usr/bin/env bash
set -euo pipefail

css="${1:-../public/style.css}"

required=(
  ".hero-grid"
  ".site-header"
  ".docs"
  ".docs-sidebar"
  ".docs-main"
  ".docs-toc"
  ".doc-index-grid"
  ".group-label"
  ".lead"
  "--accent:#ff5a5f"
)

for selector in "${required[@]}"; do
  if ! rg -Fq -- "$selector" "$css"; then
    echo "missing required CSS selector/token: $selector" >&2
    exit 1
  fi
done

echo "verified required site CSS selectors"
