#!/usr/bin/env bash
# Verify the extracted shared Rust crate with all feature profiles.
set -euo pipefail
cd "$(dirname "$0")/../.."
for features in '' sha256 ed25519 sha256,ed25519; do
  args=(--locked --no-default-features)
  if [[ -n "$features" ]]; then args+=(--features "$features"); fi
  cargo test -p verification "${args[@]}"
  cargo clippy -p verification --all-targets "${args[@]}" -- -D warnings
  for target in ${VERIFICATION_TARGETS:-wasm32-unknown-unknown wasm32-wasip1 thumbv7em-none-eabi}; do
    cargo check -p verification --target "$target" "${args[@]}"
  done
done
RUSTDOCFLAGS='-D warnings' cargo doc --locked -p verification --all-features --no-deps
