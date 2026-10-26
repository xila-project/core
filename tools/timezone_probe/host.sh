#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
manifest=tools/timezone_probe/Cargo.toml
base="${CARGO_TARGET_DIR:-$PWD/target}/timezone-probe/native"
for profile in empty utc paris bundled; do
  features=hosted
  if [[ "$profile" != empty ]]; then features+=,$profile; fi
  CARGO_TARGET_DIR="$base/$profile" cargo build --locked --manifest-path "$manifest" \
    --release --bin timezone-probe-host --no-default-features --features "$features"
  binary="$base/$profile/release/timezone-probe-host"
  echo "profile=$profile artifact=$binary"
  "$binary"
  size "$binary"
done
