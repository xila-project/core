#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
for features in '' zone-europe-paris zone-america-new-york zone-asia-kathmandu zone-asia-kolkata zone-australia-lord-howe bundled bundled,runtime-clock; do
  args=(--no-default-features)
  if [[ -n "$features" ]]; then args+=(--features "$features"); fi
  cargo test --locked -p xila-timezone "${args[@]}"
  cargo clippy --locked -p xila-timezone --all-targets "${args[@]}" -- -D warnings
done
for target in ${TIMEZONE_TARGETS:-wasm32-unknown-unknown wasm32-wasip1 thumbv7em-none-eabi}; do
  cargo check --locked -p xila-timezone --target "$target" --no-default-features
  cargo check --locked -p xila-timezone --target "$target" --no-default-features --features bundled
done
cargo check --locked -p xila --features timezone-clock
cargo fmt -p xila-timezone -- --check
cargo fmt --manifest-path tools/timezone_probe/Cargo.toml -- --check
RUSTDOCFLAGS='-D warnings' cargo doc --locked -p xila-timezone --all-features --no-deps
