#!/usr/bin/env bash
# No allocator is supplied: successful final linking is the test, not cargo check.
set -euo pipefail
cd "$(dirname "$0")/../.."
manifest=tools/verification_probe/Cargo.toml
target=${1:-thumbv7em-none-eabi}
out="${CARGO_TARGET_DIR:-$PWD/target/verification-probe}/$target"
case "$target" in
  thumb*-none-eabi*) flags='["-C", "link-arg=--entry=probe"]' ;;
  wasm32-unknown-unknown) flags='["-C", "link-arg=--export=probe"]' ;;
  x86_64-unknown-linux-gnu) flags='["-C", "link-arg=-nostartfiles", "-C", "link-arg=-Wl,-e,probe", "-C", "link-arg=-lc"]' ;;
  *) echo "Unsupported link-probe target: $target" >&2; exit 2 ;;
esac

# Optional target-local SHA backend settings, never imposed by the core crate.
if [[ ${COMPACT_SHA2:-0} == 1 ]]; then
  flags=${flags%]}
  flags+=', "--cfg", "sha2_backend=\"soft\"", "--cfg", "sha2_backend_soft=\"compact\""]'
fi
for features in '' sha256 ed25519 sha256,ed25519 sha256,fast; do
  name=${features//,/-}
  name=${name:-empty}
  args=(--no-default-features)
  if [[ -n "$features" ]]; then args+=(--features "$features"); fi
  CARGO_TARGET_DIR="$out/$name" cargo build --locked --manifest-path "$manifest" \
    --release --target "$target" --config "target.$target.rustflags=$flags" "${args[@]}"
  binary="$out/$name/$target/release/verification_probe"
  if [[ "$target" == wasm* ]]; then binary+=.wasm; fi
  echo "profile=$name compact=${COMPACT_SHA2:-0} artifact=$binary"
  if [[ "$target" != wasm* ]] && command -v llvm-size >/dev/null; then llvm-size "$binary"; fi
done

# Positive control: adding an actual Box must fail compilation/linking.
log="$out/allocator-control.log"
if CARGO_TARGET_DIR="$out/control" cargo build --locked --manifest-path "$manifest" \
  --release --target "$target" --features allocator-control \
  --config "target.$target.rustflags=$flags" >"$log" 2>&1; then
  echo "ERROR: no-allocator positive control unexpectedly linked" >&2
  exit 1
fi
if ! grep -q 'no global memory allocator found' "$log"; then cat "$log"; exit 1; fi
echo 'Positive control rejected: allocation needs an allocator.'
