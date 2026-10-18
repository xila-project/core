#include "tflm_wrapper.h"

#include <cmath>
#include <cstring>
#include <limits>
#include <new>

#include "tensorflow/lite/micro/kernels/add.h"
#include "tensorflow/lite/micro/micro_interpreter.h"

namespace {
constexpr size_t kAlignment = 16;
constexpr size_t kMaxTensors = 256;
constexpr size_t kMaxOperators = 128;
constexpr size_t kMaxIo = 16;
// ABI metadata capacity is eight; upstream RuntimeShape limits this kernel to six.
constexpr size_t kMaxRank = tflite::micro::RuntimeShape::kMaxSmallSize;

// Only ADD is registered. Upstream currently supports int8, but not uint8 ADD.
class AddResolver final : public tflite::MicroOpResolver {
 public:
  AddResolver() : registration_(tflite::micro::Register_ADD()) {
    registration_.builtin_code = tflite::BuiltinOperator_ADD;
  }
  const TFLMRegistration* FindOp(tflite::BuiltinOperator op) const override {
    return op == tflite::BuiltinOperator_ADD ? &registration_ : nullptr;
  }
  const TFLMRegistration* FindOp(const char*) const override { return nullptr; }
  tflite::micro::TfLiteBridgeBuiltinParseFunction GetOpDataParser(
      tflite::BuiltinOperator op) const override {
    return op == tflite::BuiltinOperator_ADD ? tflite::micro::ParseAdd : nullptr;
  }
 private:
  TFLMRegistration registration_;
};

struct alignas(kAlignment) Context {
  AddResolver resolver;
  tflite::MicroInterpreter interpreter;
  uint32_t initialized_inputs = 0;
  bool output_available = false;

  Context(const tflite::Model* model, uint8_t* arena, size_t arena_len)
      : interpreter(model, resolver, arena, arena_len) {}
};

constexpr size_t ContextBytes() {
  return (sizeof(Context) + kAlignment - 1) & ~(kAlignment - 1);
}

bool SameShape(const tflite::Tensor* a, const tflite::Tensor* b) {
  if (a->shape()->size() != b->shape()->size()) return false;
  for (size_t i = 0; i < a->shape()->size(); ++i) {
    if (a->shape()->Get(i) != b->shape()->Get(i)) return false;
  }
  return true;
}

// FlatBuffer verification checks structure; these checks additionally enforce
// semantic invariants before TFLM dereferences indices or prepares a kernel.
// The conservative bound avoids unchecked placement-new on exhausted upstream
// allocator bootstrap / kernel-init paths. It is not the exact arena usage.
int32_t Validate(const uint8_t* bytes, size_t len, size_t* arena_bound) {
  if (!bytes || len < 8 || len >= FLATBUFFERS_MAX_BUFFER_SIZE) {
    return XILA_TFLM_INVALID_MODEL;
  }
  // TFLM borrows typed shape/scale vectors directly from the model buffer.
  if (reinterpret_cast<uintptr_t>(bytes) % 8) return XILA_TFLM_INVALID_ARGUMENT;
  flatbuffers::Verifier verifier(bytes, len);
  if (!tflite::VerifyModelBuffer(verifier)) return XILA_TFLM_INVALID_MODEL;
  const auto* model = tflite::GetModel(bytes);
  if (model->version() != TFLITE_SCHEMA_VERSION) return XILA_TFLM_UNSUPPORTED;
  if (!model->subgraphs() || !model->buffers() || !model->operator_codes()) {
    return XILA_TFLM_INVALID_MODEL;
  }
  if (model->subgraphs()->size() != 1 ||
      (model->metadata() && model->metadata()->size()) ||
      (model->metadata_buffer() && model->metadata_buffer()->size()) ||
      (model->signature_defs() && model->signature_defs()->size())) {
    return XILA_TFLM_UNSUPPORTED;
  }
  const auto* graph = model->subgraphs()->Get(0);
  const auto* tensors = graph->tensors();
  const auto* ops = graph->operators();
  if (!tensors || !ops || !graph->inputs() || !graph->outputs() ||
      tensors->size() == 0 || ops->size() == 0 ||
      graph->inputs()->size() == 0 || graph->outputs()->size() == 0) {
    return XILA_TFLM_INVALID_MODEL;
  }
  if (tensors->size() > kMaxTensors || ops->size() > kMaxOperators ||
      graph->inputs()->size() > kMaxIo || graph->outputs()->size() > kMaxIo) {
    return XILA_TFLM_UNSUPPORTED;
  }
  size_t bound = 4096 + 512 * tensors->size() + 1024 * ops->size();
  bool available[kMaxTensors] = {};
  for (size_t i = 0; i < model->buffers()->size(); ++i) {
    const auto* buffer = model->buffers()->Get(i);
    if (buffer->offset() || buffer->size()) return XILA_TFLM_UNSUPPORTED;
  }
  for (size_t i = 0; i < tensors->size(); ++i) {
    const auto* tensor = tensors->Get(i);
    if (!tensor->shape() || tensor->buffer() >= model->buffers()->size()) {
      return XILA_TFLM_INVALID_MODEL;
    }
    if (tensor->type() != tflite::TensorType_INT8 || tensor->is_variable() ||
        tensor->sparsity() || tensor->shape()->size() > kMaxRank) {
      return XILA_TFLM_UNSUPPORTED;
    }
    size_t elements = 1;
    for (size_t d = 0; d < tensor->shape()->size(); ++d) {
      const int32_t extent = tensor->shape()->Get(d);
      if (extent <= 0) return XILA_TFLM_UNSUPPORTED;
      if (elements > static_cast<size_t>(std::numeric_limits<int>::max()) /
                         static_cast<size_t>(extent)) {
        return XILA_TFLM_UNSUPPORTED;
      }
      elements *= static_cast<size_t>(extent);
    }
    if (tensor->shape_signature()) {
      if (tensor->shape_signature()->size() != tensor->shape()->size()) {
        return XILA_TFLM_UNSUPPORTED;
      }
      for (size_t d = 0; d < tensor->shape()->size(); ++d) {
        if (tensor->shape_signature()->Get(d) != tensor->shape()->Get(d)) {
          return XILA_TFLM_UNSUPPORTED;
        }
      }
    }
    const auto* q = tensor->quantization();
    if (!q || !q->scale() || q->scale()->size() != 1 || !q->zero_point() ||
        q->zero_point()->size() != 1 || q->details_type() != tflite::QuantizationDetails_NONE ||
        !std::isfinite(q->scale()->Get(0)) || q->scale()->Get(0) <= 0 ||
        q->zero_point()->Get(0) < -128 || q->zero_point()->Get(0) > 127) {
      return XILA_TFLM_UNSUPPORTED;
    }
    const auto* data = model->buffers()->Get(tensor->buffer())->data();
    if (data && data->size()) {
      if (data->size() != elements) return XILA_TFLM_INVALID_MODEL;
      available[i] = true;
    }
    if (elements > (std::numeric_limits<size_t>::max() - bound - 16) / 2) {
      return XILA_TFLM_UNSUPPORTED;
    }
    bound += 2 * elements + 16;
  }
  auto valid_index = [tensors](int32_t index) {
    return index >= 0 && static_cast<size_t>(index) < tensors->size();
  };
  for (size_t i = 0; i < graph->inputs()->size(); ++i) {
    const auto index = graph->inputs()->Get(i);
    if (!valid_index(index) || available[index]) return XILA_TFLM_INVALID_MODEL;
    available[index] = true;
  }
  for (size_t i = 0; i < ops->size(); ++i) {
    const auto* op = ops->Get(i);
    if (op->opcode_index() >= model->operator_codes()->size() ||
        !op->inputs() || !op->outputs()) return XILA_TFLM_INVALID_MODEL;
    const auto* code = model->operator_codes()->Get(op->opcode_index());
    if (code->builtin_code() != tflite::BuiltinOperator_ADD ||
        code->deprecated_builtin_code() != tflite::BuiltinOperator_ADD ||
        code->version() < 1 || code->version() > 2 ||
        op->builtin_options_type() != tflite::BuiltinOptions_AddOptions ||
        !op->builtin_options_as_AddOptions() ||
        op->builtin_options_2_type() != tflite::BuiltinOptions2_NONE ||
        (op->custom_options() && op->custom_options()->size()) ||
        (op->intermediates() && op->intermediates()->size()) ||
        (op->mutating_variable_inputs() && op->mutating_variable_inputs()->size()) ||
        op->large_custom_options_offset() || op->large_custom_options_size() ||
        op->inputs()->size() != 2 || op->outputs()->size() != 1) {
      return XILA_TFLM_UNSUPPORTED;
    }
    const auto a = op->inputs()->Get(0), b = op->inputs()->Get(1);
    const auto out = op->outputs()->Get(0);
    if (!valid_index(a) || !valid_index(b) || !valid_index(out) ||
        !available[a] || !available[b] || available[out]) {
      return XILA_TFLM_INVALID_MODEL;
    }
    const auto* ta = tensors->Get(a);
    const auto* tb = tensors->Get(b);
    const auto* to = tensors->Get(out);
    // This initial resolver accepts equal shapes only, avoiding broadcast
    // kernel rank restrictions. Equal-shape ADD supports rank through six.
    if (!SameShape(ta, tb) || !SameShape(ta, to)) return XILA_TFLM_UNSUPPORTED;
    const auto activation = op->builtin_options_as_AddOptions()->fused_activation_function();
    if (activation != tflite::ActivationFunctionType_NONE &&
        activation != tflite::ActivationFunctionType_RELU &&
        activation != tflite::ActivationFunctionType_RELU6 &&
        activation != tflite::ActivationFunctionType_RELU_N1_TO_1) {
      return XILA_TFLM_UNSUPPORTED;
    }
    const double max_scale = std::fmax(ta->quantization()->scale()->Get(0),
                                     tb->quantization()->scale()->Get(0));
    const double multiplier = (2.0 * max_scale) /
        (1048576.0 * to->quantization()->scale()->Get(0));
    if (!(multiplier < 1.0)) return XILA_TFLM_UNSUPPORTED;
    available[out] = true;
  }
  for (size_t i = 0; i < graph->outputs()->size(); ++i) {
    const auto index = graph->outputs()->Get(i);
    if (!valid_index(index) || !available[index]) return XILA_TFLM_INVALID_MODEL;
  }
  *arena_bound = bound;
  return XILA_TFLM_OK;
}

uint32_t Type(const TfLiteTensor* tensor) {
  if (tensor->type == kTfLiteInt8) return XILA_TFLM_I8;
  if (tensor->type == kTfLiteUInt8) return XILA_TFLM_U8;
  return 0;
}

int32_t Info(Context* context, bool input, size_t index, uint32_t* type,
             size_t* rank, size_t* dims, size_t capacity, float* scale,
             int32_t* zero_point) {
  if (!context || !type || !rank || !scale || !zero_point) {
    return XILA_TFLM_INVALID_ARGUMENT;
  }
  auto& interpreter = context->interpreter;
  if (index >= (input ? interpreter.inputs_size() : interpreter.outputs_size())) {
    return XILA_TFLM_INVALID_ARGUMENT;
  }
  auto* tensor = input ? interpreter.input(index) : interpreter.output(index);
  *rank = tensor->dims->size;
  *type = Type(tensor);
  *scale = tensor->params.scale;
  *zero_point = tensor->params.zero_point;
  if (capacity < *rank || (*rank && !dims)) return XILA_TFLM_INVALID_ARGUMENT;
  for (size_t i = 0; i < *rank; ++i) dims[i] = tensor->dims->data[i];
  return XILA_TFLM_OK;
}
}  // namespace

extern "C" {
size_t xila_tflm_context_size(void) { return ContextBytes(); }
size_t xila_tflm_context_alignment(void) { return alignof(Context); }

int32_t xila_tflm_validate_model(const uint8_t* model, size_t len) {
  size_t ignored;
  return Validate(model, len, &ignored);
}

int32_t xila_tflm_model_requirements(const uint8_t* model, size_t len,
                                    size_t* workspace_bytes,
                                    size_t* input_count, size_t* output_count) {
  if (workspace_bytes) *workspace_bytes = 0;
  if (input_count) *input_count = 0;
  if (output_count) *output_count = 0;
  if (!workspace_bytes || !input_count || !output_count) {
    return XILA_TFLM_INVALID_ARGUMENT;
  }
  size_t arena_bound;
  const int32_t status = Validate(model, len, &arena_bound);
  if (status != XILA_TFLM_OK) return status;
  if (arena_bound > SIZE_MAX - ContextBytes() - (kAlignment - 1)) {
    return XILA_TFLM_UNSUPPORTED;
  }
  const auto* graph = tflite::GetModel(model)->subgraphs()->Get(0);
  *workspace_bytes = ContextBytes() + arena_bound + kAlignment - 1;
  *input_count = graph->inputs()->size();
  *output_count = graph->outputs()->size();
  return XILA_TFLM_OK;
}

void xila_tflm_invalidate_outputs(void* context) {
  if (context) static_cast<Context*>(context)->output_available = false;
}

int32_t xila_tflm_create(const uint8_t* model, size_t model_len,
                       uint8_t* workspace, size_t len, void** context,
                       size_t* arena_used) {
  if (context) *context = nullptr;
  if (arena_used) *arena_used = 0;
  if (!context || !arena_used || !workspace ||
      reinterpret_cast<uintptr_t>(workspace) % alignof(Context)) {
    return XILA_TFLM_INVALID_ARGUMENT;
  }
  size_t bound;
  const int32_t status = Validate(model, model_len, &bound);
  if (status != XILA_TFLM_OK) return status;
  if (len < ContextBytes() || len - ContextBytes() < bound) {
    return XILA_TFLM_INSUFFICIENT_MEMORY;
  }
  const uintptr_t w = reinterpret_cast<uintptr_t>(workspace);
  const uintptr_t m = reinterpret_cast<uintptr_t>(model);
  if (len > UINTPTR_MAX - w || model_len > UINTPTR_MAX - m ||
      (w < m + model_len && m < w + len)) return XILA_TFLM_INVALID_ARGUMENT;
  auto* ctx = new (workspace) Context(tflite::GetModel(model),
      workspace + ContextBytes(), len - ContextBytes());
  if (ctx->interpreter.initialization_status() != kTfLiteOk ||
      ctx->interpreter.AllocateTensors() != kTfLiteOk) {
    ctx->~Context();
    return XILA_TFLM_INSUFFICIENT_MEMORY;
  }
  *arena_used = ContextBytes() + ctx->interpreter.arena_used_bytes();
  *context = ctx;
  return XILA_TFLM_OK;
}

void xila_tflm_destroy(void* context) {
  if (context) static_cast<Context*>(context)->~Context();
}

int32_t xila_tflm_input_info(void* context, size_t index, uint32_t* type,
                           size_t* rank, size_t* dims, size_t capacity,
                           float* scale, int32_t* zero_point) {
  return Info(static_cast<Context*>(context), true, index, type, rank, dims,
              capacity, scale, zero_point);
}

int32_t xila_tflm_output_info(void* context, size_t index, uint32_t* type,
                            size_t* rank, size_t* dims, size_t capacity,
                            float* scale, int32_t* zero_point) {
  return Info(static_cast<Context*>(context), false, index, type, rank, dims,
              capacity, scale, zero_point);
}

int32_t xila_tflm_set_input(void* context, size_t index, uint32_t type,
                          const void* data, size_t bytes) {
  auto* ctx = static_cast<Context*>(context);
  if (!ctx) return XILA_TFLM_INVALID_ARGUMENT;
  ctx->output_available = false;
  if (index >= ctx->interpreter.inputs_size()) return XILA_TFLM_INVALID_ARGUMENT;
  if (type != XILA_TFLM_I8 && type != XILA_TFLM_U8) return XILA_TFLM_UNSUPPORTED;
  auto* tensor = ctx->interpreter.input(index);
  if (!data || type != Type(tensor) || bytes != tensor->bytes) {
    return XILA_TFLM_INVALID_ARGUMENT;
  }
  // memmove permits a caller to reuse an output view as input.
  std::memmove(tensor->data.raw, data, bytes);
  ctx->initialized_inputs |= uint32_t{1} << index;
  return XILA_TFLM_OK;
}

int32_t xila_tflm_invoke(void* context) {
  auto* ctx = static_cast<Context*>(context);
  if (!ctx) return XILA_TFLM_INVALID_ARGUMENT;
  ctx->output_available = false;
  if (ctx->initialized_inputs != (uint32_t{1} << ctx->interpreter.inputs_size()) - 1) {
    return XILA_TFLM_INVALID_ARGUMENT;
  }
  if (ctx->interpreter.Invoke() != kTfLiteOk) return XILA_TFLM_BACKEND;
  ctx->output_available = true;
  return XILA_TFLM_OK;
}

int32_t xila_tflm_output_data(void* context, size_t index, uint32_t* type,
                            const void** data, size_t* bytes) {
  if (type) *type = 0;
  if (data) *data = nullptr;
  if (bytes) *bytes = 0;
  auto* ctx = static_cast<Context*>(context);
  if (!ctx || !type || !data || !bytes || index >= ctx->interpreter.outputs_size()) {
    return XILA_TFLM_INVALID_ARGUMENT;
  }
  if (!ctx->output_available) return XILA_TFLM_OUTPUT_UNAVAILABLE;
  auto* tensor = ctx->interpreter.output(index);
  *type = Type(tensor);
  *data = tensor->data.raw_const;
  *bytes = tensor->bytes;
  return XILA_TFLM_OK;
}
}  // extern "C"
