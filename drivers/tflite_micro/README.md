# Xila TFLite Micro backend

This optional backend adapts the pinned TensorFlow Lite Micro runtime to
`device::neural_network::NeuralNetworkDevice<()>`. Enable the `tflite_micro`
feature on the Xila root crate to include it; it is not enabled by default.

## Supported boundary

The current resolver is intentionally small and compiled from the upstream
reference kernels. It accepts one TFLite subgraph with one or more fixed-shape,
positive-dimension, per-tensor quantized `int8` tensors and the equal-shape
`ADD` operator (versions 1 and 2). Inputs and outputs are limited to 16 each,
tensor rank is limited to six, and unsupported options, broadcasting, model
metadata, sparse tensors, dynamic dimensions, and other operators are rejected.
The model validator is authoritative; only `I8` is advertised. `U8` is not
advertised because the compiled resolver and fixture do not provide an
end-to-end unsigned model.

The backend accepts only `ModelSource::Buffer` with `ModelFormat::Tflite`.
Model storage must be immutable, remain alive for the entire context lifetime,
and start at an 8-byte-aligned address. `load_model` validates the bytes and
borrows them without copying. Applications should pass one input `TensorSpec`
per model input to `create_context`, in model input-index order. Each spec must
exactly match the model's `I8` shape and per-tensor scale/zero point. A compute
call supplies exactly one binding per model input, with matching metadata and
`TensorData::I8` values. Input tensors are copied into TFLM's arena, so
`zero_copy` is false.

## Workspace and lifecycle

Call `memory_requirements` with the TFLite buffer, allocate at least the
reported `workspace_bytes`, and provide a base address aligned to the reported
alignment (at least 16 bytes). The size is model-specific and includes the
placement-constructed context, a conservative arena bound, and alignment
padding. The model's storage is borrowed, so `model_bytes` is zero. The backend
does not allocate memory for model, context, or inference storage.

The generic `Context` keeps the model and exclusive workspace borrowed until
it is destroyed. Call `destroy_context` exactly once for each successfully
created context, then `unload_model`. Outputs are borrowed views into the
workspace and remain valid until the next compute attempt or context
destruction. Any failed compute attempt invalidates earlier outputs.

## Build and provenance

The crate vendors the minimal required source subset of TensorFlow Lite Micro
revision `64b3de836daf42ef5094d06035742c058e7638f6` and FlatBuffers revision
`187240970746d00bbd26b0f5873ed54d2477f9f3`, including the upstream Apache-2.0
license files. `vendor/tflite-micro/MANIFEST.md` records the source inventory,
hashes, licenses, resolver, and model fixture provenance. Cargo does not fetch
runtime source during builds. Host tests compile the reference kernels as
C++17 and require a working C++ compiler.

CMSIS-NN optimized kernels and a board/toolchain smoke test remain follow-up
work. This first backend slice has only been exercised on a GNU x86_64 Linux
host; it does not claim MCU build or hardware validation.
