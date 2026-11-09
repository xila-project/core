# Neural network backend interface

This portable, `no_std` library defines `Backend`, the lower-half inference interface.
It has no VFS adapter or inference-engine dependency. ABI types are defined in
`device::neural_network`; the generic VFS upper half lives in
[`drivers/neural_network`](../../drivers/neural_network/README.md).

Backends own model lifetimes and execution storage. Tensor views access that storage
directly; implementing the trait adds no allocation or dynamic dispatch.
Platforms selects and constructs Nanoglia backends and mounts their devices.
