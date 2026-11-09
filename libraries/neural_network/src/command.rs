use core::mem::{align_of, size_of};

use file_system::{ControlCommand, Error, Result, define_command};
use shared::AnyByLayout;

define_command!(GET_INPUT_COUNT, Read, b'N', 0x01, (), u32);
define_command!(GET_INPUT_INFO, Read, b'N', 0x02, u32, TensorInfo);
define_command!(SET_INPUT_INDEX, Write, b'N', 0x03, u32, ());
define_command!(GET_OUTPUT_COUNT, Read, b'N', 0x04, (), u32);
define_command!(GET_OUTPUT_INFO, Read, b'N', 0x05, u32, TensorInfo);
define_command!(SET_OUTPUT_INDEX, Write, b'N', 0x06, u32, ());
define_command!(COMPUTE, Write, b'N', 0x07, (), ());
define_command!(
    GET_INPUT_QUANTIZATION,
    Read,
    b'N',
    0x08,
    QuantizationRequest,
    QuantizationEntry
);
define_command!(
    GET_OUTPUT_QUANTIZATION,
    Read,
    b'N',
    0x09,
    QuantizationRequest,
    QuantizationEntry
);

/// Metadata returned by `GET_INPUT_INFO` and `GET_OUTPUT_INFO`.
///
/// `data_type` uses the TensorFlow Lite tensor type value. `dimensions` contains
/// the first `rank` dimensions; `rank` cannot exceed Nanoglia's `MAX_RANK`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TensorInfo {
    pub data_type: u32,
    pub rank: u32,
    pub dimensions: [i32; nanoglia::core::MAX_RANK],
    pub byte_len: u32,
    /// `-1` when the tensor is not quantized.
    pub quantization_axis: i32,
    /// Number of quantization entries, zero when not quantized.
    pub quantization_count: u32,
}

/// Selects a quantization entry for an input or output tensor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuantizationRequest {
    pub tensor_index: u32,
    pub quantization_index: u32,
}

/// One scale/zero-point pair for a tensor quantization axis.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuantizationEntry {
    pub scale: f32,
    pub zero_point: i64,
}

pub fn validate_command<C: ControlCommand>(
    input: &AnyByLayout,
    output: &AnyByLayout,
) -> Result<()> {
    validate_payload::<C::Input>(input)?;
    validate_payload::<C::Output>(output)
}

fn validate_payload<T>(payload: &AnyByLayout) -> Result<()> {
    let address = core::ptr::from_ref(payload).cast::<u8>() as usize;
    if payload.get_size() != size_of::<T>() || !address.is_multiple_of(align_of::<T>()) {
        return Err(Error::InvalidParameter);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_payloads_require_exact_layout() {
        let request = 2u32;
        let mut output = TensorInfo::default();
        assert!(
            validate_command::<GET_INPUT_INFO>(
                AnyByLayout::from(&request),
                AnyByLayout::from_mutable(&mut output),
            )
            .is_ok()
        );

        let wrong_input = 2u64;
        assert_eq!(
            validate_command::<GET_INPUT_INFO>(
                AnyByLayout::from(&wrong_input),
                AnyByLayout::from_mutable(&mut output),
            ),
            Err(Error::InvalidParameter)
        );

        let wrong_output = 0u64;
        assert_eq!(
            validate_command::<GET_INPUT_INFO>(
                AnyByLayout::from(&request),
                AnyByLayout::from(&wrong_output),
            ),
            Err(Error::InvalidParameter)
        );
    }

    #[test]
    fn command_payloads_reject_misaligned_values() {
        #[repr(align(8))]
        struct Aligned([u8; size_of::<TensorInfo>() + 8]);

        let mut bytes = Aligned([0; size_of::<TensorInfo>() + 8]);
        let misaligned = unsafe {
            AnyByLayout::from_raw_parts(bytes.0.as_mut_ptr().add(1), size_of::<TensorInfo>())
        };
        let request = 0u32;
        assert_eq!(
            validate_command::<GET_INPUT_INFO>(AnyByLayout::from(&request), misaligned,),
            Err(Error::InvalidParameter)
        );
    }
}
