#!/usr/bin/env bash
# Build public/style.css from style/input.css using the vendored Tailwind
# standalone CLI + DaisyUI bundle.
#
# Run this whenever you edit HTML/RS files that use new Tailwind/DaisyUI
# classes, or when you edit input.css itself.
#
# Usage:
#   style/build.sh            # one-shot build
#   style/build.sh --watch    # rebuild on file changes
set -euo pipefail

cd "$(dirname "$0")"

if [ ! -x ./tailwindcss ]; then
  echo "style/tailwindcss not found — downloading…"
  case "$(uname -sm)" in
    "Darwin arm64") url="https://github.com/tailwindlabs/tailwindcss/releases/latest/download/tailwindcss-macos-arm64" ;;
    "Darwin x86_64") url="https://github.com/tailwindlabs/tailwindcss/releases/latest/download/tailwindcss-macos-x64" ;;
    "Linux x86_64") url="https://github.com/tailwindlabs/tailwindcss/releases/latest/download/tailwindcss-linux-x64" ;;
    "Linux aarch64") url="https://github.com/tailwindlabs/tailwindcss/releases/latest/download/tailwindcss-linux-arm64" ;;
    *) echo "Unsupported platform: $(uname -sm)"; exit 1 ;;
  esac
  curl -sL -o tailwindcss "$url"
  chmod +x tailwindcss
fi

./tailwindcss -i input.css -o ../public/style.css --minify "$@"
echo "wrote public/style.css ($(wc -c < ../public/style.css) bytes)"
