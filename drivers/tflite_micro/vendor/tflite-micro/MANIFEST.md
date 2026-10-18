# Vendored native runtime source and license manifest

The files below are an **unmodified** subset of upstream repositories. File paths
are relative to this directory. No build step downloads, generates, or patches
these sources. `build.rs` is the explicit compiled translation-unit list.

| Component | Source revision | License file |
| --- | --- | --- |
| TensorFlow Lite Micro | https://github.com/tensorflow/tflite-micro/tree/64b3de836daf42ef5094d06035742c058e7638f6 | `LICENSE` (Apache-2.0) |
| FlatBuffers | https://github.com/google/flatbuffers/tree/187240970746d00bbd26b0f5873ed54d2477f9f3 (tag `v25.9.23`, the version required by this TFLM pin) | `third_party/flatbuffers/LICENSE` (Apache-2.0) |

Retain each source file's copyright/license notice. No upstream FlatBuffers patch
was applied: the runtime only reads buffers, never constructs a FlatBuffer, and
uses `FLATBUFFERS_LOCALE_INDEPENDENT=0`. The fixture generator below intentionally
uses the upstream builder outside the runtime build.

## Selection and build

Only runtime allocation/interpreter dependencies and the upstream reference ADD
kernel are compiled. The compiler's `g++ -MM` dependency closure of these files
and `src/tflm_wrapper.cc` selected the retained headers. An archive link map was
used to remove three unused translation units (`internal/common.cc`,
`internal/runtime_shape.cc`, and `internal/tensor_ctypes.cc`). Whole upstream
translation units are preserved, including unused functions; Cargo's normal
function-section garbage collection removes those functions at link time. A
standalone C++ consumer must also link with section garbage collection (GNU:
`-Wl,--gc-sections`). CMSIS-NN, other optimized kernels, examples and tests are
excluded. No gemmlowp or ruy dependency is needed at this revision.

Cargo uses `cc` with C++17, `-fno-exceptions`, `-fno-rtti`,
`TF_LITE_STATIC_MEMORY`, and `TF_LITE_STRIP_ERROR_STRINGS` on the verified GNU host.
Only building this optional workspace crate invokes its C++ compiler. The first
backend is verified on x86_64 Linux; MCU builds and hardware remain follow-up.

## Private ABI contract and current model subset

The ABI is internal to the Rust crate. Its status/type integers are defined in
`src/tflm_wrapper.h` and mirrored in `src/ffi.rs`; they do not expose TfLite enums.
Callers supply valid pointer/length pairs, 8-byte aligned immutable model bytes,
and exclusive 16-byte aligned workspace. Model and workspace must not overlap.
Both remain borrowed until exactly one destruction of a successfully created
context. An output view stays valid only until input changes, invocation or
destruction; a context must never be accessed concurrently.

Workspace layout is `[placement Context][16-byte aligned TFLM tensor arena]`.
`context_size()` includes context alignment padding; `context_alignment()` is 16.
`arena_used` reports context plus actual TFLM arena use after successful creation,
not the input workspace capacity. Before construction, a conservative arena
capacity preflight requires `4096 + 512*tensor_count + 1024*operator_count +
sum(2*tensor_element_count + 16)` bytes in addition to context storage. This
protects unchecked placement construction in upstream allocator bootstrap and
kernel init for small workspaces; it deliberately exceeds exact usage. No
wrapper allocation or inference-time allocation occurs.

Supported models have schema version 3, one graph, inline buffers, 1–16 inputs
and outputs, at most 256 tensors / 128 operators, fixed positive equal shapes
for each ADD, per-tensor affine int8 quantization with finite positive scale,
and ADD opcode version 1 or 2. Activations NONE, RELU, RELU6 and RELU_N1_TO_1 are
accepted. Invalid indices, buffer lengths and dataflow are rejected before TFLM.
Variables, sparsity, dynamic shapes, broadcasting, custom options, metadata,
signatures and other operators/types are rejected. ABI type values reserve u8,
but this pinned reference ADD has no u8 implementation. The metadata ABI allows
an eight-element dimensions buffer; upstream `RuntimeShape` limits this resolver
to rank six, and ranks seven/eight are explicitly rejected. Scale combinations
that violate upstream ADD's output multiplier constraint are rejected.

Every input must be initialized before invoke. Output is available only after a
successful invocation and becomes unavailable on every attempted input update or
invocation, including failure. C API output-data errors clear returned pointer,
type and size.

## Deterministic fixture generation

`tests/data/quantized_add.tflite` is an original Xila test fixture (Apache-2.0),
not an upstream model. It uses scale 1.0 and zero point 0 on three int8 tensors,
shape `[1,4]`, input plus constant `[1,2,3,4]`, ADD v2, activation NONE. Input
`[-3,0,4,10]` produces exact output `[-2,2,7,14]`.

To reproduce, save this C++ program to `work/generate_fixture.cc` and run from the
repository root (the generator is not part of Cargo builds):

```sh
g++ -std=c++17 -Idrivers/tflite_micro/vendor/tflite-micro \
  -Idrivers/tflite_micro/vendor/tflite-micro/third_party/flatbuffers/include \
  work/generate_fixture.cc -o work/generate_fixture
work/generate_fixture
```

```cpp
#include <fstream>
#include "tensorflow/lite/micro/schema/schema_generated.h"
int main() {
  tflite::ModelT model;
  model.version = 3;
  model.description = "Xila deterministic int8 ADD: input + [1,2,3,4], scale 1, zero point 0";
  auto opcode = std::make_unique<tflite::OperatorCodeT>();
  opcode->builtin_code = tflite::BuiltinOperator_ADD;
  opcode->deprecated_builtin_code = tflite::BuiltinOperator_ADD;
  opcode->version = 2;
  model.operator_codes.push_back(std::move(opcode));
  model.buffers.push_back(std::make_unique<tflite::BufferT>());
  auto constant = std::make_unique<tflite::BufferT>();
  constant->data = {1,2,3,4};
  model.buffers.push_back(std::move(constant));
  auto graph = std::make_unique<tflite::SubGraphT>();
  for (int i = 0; i < 3; ++i) {
    auto tensor = std::make_unique<tflite::TensorT>();
    tensor->shape = {1,4};
    tensor->type = tflite::TensorType_INT8;
    tensor->buffer = i == 1 ? 1 : 0;
    tensor->quantization = std::make_unique<tflite::QuantizationParametersT>();
    tensor->quantization->scale = {1.0f};
    tensor->quantization->zero_point = {0};
    graph->tensors.push_back(std::move(tensor));
  }
  auto op = std::make_unique<tflite::OperatorT>();
  op->inputs = {0,1};
  op->outputs = {2};
  op->builtin_options.Set(tflite::AddOptionsT{});
  graph->operators.push_back(std::move(op));
  graph->inputs = {0};
  graph->outputs = {2};
  model.subgraphs.push_back(std::move(graph));
  flatbuffers::FlatBufferBuilder builder;
  auto offset = tflite::Model::Pack(builder, &model);
  tflite::FinishModelBuffer(builder, offset);
  std::ofstream out("drivers/tflite_micro/tests/data/quantized_add.tflite", std::ios::binary);
  out.write(reinterpret_cast<const char*>(builder.GetBufferPointer()), builder.GetSize());
}
```

Fixture SHA-256: `7f2a29a37f84b4d35702e4d70b72a2078fb23674a2b58d14eab73176f6b4722b`.

## Exact source inventory (SHA-256)

| Path | SHA-256 |
| --- | --- |
| `LICENSE` | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` |
| `tensorflow/lite/micro/arena_allocator/ibuffer_allocator.h` | `6586f5d8eea4d9094fd7b582b7fcc49378972e2b3df84185916ae07df7294f7b` |
| `tensorflow/lite/micro/arena_allocator/non_persistent_arena_buffer_allocator.cc` | `ee599d4867373634ab5da5e4842e793fe42f05c434dfe3ecb37d383e10586514` |
| `tensorflow/lite/micro/arena_allocator/non_persistent_arena_buffer_allocator.h` | `a4a0961425b20e6110beaad37c6d80b3b7e7a3a879531b7a9c868caba7d6ee9a` |
| `tensorflow/lite/micro/arena_allocator/persistent_arena_buffer_allocator.cc` | `bac7a1461bb19482f598b3af1b1338bb7155ccb02a75570047f811af64b38ac8` |
| `tensorflow/lite/micro/arena_allocator/persistent_arena_buffer_allocator.h` | `ac3b1640739ea00113c11697a91b29d4714b974c7b366e6652e381c9ca5e640d` |
| `tensorflow/lite/micro/arena_allocator/single_arena_buffer_allocator.cc` | `d9fb253f3f46865db411b402c41ae609dd035d9b88b8f80b676dd3e53ea2b249` |
| `tensorflow/lite/micro/arena_allocator/single_arena_buffer_allocator.h` | `a5a314e5db64a76b7800cd490981cb551c63e7b0d6164c13c85c0590c1d5f483` |
| `tensorflow/lite/micro/builtin_op_data.h` | `8fc019e19c3501bd6ae645064faef9837c143b26c85f5d89a22321ab88330e03` |
| `tensorflow/lite/micro/compatibility.h` | `c6f9ec2dfb70af1f1138feece2d80faf74e07bf9144a58c5ba36e9eeb921b4e8` |
| `tensorflow/lite/micro/compression.h` | `5a4b282afee49d88aa9cd50c7d7c98a543a0aab0ef676e1d5ec600be56dc517c` |
| `tensorflow/lite/micro/flatbuffer_conversions.cc` | `e185acf03298d3367cd4a7b61fbfd705f20503621b0f42f1b894ed9035c774cb` |
| `tensorflow/lite/micro/flatbuffer_conversions.h` | `a21737229ae0ad49e0f755b80f6200292b07df51e9f02dea8a07e0285713c938` |
| `tensorflow/lite/micro/flatbuffer_utils.cc` | `df40af7d06c93fcee03396976f9692ed7c0e1573e3bdb50dcf1b30e9e76140c0` |
| `tensorflow/lite/micro/flatbuffer_utils.h` | `bdcdd512031ee42844b000ce6aeb6cec596167e14a76b67b88bbc5b372c2410a` |
| `tensorflow/lite/micro/kernels/add.cc` | `09786b6ef3e06140317767908b9db7c59c03e76b9fa544fe3710bff199d40478` |
| `tensorflow/lite/micro/kernels/add.h` | `1a1b79ed412ebed8683f04ee8d9b34395bad57ac471fb1879469ce7cf157465c` |
| `tensorflow/lite/micro/kernels/add_common.cc` | `749092ff935785c6911d07170affa2817de5a2f60fcd4841bd6c1d0e46b6982f` |
| `tensorflow/lite/micro/kernels/decompress.h` | `dbe0b22e045fe011c9615fba9f59a9120c7ac7f1ffa58b503a4ff59c522eed51` |
| `tensorflow/lite/micro/kernels/internal/common.h` | `41343398489de14acfc966fb6e4cd2a161aeadc1c0c11fe224aaf796693a0b10` |
| `tensorflow/lite/micro/kernels/internal/cppmath.h` | `7a4eebbb914018ae43dc4fae834d521b42cfb793d03fccf90fb94a8beccbc648` |
| `tensorflow/lite/micro/kernels/internal/fixedpoint.h` | `c19cb30b01ae9452c434140371502a7891bcc07a85f3d30a262e0444a11dc189` |
| `tensorflow/lite/micro/kernels/internal/portable_tensor_utils.h` | `6a28fabdedfe6230228bc6a125cb6ad758246dabf5f34249e0372c71cb78444c` |
| `tensorflow/lite/micro/kernels/internal/quantization_util.cc` | `75dfd7469c4d808753562064624e4c919db7799b0698e8f36febd550dd4b3c97` |
| `tensorflow/lite/micro/kernels/internal/quantization_util.h` | `2460f5530ac0e7a6064fcec0111101fec4211fb7f613e8abf6a02e84f5a74d86` |
| `tensorflow/lite/micro/kernels/internal/reference/add.h` | `b4fdffb5eb644484b441908f5aa83fb9f4e01ea15207bf9e983c691c19f73cca` |
| `tensorflow/lite/micro/kernels/internal/reference/broadcast_loop.h` | `af07d5cce402ea8ca59cd2c341cf4c1c7441153823f505204879b4ce5b81bd22` |
| `tensorflow/lite/micro/kernels/internal/reference/integer_ops/add.h` | `a047c256e389c6073b465e832fdf66d2e0059b2c3714d2ae738a5ccd54e2c61d` |
| `tensorflow/lite/micro/kernels/internal/reference/process_broadcast_shapes.h` | `f427c93f4b152514cb5994a4f61f0373b265130c1a24f4bc52f2226ba3e19af4` |
| `tensorflow/lite/micro/kernels/internal/runtime_shape.h` | `ace53bea7c2eb9c6e025dc2cd7b23d42977ce3860cbd9018c4c3bbe6de47b92c` |
| `tensorflow/lite/micro/kernels/internal/tensor_ctypes.h` | `912bb7e58bcec646e429c6327cd3e7b0542de6c880992b4e5c40b726a9cba678` |
| `tensorflow/lite/micro/kernels/internal/types.h` | `aa30efcc27f0933f2005e211cc098bd63f821ae65e8f228abfd3361804e1110f` |
| `tensorflow/lite/micro/kernels/kernel_util.cc` | `8b116013a3e9b50c697d987588796774efb66eb3fa4e60676dd963c9aafac95f` |
| `tensorflow/lite/micro/kernels/kernel_util.h` | `37ad3b645943454798dba7d11e7149648cb0536c07f98de1f51bc82eba978a4f` |
| `tensorflow/lite/micro/memory_helpers.cc` | `84fe92cd9e0eb12028fde1a269386d7a4bd3372fd2d4795af5e6b127a6fe8fa3` |
| `tensorflow/lite/micro/memory_helpers.h` | `9b08e4a5d5bc36a0e335b6f8c3171ef6a8fb143276c8191f199e24653300198f` |
| `tensorflow/lite/micro/memory_planner/greedy_memory_planner.cc` | `69657905f4def9504acc81f53f16463f9fc5a16ad841c1f39015ae29803d5b32` |
| `tensorflow/lite/micro/memory_planner/greedy_memory_planner.h` | `2e16b7ad814160caa9b13769a62641ab6c369011a6f3d8ec66ccbbb21ae09fd7` |
| `tensorflow/lite/micro/memory_planner/linear_memory_planner.cc` | `19c3dc9e5ab4982ae2a7fdabb82be5ddd8aa71125d52ccea4741d69408351a8d` |
| `tensorflow/lite/micro/memory_planner/linear_memory_planner.h` | `6024795915b097161d9d2f6c1f9049631199fbab39571e8c21741cded07f61a6` |
| `tensorflow/lite/micro/memory_planner/micro_memory_planner.h` | `a726331f339ac271a3330e44a17a7a7ef0912f1b72f5b341614b59cb448e9b50` |
| `tensorflow/lite/micro/micro_allocation_info.cc` | `f0668c46721e37884cc2b4457ab42a0cf61c7a69fa27ba9c3642b136b470f1f9` |
| `tensorflow/lite/micro/micro_allocation_info.h` | `8cc9fe50ed25b75048e5c278f497eb4376503fc0f3a2e93eaf101ac1d9a1f2ad` |
| `tensorflow/lite/micro/micro_allocator.cc` | `e9b004bed4b6e68de84f8dfeb9e711a6fb43013da8896e0927a444dd7b13c24c` |
| `tensorflow/lite/micro/micro_allocator.h` | `11bb3d2f4315608ebc62f68d2db22146d0e33002b352b492c75755e7352b4405` |
| `tensorflow/lite/micro/micro_arena_constants.h` | `3286cc071482625d20796da6f9ac6c6a846d282ae3b9ad35c8cd76bc138152b9` |
| `tensorflow/lite/micro/micro_common.cc` | `c42383770195037c89c37f14913e1569b281442e39b955cec2b7775b839246ba` |
| `tensorflow/lite/micro/micro_common.h` | `8776c46a74d8959d6e9543f2d52b3544c537f93942c4b7312828ba5ac57a33db` |
| `tensorflow/lite/micro/micro_context.cc` | `a3a69b12e3e6dd6a869696935ac8624dbb1ce999591739961564d8e5628b05f0` |
| `tensorflow/lite/micro/micro_context.h` | `957a144d99007a5a98ac252511208130898cbf482da16e3500392dc3d0cf05a4` |
| `tensorflow/lite/micro/micro_graph.h` | `e52173912b8d113bb7a955152e21fa75267232f9c294ede9548e3b0087f7b45b` |
| `tensorflow/lite/micro/micro_interpreter.cc` | `ad59383b1ebbc7fcfdf6c559601a1c9904f836cb002d41a406fa6a14934c972f` |
| `tensorflow/lite/micro/micro_interpreter.h` | `77ea0e18b88c108cea7eb72cb79437be7bb01cb1a1ac3eee1588645daaea0e81` |
| `tensorflow/lite/micro/micro_interpreter_context.cc` | `f8b0b932f1acb484f7d9588c638e0125ab1d00f69c4ca23c61b867d573ba100f` |
| `tensorflow/lite/micro/micro_interpreter_context.h` | `7db248f661202b94421f3a07eead964e35150e3a618243917d92c5b1ce3d904f` |
| `tensorflow/lite/micro/micro_interpreter_graph.cc` | `39c333e53e1fafef4b1b2a47596b0e2e8231706a5dcf9501c1a55501db4171c8` |
| `tensorflow/lite/micro/micro_interpreter_graph.h` | `34993bf88033a0f6f5910b9ee33c3b1d6250e93df7d311baf7974feca1901de1` |
| `tensorflow/lite/micro/micro_log.h` | `892c5bee78a8e0a13b3ef5c335d4cca9b5fec62163c8487b73f1579798dffd61` |
| `tensorflow/lite/micro/micro_op_resolver.cc` | `8626f48ea0a5220f0ba127152ed548647b844150e35b8942be86b3df8ee2c012` |
| `tensorflow/lite/micro/micro_op_resolver.h` | `d8a8bbbb2fdcb96c2c4aa82c080ee4641373d7514e2b61d91de6dc5b7e559d09` |
| `tensorflow/lite/micro/micro_profiler.h` | `bfda31904cf3f677c12eea9b4b27292e42dccde3c00aa7cbf4be8fecaea176b7` |
| `tensorflow/lite/micro/micro_profiler_interface.h` | `8a2178165c979c4f15c42d007e0ea781377ce33523f0d51e39cb6b72fc1f47e3` |
| `tensorflow/lite/micro/micro_resource_variable.cc` | `ab4e492267209e06c73d89f48eb53784c1efc03ae63d805c4c236d8fa5d0b07b` |
| `tensorflow/lite/micro/micro_resource_variable.h` | `9d961ad553211f04beb5cc18233439f45891492f86e6a9147fe0aa4e733f9e92` |
| `tensorflow/lite/micro/micro_utils.cc` | `9e9ffaa685c21a560b0b5241a3d1f75c2172f406ae5cc24d9975866a14e241c5` |
| `tensorflow/lite/micro/micro_utils.h` | `5f0c6e83543be8e3f2b82def8da2e67dda3309ede164455e12e6d9d6f03b91f6` |
| `tensorflow/lite/micro/portable_type_to_tflitetype.h` | `31279389a3e79a189c57ec8ecc463002cc474b8d03f495a01df303b684570a91` |
| `tensorflow/lite/micro/schema/schema_generated.h` | `c1ac4bd891a75715514a823e635048c96c98e76a0b80f83f22ea270b0f195a62` |
| `third_party/flatbuffers/LICENSE` | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` |
| `third_party/flatbuffers/include/flatbuffers/allocator.h` | `9ac3f3659ff1e5ff1b2be6166f08490d16f67b1e87d2e70ef87c3fd5d2689caf` |
| `third_party/flatbuffers/include/flatbuffers/array.h` | `124aa84b1afab74b0e0b4b01989d154aef1d27e210ae7a20148f2932819b94c8` |
| `third_party/flatbuffers/include/flatbuffers/base.h` | `5aa62f7124a1fc0788e9bedd3a95c7b9ddc634adedb9d0529704503a29bc6739` |
| `third_party/flatbuffers/include/flatbuffers/buffer.h` | `8506628aa30e1c4768079eaceb4e46292ecae82b6adf6513302d3b5316547f6c` |
| `third_party/flatbuffers/include/flatbuffers/buffer_ref.h` | `db4c627e3bfb728273ea36e65fa0f97c70c9911abc8799797893f986f0c68286` |
| `third_party/flatbuffers/include/flatbuffers/default_allocator.h` | `9532a571b57f5f0c1763fe6f6f90121b46ce20a8f3d6b53b22645bb8f2716c7f` |
| `third_party/flatbuffers/include/flatbuffers/detached_buffer.h` | `d34758c25713bebc10e31f4a591d38485413f16942b3650245b3371a0cdcd4cf` |
| `third_party/flatbuffers/include/flatbuffers/flatbuffer_builder.h` | `564db1ad19a98cb3197471393f038411ae69464d9a8ab1a3177f444a3931b305` |
| `third_party/flatbuffers/include/flatbuffers/flatbuffers.h` | `0d0edc1a6c9dd1ccedc1c0ac2d144b7c04e829f76bb99ab007ab5631ffa96ecd` |
| `third_party/flatbuffers/include/flatbuffers/flexbuffers.h` | `0b4285d26ef7ed519b5abea4e75c7dd14a43741ef4824479e6a2f9c3d9372a31` |
| `third_party/flatbuffers/include/flatbuffers/stl_emulation.h` | `a6a8033b4738e18b1e0fc0fb4a1e18b446d25799b9769e6d20ffe25395051882` |
| `third_party/flatbuffers/include/flatbuffers/string.h` | `cc2f7d40f2d5613a0932e63d686aea68cde79f8eb4370476361b219b979f5a53` |
| `third_party/flatbuffers/include/flatbuffers/struct.h` | `7efdabf43a53fd2a96382b08745c58832e80be9606c1e8c0b62849e56c5f8e9f` |
| `third_party/flatbuffers/include/flatbuffers/table.h` | `e351a1a3ab02c57b2d10f453b773d4d6ed84110595897f2320fa29ef9e151199` |
| `third_party/flatbuffers/include/flatbuffers/util.h` | `5f0feaeabb6f807801cc232628646199f5c199a86f3150a696376dbe6f3cd836` |
| `third_party/flatbuffers/include/flatbuffers/vector.h` | `4fba123f88cb23e4a9d0c065b017226070c87d3594a7b579f29d529536cb2180` |
| `third_party/flatbuffers/include/flatbuffers/vector_downward.h` | `7f687fdb661f4c1be09223d6e416e042b139ff1748b5a215cb9864c56f9f3be2` |
| `third_party/flatbuffers/include/flatbuffers/verifier.h` | `d7fbf583019088983af684e7d00cbfda23b759093c6501f850c1c33400559b5a` |
