#!/usr/bin/env bash
# Build the wasm module into www/pkg/ and (optionally) serve it.
#
#   ./build.sh          # build only
#   ./build.sh --serve  # build, then serve www/ on http://localhost:8000
set -euo pipefail

command -v wasm-pack >/dev/null 2>&1 || {
  echo "wasm-pack not found. Install it with:  cargo install wasm-pack" >&2
  exit 1
}
rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown || {
  echo "Adding wasm32 target…"; rustup target add wasm32-unknown-unknown
}

wasm-pack build --target web --out-dir www/pkg --release

echo "Built www/pkg/. "
if [[ "${1:-}" == "--serve" ]]; then
  cd www
  echo "Serving http://localhost:8000  (Ctrl-C to stop)"
  python3 -m http.server 8000
fi
