#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_DIR="${1:-$REPO_ROOT/release/stellar-streams-mainnet}"
ARTIFACT_DIR="${2:-$REPO_ROOT/target/reproducible-release}"
PROVENANCE="$RELEASE_DIR/PROVENANCE.tsv"
CHECKSUMS="$RELEASE_DIR/SHA256SUMS"

fail() {
  echo "provenance verification failure: $*" >&2
  exit 1
}

manifest_value() {
  local key="$1"
  awk -F '\t' -v key="$key" '$1 == key { print $2; found = 1 } END { if (!found) exit 1 }' "$PROVENANCE"
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

assert_equal() {
  local label="$1"
  local expected="$2"
  local actual="$3"
  [[ "$actual" == "$expected" ]] || fail "$label: expected '$expected', got '$actual'"
}

[[ -f "$PROVENANCE" ]] || fail "missing $PROVENANCE"
[[ -f "$CHECKSUMS" ]] || fail "missing $CHECKSUMS"
[[ -d "$ARTIFACT_DIR" ]] || fail "missing artifact directory $ARTIFACT_DIR"

SOURCE_COMMIT="$(manifest_value source.git_commit)"
git -C "$REPO_ROOT" cat-file -e "$SOURCE_COMMIT^{commit}" 2>/dev/null || \
  fail "source commit $SOURCE_COMMIT is unavailable"

assert_equal \
  "source Git tree" \
  "$(manifest_value source.git_tree)" \
  "$(git -C "$REPO_ROOT" rev-parse "$SOURCE_COMMIT^{tree}")"

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fundable-provenance.XXXXXX")"
cleanup() {
  rm -rf -- "$TEMP_DIR"
}
trap cleanup EXIT

git -C "$REPO_ROOT" show "$SOURCE_COMMIT:Cargo.lock" >"$TEMP_DIR/Cargo.lock"
git -C "$REPO_ROOT" show "$SOURCE_COMMIT:rust-toolchain.toml" >"$TEMP_DIR/rust-toolchain.toml"

assert_equal \
  "Cargo.lock SHA-256" \
  "$(manifest_value dependency.lockfile_sha256)" \
  "$(hash_file "$TEMP_DIR/Cargo.lock")"
assert_equal \
  "rust-toolchain.toml SHA-256" \
  "$(manifest_value toolchain.file_sha256)" \
  "$(hash_file "$TEMP_DIR/rust-toolchain.toml")"

SDK_VERSION="$(awk '
  $0 == "name = \"soroban-sdk\"" { in_package = 1; next }
  in_package && /^version = / { gsub(/^version = \"|\"$/, ""); print; exit }
' "$TEMP_DIR/Cargo.lock")"
SDK_CHECKSUM="$(awk '
  $0 == "name = \"soroban-sdk\"" { in_package = 1; next }
  in_package && /^checksum = / { gsub(/^checksum = \"|\"$/, ""); print; exit }
' "$TEMP_DIR/Cargo.lock")"
assert_equal "Soroban SDK version" "$(manifest_value dependency.soroban_sdk.version)" "$SDK_VERSION"
assert_equal "Soroban SDK checksum" "$(manifest_value dependency.soroban_sdk.checksum)" "$SDK_CHECKSUM"

RUSTC_DETAILS="$(rustc -Vv)"
assert_equal "rustc version" "$(manifest_value tool.rustc.version)" "$(awk '/^release:/ {print $2}' <<<"$RUSTC_DETAILS")"
assert_equal "rustc commit" "$(manifest_value tool.rustc.commit)" "$(awk '/^commit-hash:/ {print $2}' <<<"$RUSTC_DETAILS")"
assert_equal "rustc commit date" "$(manifest_value tool.rustc.commit_date)" "$(awk '/^commit-date:/ {print $2}' <<<"$RUSTC_DETAILS")"
assert_equal "rustc host" "$(manifest_value tool.rustc.host)" "$(awk '/^host:/ {print $2}' <<<"$RUSTC_DETAILS")"
assert_equal "rustc LLVM" "$(manifest_value tool.rustc.llvm_version)" "$(awk '/^LLVM version:/ {print $3}' <<<"$RUSTC_DETAILS")"
rustup target list --installed | grep -Fxq "$(manifest_value build.target)" || \
  fail "Rust target $(manifest_value build.target) is not installed"

CARGO_DETAILS="$(cargo -V)"
assert_equal "Cargo version" "$(manifest_value tool.cargo.version)" "$(awk '{print $2}' <<<"$CARGO_DETAILS")"
[[ "$CARGO_DETAILS" == *"($(manifest_value tool.cargo.commit) "* ]] || \
  fail "Cargo commit does not match the recorded provenance"

STELLAR_DETAILS="$(stellar version)"
assert_equal "Stellar CLI version" "$(manifest_value tool.stellar_cli.version)" "$(awk 'NR == 1 {print $2}' <<<"$STELLAR_DETAILS")"
[[ "$STELLAR_DETAILS" == *"($(manifest_value tool.stellar_cli.commit))"* ]] || \
  fail "Stellar CLI commit does not match the recorded provenance"
[[ "$STELLAR_DETAILS" == *"stellar-xdr $(manifest_value tool.stellar_xdr.version) ($(manifest_value tool.stellar_xdr.commit))"* ]] || \
  fail "Stellar XDR build does not match the recorded provenance"
[[ "$STELLAR_DETAILS" == *"xdr ($(manifest_value tool.xdr.commit))"* ]] || \
  fail "XDR commit does not match the recorded provenance"

while read -r expected wasm; do
  [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || fail "invalid SHA-256 entry for $wasm"
  [[ "$wasm" == "$(basename "$wasm")" ]] || fail "artifact name must be a basename: $wasm"
  [[ -f "$ARTIFACT_DIR/$wasm" ]] || fail "missing artifact $ARTIFACT_DIR/$wasm"
  assert_equal "$wasm SHA-256" "$expected" "$(hash_file "$ARTIFACT_DIR/$wasm")"
done <"$CHECKSUMS"

echo "Release provenance verified for $SOURCE_COMMIT"
echo "Artifacts: $ARTIFACT_DIR"
