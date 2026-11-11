# Neural network VFS device

A portable VFS adapter, exported as `xila::drivers_neural_network` by the optional `neural_network` feature.
The ABI is defined in `device::neural_network`; the lower-half traits are
`neural_network::Backend` and `neural_network::Loader`. Applications use ordinary VFS
open/write/control/read/close operations. This crate has no inference-engine or accelerator dependency.

## Device types and ownership

### Fixed-model devices (`NeuralNetworkDevice<B>`)

Implement `Backend` for a prepared immutable model and wrap it with
`NeuralNetworkDevice::new(backend)`. The backend owns model lifetimes and creates independent
execution storage per open. Input and output associated views access the execution/model
storage directly. The adapter stores its concrete backend and execution type by value;
there is no additional backend box, vtable, model copy or staging tensor buffer.
Fixed models mounted at boot (via `mount_rom` / `from_rom` or `from_owned_model`) remain supported.

### Runtime loader devices (`LoaderDevice<L>`)

Implement `Loader` for a backend capable of parsing models at runtime, and wrap it with
`LoaderDevice::new(loader)`. A platform mounts the loader node once (for example at
`/devices/nn/cpu`). Each open handle maintains independent per-open state:
`Staging { bytes }` transitions to `Ready { backend, device_context }` after a successful `LOAD`.

The Nanoglia implementation lives in Platforms' `drivers/shared`, behind its optional
`neural-network` feature. Runtime assemblies choose accelerator features, construct ROM,
owned-file models, or mount dynamic loader nodes. Core owns only this protocol and adapter.

Per open, this adapter allocates its ordinary VFS context box and input-completeness counters.
The backend allocates its execution arena once. Repeated write/compute/read calls do not
allocate in the adapter. Backend methods must preserve that property if required by a target.
Model retention and shared ownership are backend responsibilities.

## VFS protocols

### Fixed-model inference protocol

1. Query `GET_INPUT_COUNT`, `GET_INPUT_INFO` (and quantization commands as needed).
2. Select each input using `SET_INPUT_INDEX`. Seek the file cursor to zero, then write
   the complete tensor in contiguous chunks. Gaps are rejected; rewrites within the filled
   prefix are supported. Tensor bytes use Nanoglia's raw little-endian representation.
3. Issue `COMPUTE`. Every input must be complete. Inference consumes input completeness,
   including on kernel failure, because the arena may reuse input storage.
4. Select an output with `SET_OUTPUT_INDEX`, seek to zero and read its bytes. Output is
   available only after successful compute and until a successful write or next compute.
5. Supply all inputs again for another inference. Close releases this context's arena.

### Runtime loading protocol (`LoaderDevice`)

1. Open the loader node (e.g. `/devices/nn/cpu`). The handle starts in the staging state.
2. Write the raw model FlatBuffer bytes sequentially starting from position 0. Writes must be
   contiguous; gaps return `InvalidParameter`. Writes exceeding `maximum_model_size()` are
   rejected with `FileTooLarge`.
3. Issue the `LOAD` control command with a `LoadRequest` payload (`version = 1`, `encoding = 0`
   for TensorFlow Lite).
4. Upon successful loading, the handle transitions to ready state and thereafter functions
   identically to a `NeuralNetworkDevice` handle (steps 1–5 of the inference protocol).
5. Issuing inference commands before `LOAD` returns `InvalidContext`. Issuing a second `LOAD`
   on a ready handle returns `InvalidContext`.
6. Closing the file descriptor deallocates both the model storage and the execution arena.

Commands return `TensorInfo`, `QuantizationRequest` and `QuantizationEntry` as appropriate.
Selection does not reset the VFS cursor. Duplicate is unsupported; open the device again for
an independent execution arena. CPU inference is synchronous and returns `Poll::Ready`;
it does not claim to yield between operators or to validate hardware accelerator execution.

The version-one `TensorInfo` layout supports up to six dimensions and retains the
TensorFlow Lite numeric tensor-type codes for compatibility. Backends translate their
metadata into that layout. The protocol and command identifiers remain stable across engines.
