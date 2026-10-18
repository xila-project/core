// Private ABI: buffers and context remain caller-owned. The caller must keep
// 8-byte aligned model / 16-byte aligned workspace alive, nonoverlapping, and
// unmodified until destroy. Workspace access stays exclusive while context lives.
// Live context pointers must be used on one thread at a time and destroyed once.
#![allow(dead_code)] // Consumed by the safe device adapter in the next task.
use core::ffi::c_void;

pub(crate) const OK: i32 = 0;
pub(crate) const INVALID_MODEL: i32 = 1;
pub(crate) const UNSUPPORTED: i32 = 2;
pub(crate) const INSUFFICIENT_MEMORY: i32 = 3;
pub(crate) const INVALID_ARGUMENT: i32 = 4;
pub(crate) const BACKEND: i32 = 5;
pub(crate) const OUTPUT_UNAVAILABLE: i32 = 6;
pub(crate) const TYPE_I8: u32 = 1;
pub(crate) const TYPE_U8: u32 = 2;

unsafe extern "C" {
    pub(crate) fn xila_tflm_context_size() -> usize;
    pub(crate) fn xila_tflm_context_alignment() -> usize;
    pub(crate) fn xila_tflm_validate_model(model: *const u8, model_len: usize) -> i32;
    pub(crate) fn xila_tflm_create(
        model: *const u8,
        model_len: usize,
        workspace: *mut u8,
        workspace_len: usize,
        context: *mut *mut c_void,
        arena_used: *mut usize,
    ) -> i32;
    pub(crate) fn xila_tflm_destroy(context: *mut c_void);
    pub(crate) fn xila_tflm_input_info(
        context: *mut c_void,
        index: usize,
        kind: *mut u32,
        rank: *mut usize,
        dimensions: *mut usize,
        dimensions_capacity: usize,
        scale: *mut f32,
        zero_point: *mut i32,
    ) -> i32;
    pub(crate) fn xila_tflm_output_info(
        context: *mut c_void,
        index: usize,
        kind: *mut u32,
        rank: *mut usize,
        dimensions: *mut usize,
        dimensions_capacity: usize,
        scale: *mut f32,
        zero_point: *mut i32,
    ) -> i32;
    pub(crate) fn xila_tflm_set_input(
        context: *mut c_void,
        index: usize,
        kind: u32,
        data: *const c_void,
        bytes: usize,
    ) -> i32;
    pub(crate) fn xila_tflm_invoke(context: *mut c_void) -> i32;
    pub(crate) fn xila_tflm_output_data(
        context: *mut c_void,
        index: usize,
        kind: *mut u32,
        data: *mut *const c_void,
        bytes: *mut usize,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{ffi::c_void, ptr};

    #[repr(align(16))]
    struct AlignedModel([u8; include_bytes!("../tests/data/quantized_add.tflite").len()]);
    static MODEL: AlignedModel =
        AlignedModel(*include_bytes!("../tests/data/quantized_add.tflite"));

    #[repr(align(16))]
    struct Workspace([u8; 16384]);

    #[test]
    fn quantized_add() {
        let model = &MODEL.0;
        let mut workspace = Workspace([0; 16384]);
        let mut context: *mut c_void = ptr::null_mut();
        let mut used = 0;
        unsafe {
            assert_eq!(xila_tflm_validate_model(model.as_ptr(), model.len()), OK);
            assert_eq!(
                xila_tflm_validate_model(b"invalid".as_ptr(), 7),
                INVALID_MODEL
            );
            assert_eq!(
                xila_tflm_create(
                    model.as_ptr(),
                    model.len(),
                    workspace.0.as_mut_ptr(),
                    16,
                    &mut context,
                    &mut used
                ),
                INSUFFICIENT_MEMORY
            );
            assert!(context.is_null());
            assert_eq!(
                xila_tflm_create(
                    model.as_ptr(),
                    model.len(),
                    workspace.0.as_mut_ptr(),
                    workspace.0.len(),
                    &mut context,
                    &mut used
                ),
                OK
            );
            assert!(!context.is_null());
            assert!(used > xila_tflm_context_size() && used <= workspace.0.len());
            assert!(xila_tflm_context_alignment() <= 16);
            let mut kind = 0;
            let mut data = ptr::null();
            let mut bytes = 0;
            assert_eq!(
                xila_tflm_output_data(context, 0, &mut kind, &mut data, &mut bytes),
                OUTPUT_UNAVAILABLE
            );
            let mut rank = 0;
            let mut dimensions = [0_usize; 8];
            let mut scale = 0.0;
            let mut zero_point = -1;
            assert_eq!(
                xila_tflm_input_info(
                    context,
                    0,
                    &mut kind,
                    &mut rank,
                    dimensions.as_mut_ptr(),
                    dimensions.len(),
                    &mut scale,
                    &mut zero_point
                ),
                OK
            );
            assert_eq!((kind, rank, scale, zero_point), (TYPE_I8, 2, 1.0, 0));
            assert_eq!(&dimensions[..rank], &[1, 4]);
            assert_eq!(
                xila_tflm_output_info(
                    context,
                    0,
                    &mut kind,
                    &mut rank,
                    dimensions.as_mut_ptr(),
                    dimensions.len(),
                    &mut scale,
                    &mut zero_point
                ),
                OK
            );
            assert_eq!((kind, rank, scale, zero_point), (TYPE_I8, 2, 1.0, 0));
            assert_eq!(&dimensions[..rank], &[1, 4]);
            // An invocation without initialized input must never expose output.
            assert_eq!(xila_tflm_invoke(context), INVALID_ARGUMENT);
            assert_eq!(
                xila_tflm_output_data(context, 0, &mut kind, &mut data, &mut bytes),
                OUTPUT_UNAVAILABLE
            );
            let input = [-3_i8, 0, 4, 10];
            assert_eq!(
                xila_tflm_set_input(context, 0, 999, input.as_ptr().cast(), input.len()),
                UNSUPPORTED
            );
            assert_eq!(
                xila_tflm_set_input(context, 0, TYPE_I8, input.as_ptr().cast(), input.len()),
                OK
            );
            assert_eq!(xila_tflm_invoke(context), OK);
            assert_eq!(
                xila_tflm_output_data(context, 0, &mut kind, &mut data, &mut bytes),
                OK
            );
            assert_eq!(kind, TYPE_I8);
            assert_eq!(bytes, 4);
            assert_eq!(
                core::slice::from_raw_parts(data.cast::<i8>(), bytes),
                &[-2, 2, 7, 14]
            );
            // Any attempted input change invalidates the previously returned output.
            assert_eq!(
                xila_tflm_set_input(context, 0, 999, input.as_ptr().cast(), input.len()),
                UNSUPPORTED
            );
            assert_eq!(
                xila_tflm_output_data(context, 0, &mut kind, &mut data, &mut bytes),
                OUTPUT_UNAVAILABLE
            );
            assert!(data.is_null());
            assert_eq!(bytes, 0);
            xila_tflm_destroy(context);
        }
    }
    // Locate a FlatBuffer field in the trusted fixture for semantic mutations.
    fn u32_at(bytes: &[u8], offset: usize) -> usize {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
    }

    fn field(bytes: &[u8], table: usize, slot: usize) -> usize {
        let distance = i32::from_le_bytes(bytes[table..table + 4].try_into().unwrap());
        let vtable = table.checked_add_signed(-(distance as isize)).unwrap();
        let offset = vtable + 4 + 2 * slot;
        table
            + usize::from(u16::from_le_bytes(
                bytes[offset..offset + 2].try_into().unwrap(),
            ))
    }

    fn indirect(bytes: &[u8], offset: usize) -> usize {
        offset + u32_at(bytes, offset)
    }

    fn tensor_field(bytes: &[u8], index: usize, slot: usize) -> usize {
        let model = u32_at(bytes, 0);
        let graphs = indirect(bytes, field(bytes, model, 2));
        let graph = indirect(bytes, graphs + 4);
        let tensors = indirect(bytes, field(bytes, graph, 0));
        let tensor = indirect(bytes, tensors + 4 + index * 4);
        field(bytes, tensor, slot)
    }

    #[test]
    fn rejects_malformed_and_unsupported_models() {
        let model = &MODEL.0;
        unsafe {
            assert_eq!(xila_tflm_validate_model(ptr::null(), 0), INVALID_MODEL);
            for len in 0..model.len() {
                assert_eq!(
                    xila_tflm_validate_model(model.as_ptr(), len),
                    INVALID_MODEL,
                    "prefix length {len}"
                );
            }
            let mut unsupported = model.to_vec();
            let kind = tensor_field(&unsupported, 0, 1);
            unsupported[kind] = 1; // TensorType_FLOAT16, not an ABI type code.
            assert_eq!(
                xila_tflm_validate_model(unsupported.as_ptr(), unsupported.len()),
                UNSUPPORTED
            );
            let mut invalid_shape = model.to_vec();
            let shape = indirect(&invalid_shape, tensor_field(&invalid_shape, 0, 0));
            invalid_shape[shape + 4..shape + 8].copy_from_slice(&0_i32.to_le_bytes());
            assert_eq!(
                xila_tflm_validate_model(invalid_shape.as_ptr(), invalid_shape.len()),
                UNSUPPORTED
            );
        }
    }

    #[test]
    fn rejects_invalid_workspace_and_input_metadata() {
        let model = &MODEL.0;
        let mut workspace = Workspace([0; 16384]);
        let mut context = ptr::null_mut();
        let mut used = 0;
        unsafe {
            assert_eq!(
                xila_tflm_create(
                    model.as_ptr(),
                    model.len(),
                    workspace.0.as_mut_ptr().add(1),
                    workspace.0.len() - 1,
                    &mut context,
                    &mut used
                ),
                INVALID_ARGUMENT
            );
            assert!(context.is_null());
            assert_eq!(used, 0);
            // Upstream borrows typed model vectors; reject an unaligned model.
            let mut unaligned = Workspace([0; 16384]);
            unaligned.0[1..1 + model.len()].copy_from_slice(model);
            assert_eq!(
                xila_tflm_validate_model(unaligned.0.as_ptr().add(1), model.len()),
                INVALID_ARGUMENT
            );
            // The conservative arena preflight rejects intermediate small sizes.
            for len in [0, 16, 512, 1024, 2048, 4096, 6144] {
                assert_eq!(
                    xila_tflm_create(
                        model.as_ptr(),
                        model.len(),
                        workspace.0.as_mut_ptr(),
                        len,
                        &mut context,
                        &mut used
                    ),
                    INSUFFICIENT_MEMORY
                );
                assert!(context.is_null());
                assert_eq!(used, 0);
            }
            assert_eq!(
                xila_tflm_create(
                    model.as_ptr(),
                    model.len(),
                    workspace.0.as_mut_ptr(),
                    workspace.0.len(),
                    &mut context,
                    &mut used
                ),
                OK
            );
            let input = [1_i8; 4];
            assert_eq!(
                xila_tflm_set_input(context, 1, TYPE_I8, input.as_ptr().cast(), 4),
                INVALID_ARGUMENT
            );
            assert_eq!(
                xila_tflm_set_input(context, 0, TYPE_U8, input.as_ptr().cast(), 4),
                INVALID_ARGUMENT
            );
            assert_eq!(
                xila_tflm_set_input(context, 0, TYPE_I8, input.as_ptr().cast(), 3),
                INVALID_ARGUMENT
            );
            assert_eq!(
                xila_tflm_set_input(context, 0, TYPE_I8, ptr::null(), 4),
                INVALID_ARGUMENT
            );
            let mut kind = 0;
            let mut rank = 0;
            let mut scale = 0.0;
            let mut zero_point = 0;
            assert_eq!(
                xila_tflm_input_info(
                    context,
                    1,
                    &mut kind,
                    &mut rank,
                    ptr::null_mut(),
                    0,
                    &mut scale,
                    &mut zero_point
                ),
                INVALID_ARGUMENT
            );
            assert_eq!(
                xila_tflm_input_info(
                    context,
                    0,
                    &mut kind,
                    &mut rank,
                    ptr::null_mut(),
                    0,
                    &mut scale,
                    &mut zero_point
                ),
                INVALID_ARGUMENT
            );
            assert_eq!(rank, 2);
            xila_tflm_destroy(context);
            xila_tflm_destroy(ptr::null_mut());
        }
    }
}
