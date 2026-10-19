// Private C ABI. All buffers remain owned by the caller. Model bytes require
// 8-byte alignment; workspace requires xila_tflm_context_alignment(). Both must
// remain alive, exclusive of overlapping mutations, until context destruction.
#ifndef XILA_TFLM_WRAPPER_H
#define XILA_TFLM_WRAPPER_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// Keep these stable and synchronized with ffi.rs. These are not TfLite enums.
enum {
  XILA_TFLM_OK = 0,
  XILA_TFLM_INVALID_MODEL = 1,
  XILA_TFLM_UNSUPPORTED = 2,
  XILA_TFLM_INSUFFICIENT_MEMORY = 3,
  XILA_TFLM_INVALID_ARGUMENT = 4,
  XILA_TFLM_BACKEND = 5,
  XILA_TFLM_OUTPUT_UNAVAILABLE = 6,
  XILA_TFLM_I8 = 1,
  XILA_TFLM_U8 = 2
};
size_t xila_tflm_context_size(void);
size_t xila_tflm_context_alignment(void);
int32_t xila_tflm_validate_model(const uint8_t *model, size_t model_len);
int32_t xila_tflm_model_requirements(const uint8_t *model, size_t model_len,
                                    size_t *workspace_bytes,
                                    size_t *input_count, size_t *output_count);
void xila_tflm_invalidate_outputs(void *context);
int32_t xila_tflm_create(const uint8_t *model, size_t model_len,
                       uint8_t *workspace, size_t workspace_len,
                       void **context, size_t *arena_used);
void xila_tflm_destroy(void *context);
int32_t xila_tflm_input_info(void *context, size_t index, uint32_t *type,
                           size_t *rank, size_t *dimensions,
                           size_t dimensions_capacity, float *scale,
                           int32_t *zero_point);
int32_t xila_tflm_output_info(void *context, size_t index, uint32_t *type,
                            size_t *rank, size_t *dimensions,
                            size_t dimensions_capacity, float *scale,
                            int32_t *zero_point);
int32_t xila_tflm_set_input(void *context, size_t index, uint32_t type,
                          const void *data, size_t bytes);
int32_t xila_tflm_invoke(void *context);
int32_t xila_tflm_output_data(void *context, size_t index, uint32_t *type,
                            const void **data, size_t *bytes);
#ifdef __cplusplus
}
#endif
#endif
