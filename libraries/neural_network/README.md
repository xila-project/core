# Neural network backend interface

This portable, `no_std` library defines the lower-half inference interfaces:
- [`Backend`]: represents a prepared, immutable model with independent execution storage per open handle.
- [`Loader`]: represents a model loader engine capable of instantiating a [`Backend`] at runtime from an aligned [`ModelBuffer`].

It has no VFS adapter or inference-engine dependency. ABI types and control command identifiers are defined in
`device::neural_network`; generic VFS upper halves live in
[`drivers/neural_network`](../../drivers/neural_network/README.md).

## Model Lifetimes and Execution

Backends own model lifetimes and execution storage. Tensor views access that storage
directly; implementing the trait adds no allocation or dynamic dispatch.

Platforms can either:
- Mount fixed, immutable models at boot (e.g. `mount_rom` / `from_rom` or `from_owned_model`), or
- Mount a dynamic loader node (e.g. `/devices/nn/cpu`) wrapping a [`Loader`] implementation, allowing applications to stage and load their own models at runtime.

[`ModelBuffer`] ensures 16-byte alignment as required by TensorFlow Lite FlatBuffers for zero-copy parsing.
