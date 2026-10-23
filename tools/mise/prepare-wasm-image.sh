#!/usr/bin/env bash
set -euo pipefail

cargo build \
  --package calculator \
  --package weather \
  --package clock \
  --package pixel_painter \
  --package text_editor \
  --package synth \
  --target wasm32-wasip1 \
  --release

mkdir -p target/seed/wasm
rm -f target/seed/wasm/seed.img
cargo run --package fs_image -- target/seed/wasm/seed.img 16777216 \
  --bundle wasm \
  --applications-dir target/wasm32-wasip1/release
