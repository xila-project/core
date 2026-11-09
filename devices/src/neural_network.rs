use file_system::{ControlCommand, define_command};

define_command!(
    GET_INPUT_COUNT,
    Read,
    crate::kind::NEURAL_NETWORK,
    0x01,
    (),
    u32
);
define_command!(
    GET_INPUT_INFO,
    Read,
    crate::kind::NEURAL_NETWORK,
    0x02,
    u32,
    TensorInfo
);
define_command!(
    SET_INPUT_INDEX,
    Write,
    crate::kind::NEURAL_NETWORK,
    0x03,
    u32,
    ()
);
define_command!(
    GET_OUTPUT_COUNT,
    Read,
    crate::kind::NEURAL_NETWORK,
    0x04,
    (),
    u32
);
define_command!(
    GET_OUTPUT_INFO,
    Read,
    crate::kind::NEURAL_NETWORK,
    0x05,
    u32,
    TensorInfo
);
define_command!(
    SET_OUTPUT_INDEX,
    Write,
    crate::kind::NEURAL_NETWORK,
    0x06,
    u32,
    ()
);
define_command!(COMPUTE, Write, crate::kind::NEURAL_NETWORK, 0x07, (), ());
define_command!(
    GET_INPUT_QUANTIZATION,
    Read,
    crate::kind::NEURAL_NETWORK,
    0x08,
    QuantizationRequest,
    QuantizationEntry
);
define_command!(
    GET_OUTPUT_QUANTIZATION,
    Read,
    crate::kind::NEURAL_NETWORK,
    0x09,
    QuantizationRequest,
    QuantizationEntry
);

/// Maximum rank in the version-one VFS tensor metadata layout.
pub const MAX_TENSOR_RANK: usize = 6;

/// Metadata returned by `GET_INPUT_INFO` and `GET_OUTPUT_INFO`.
///
/// `data_type` uses the TensorFlow Lite tensor type value. `dimensions` contains
/// the first `rank` dimensions; `rank` cannot exceed `MAX_TENSOR_RANK`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TensorInfo {
    pub data_type: u32,
    pub rank: u32,
    pub dimensions: [i32; MAX_TENSOR_RANK],
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
