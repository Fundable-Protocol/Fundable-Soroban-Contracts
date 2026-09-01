#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_COMMIT="$(git -C "$REPO_ROOT" rev-parse HEAD)"
OUTPUT_DIR="${1:-$REPO_ROOT/target/reproducible-release}"
BUILD_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/fundable-wasm-repro.XXXXXX")"

cleanup() {
  rm -rf -- "$BUILD_ROOT"
}
trap cleanup EXIT

CONTRACT_PACKAGES=(flow lockup stream-nft governance router)
WASM_FILES=(flow.wasm lockup.wasm stream_nft.wasm governance.wasm router.wasm)

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

build_once() {
  local run_name="$1"
  local source_dir="$BUILD_ROOT/source-$run_name"
  local artifact_dir="$BUILD_ROOT/artifacts-$run_name"

  mkdir -p "$source_dir" "$artifact_dir"
  git -C "$REPO_ROOT" archive --format=tar "$SOURCE_COMMIT" | tar -xf - -C "$source_dir"

  for package in "${CONTRACT_PACKAGES[@]}"; do
    (
      cd "$source_dir"
      stellar contract build \
        --locked \
        --package "$package" \
        --out-dir "$artifact_dir"
    )
  done
}

build_once first
build_once second

mkdir -p "$OUTPUT_DIR"
MANIFEST="$OUTPUT_DIR/SHA256SUMS"
: >"$MANIFEST"

for wasm in "${WASM_FILES[@]}"; do
  first="$BUILD_ROOT/artifacts-first/$wasm"
  second="$BUILD_ROOT/artifacts-second/$wasm"

  cmp --silent "$first" "$second" || {
    echo "reproducibility failure: $wasm differs between clean builds" >&2
    exit 1
  }

  cp "$first" "$OUTPUT_DIR/$wasm"
  printf '%s  %s\n' "$(hash_file "$first")" "$wasm" >>"$MANIFEST"
done

echo "Reproducible WASMs verified for commit $SOURCE_COMMIT"
echo "Artifacts: $OUTPUT_DIR"
cat "$MANIFEST"
