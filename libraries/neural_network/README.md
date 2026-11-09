# Neural network VFS device

An optional `xila::neural_network` device backed by Nanoglia. Applications use ordinary
VFS open/write/control/read/close operations; no NN-specific VFS API is needed.

## Model ownership

`NeuralNetworkDevice::new(model, target)` borrows aligned ROM (`&'static [u8]` when mounted).
`NeuralNetworkDevice::from_owned_model(block, target)` takes an aligned
`nanoglia::core::Block<nanoglia::Global>` without copying its model bytes. Allocate the block
from the file size, then read the file directly into `block.as_mut_slice()` before loading.
Both constructors verify the model before it can be mounted. The device and open contexts
share one immutable graph through Xila's shared pointer, including on targets without pointer
atomics. The standard mounted-device lifecycle remains responsible for device cleanup.

Each open allocates an execution arena, input-completeness counters and the ordinary VFS
context object. Inference does not allocate; input/output tensors have no staging buffers.
ROM weights are not copied to RAM. Heap weights stay alive until their last graph owner drops.

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

`cmsis-nn` and `esp-nn` enable Nanoglia backends. The device always requires heap allocation;
`--no-default-features` disables the default Rust kernels/float/int16 features, not allocation.
