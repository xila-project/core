#![no_std]

pub mod adc;
pub mod audio;
pub mod gpio;
pub mod hash;
pub mod i2c;
pub mod kind;
pub mod neural_network;
pub mod pwm;
pub mod screen;
pub mod spi;
pub mod uart;
pub use neural_network::{
    Accelerator, Capabilities, Context, Error as NeuralNetworkError, ExternalModel, InputBinding,
    MemoryRequirements, ModelFormat, ModelSource, ModelSourceKind, NeuralNetworkDevice,
    Quantization, StaticModel, TensorData, TensorSpec, TensorType, TensorView, Workspace,
};
