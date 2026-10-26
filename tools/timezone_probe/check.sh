#!/usr/bin/env bash
# Final link with no allocator; includes a deliberately allocating positive control.
set -euo pipefail
cd "$(dirname "$0")/../.."
manifest=tools/timezone_probe/Cargo.toml
target=${1:-thumbv7em-none-eabi}
base="${CARGO_TARGET_DIR:-$PWD/target}/timezone-probe/$target"
case "$target" in
  thumb*-none-eabi*) flags='["-C", "link-arg=--entry=probe"]' ;;
  wasm32-unknown-unknown) flags='["-C", "link-arg=--export=probe"]' ;;
  *) echo "Unsupported no-allocator link target: $target" >&2; exit 2 ;;
esac
for profile in empty utc paris bundled; do
  args=(--no-default-features)
  if [[ "$profile" != empty ]]; then args+=(--features "$profile"); fi
  CARGO_TARGET_DIR="$base/$profile" cargo build --locked --manifest-path "$manifest" \
    --release --bin timezone-probe --target "$target" \
    --config "target.$target.rustflags=$flags" "${args[@]}"
  binary="$base/$profile/$target/release/timezone-probe"
  if [[ "$target" == wasm* ]]; then binary+=.wasm; fi
  echo "profile=$profile artifact=$binary"
  if [[ "$target" != wasm* ]] && command -v llvm-size >/dev/null; then llvm-size "$binary"; fi
  if [[ "$target" == wasm* ]]; then python3 tools/timezone_probe/wasm_size.py "$binary"; fi
done
log="$base/allocator-control.log"
if CARGO_TARGET_DIR="$base/control" cargo build --locked --manifest-path "$manifest" \
  --release --bin timezone-probe --target "$target" --features bundled,allocator-control \
  --config "target.$target.rustflags=$flags" >"$log" 2>&1; then
  echo 'ERROR: allocating positive control unexpectedly linked' >&2; exit 1
fi
if ! grep -q 'no global memory allocator found' "$log"; then cat "$log"; exit 1; fi
echo 'Positive control rejected: allocation needs an allocator.'
