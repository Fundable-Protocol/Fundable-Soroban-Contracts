#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$REPO_ROOT/scripts/verify_release_provenance.sh"

(
  cd "$REPO_ROOT"
  cargo test --locked -p router release_wasm_ -- --nocapture --test-threads=1
)
