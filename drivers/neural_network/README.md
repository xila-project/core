# Neural network VFS device

A portable VFS adapter, exported as `xila::neural_network_driver` by the optional `neural_network` feature.
The ABI is defined in `device::neural_network`; the lower-half trait is
`neural_network::Backend`. Applications use ordinary VFS open/write/control/read/close operations. This crate has
no inference-engine or accelerator dependency.

## Backend and ownership

Implement `Backend` for a prepared immutable model and wrap it with
`NeuralNetworkDevice::new(backend)`. The backend owns model lifetimes and creates independent
execution storage per open. Input and output associated views access the execution/model
storage directly. The adapter stores its concrete backend and execution type by value;
there is no additional backend box, vtable, model copy or staging tensor buffer.

The Nanoglia implementation lives in Platforms' `drivers/shared`, behind its optional
`neural-network` feature. Runtime assemblies choose accelerator features, construct ROM or
owned-file models, and register devices there. Core owns only this protocol and adapter.

Per open, this adapter allocates its ordinary VFS context box and input-completeness counters.
The backend allocates its execution arena once. Repeated write/compute/read calls do not
allocate in the adapter. Backend methods must preserve that property if required by a target.
Model retention and shared ownership are backend responsibilities.

## VFS protocol

1. Query `GET_INPUT_COUNT`, `GET_INPUT_INFO` (and quantization commands as needed).
2. Select each input using `SET_INPUT_INDEX`. Seek the file cursor to zero, then write
   the complete tensor in contiguous chunks. Gaps are rejected; rewrites within the filled
   prefix are supported. Tensor bytes use Nanoglia's raw little-endian representation.
3. Issue `COMPUTE`. Every input must be complete. Inference consumes input completeness,
   including on kernel failure, because the arena may reuse input storage.
4. Select an output with `SET_OUTPUT_INDEX`, seek to zero and read its bytes. Output is
   available only after successful compute and until a successful write or next compute.
5. Supply all inputs again for another inference. Close releases this context's arena.

Commands return `TensorInfo`, `QuantizationRequest` and `QuantizationEntry` as appropriate.
Selection does not reset the VFS cursor. Duplicate is unsupported; open the device again for
an independent execution arena. CPU inference is synchronous and returns `Poll::Ready`;
it does not claim to yield between operators or to validate hardware accelerator execution.

The version-one `TensorInfo` layout supports up to six dimensions and retains the
TensorFlow Lite numeric tensor-type codes for compatibility. Backends translate their
metadata into that layout. The protocol and command identifiers remain stable across engines.
