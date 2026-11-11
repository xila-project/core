//! Neural network device class control commands and protocol definitions.
//!
//! Neural network devices are registered under class prefix [`crate::kind::NEURAL_NETWORK`] (`b'N'`).
//!
//! # Protocol Overview
//!
//! Applications interact with neural network character devices using standard VFS
//! operations ([`file_system::BaseOperations`]):
//!
//! - **Fixed-model devices** (`NeuralNetworkDevice`):
//!   A platform mounts a fixed model at a designated path (e.g. `/devices/nn/digit_classifier`).
//!   1. Query tensor counts and shapes using [`GET_INPUT_COUNT`], [`GET_INPUT_INFO`],
//!      [`GET_OUTPUT_COUNT`], and [`GET_OUTPUT_INFO`].
//!   2. Select the input tensor index using [`SET_INPUT_INDEX`]. Seek the file descriptor to 0
//!      and write the tensor bytes sequentially with `write()`.
//!   3. Execute inference by issuing the [`COMPUTE`] command.
//!   4. Select the output tensor index using [`SET_OUTPUT_INDEX`]. Seek the file descriptor to 0
//!      and read the result bytes with `read()`.
//!   5. Closing the file descriptor releases the per-open execution arena.
//!
//! - **Runtime model loader devices** (`LoaderDevice`):
//!   A platform mounts a loader node (e.g. `/devices/nn/cpu`).
//!   1. Open the loader node. The descriptor starts in the *Staging* state.
//!   2. Write the model binary bytes sequentially from offset 0 using `write()`.
//!      Writes beyond the loader's maximum model size are rejected with [`file_system::Error::FileTooLarge`].
//!      Non-contiguous writes are rejected with [`file_system::Error::InvalidParameter`].
//!   3. Send the [`LOAD`] command with a [`LoadRequest`] payload (e.g. TensorFlow Lite format).
//!      Upon successful preparation, the descriptor transitions to the *Ready* state.
//!   4. From this point forward, the descriptor behaves identically to a fixed-model device:
//!      input/output inspection, tensor staging via `write()`, [`COMPUTE`], and output retrieval
//!      via `read()`.
//!   5. Attempting inference commands before [`LOAD`] returns [`file_system::Error::InvalidContext`].
//!      A second [`LOAD`] on a ready descriptor returns [`file_system::Error::InvalidContext`].
//!   6. Closing the descriptor frees both the dynamically allocated model and its execution state.

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
define_command!(
    LOAD,
    Write,
    crate::kind::NEURAL_NETWORK,
    0x0A,
    LoadRequest,
    ()
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

/// Request payload for the [`LOAD`] control command.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadRequest {
    /// Protocol version, must be [`LoadRequest::VERSION`] (`1`).
    pub version: u32,
    /// Model format encoding (`0` for TensorFlow Lite).
    pub encoding: u32,
    /// Reserved for future expansion; must be set to `[0, 0]`.
    pub reserved: [u32; 2],
}

impl LoadRequest {
    /// Current protocol version.
    pub const VERSION: u32 = 1;
    /// TensorFlow Lite flatbuffer encoding identifier.
    pub const ENCODING_TENSORFLOW_LITE: u32 = 0;

    /// Create a new version-1 load request with the specified model encoding.
    pub const fn new(encoding: u32) -> Self {
        Self {
            version: Self::VERSION,
            encoding,
            reserved: [0; 2],
        }
    }

    /// Create a new version-1 TensorFlow Lite load request.
    pub const fn tensorflow_lite() -> Self {
        Self::new(Self::ENCODING_TENSORFLOW_LITE)
    }
}

impl Default for LoadRequest {
    fn default() -> Self {
        Self::tensorflow_lite()
    }
}
