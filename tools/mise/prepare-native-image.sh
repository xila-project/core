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
  --profile release-wasm

mkdir -p target/seed/native
rm -f target/seed/native/seed.img
cargo run --package fs_image -- target/seed/native/seed.img 16777216 \
  --bundle native \
  --applications-dir target/wasm32-wasip1/release-wasm
