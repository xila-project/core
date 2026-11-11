use drivers_neural_network::*;
use file_system::{ControlCommand, ControlCommandIdentifier};

#[test]
fn command_identifiers_use_the_nn_prefix_and_stable_numbers() {
    assert_eq!(
        GET_INPUT_COUNT::IDENTIFIER,
        ControlCommandIdentifier::new_read::<(), u32>(b'N', 0x01)
    );
    assert_eq!(
        GET_INPUT_INFO::IDENTIFIER,
        ControlCommandIdentifier::new_read::<u32, drivers_neural_network::TensorInfo>(b'N', 0x02)
    );
    assert_eq!(
        SET_INPUT_INDEX::IDENTIFIER,
        ControlCommandIdentifier::new_write::<u32, ()>(b'N', 0x03)
    );
    assert_eq!(
        GET_OUTPUT_COUNT::IDENTIFIER,
        ControlCommandIdentifier::new_read::<(), u32>(b'N', 0x04)
    );
    assert_eq!(
        GET_OUTPUT_INFO::IDENTIFIER,
        ControlCommandIdentifier::new_read::<u32, drivers_neural_network::TensorInfo>(b'N', 0x05)
    );
    assert_eq!(
        SET_OUTPUT_INDEX::IDENTIFIER,
        ControlCommandIdentifier::new_write::<u32, ()>(b'N', 0x06)
    );
    assert_eq!(
        COMPUTE::IDENTIFIER,
        ControlCommandIdentifier::new_write::<(), ()>(b'N', 0x07)
    );
    assert_eq!(
        GET_INPUT_QUANTIZATION::IDENTIFIER,
        ControlCommandIdentifier::new_read::<QuantizationRequest, QuantizationEntry>(b'N', 0x08)
    );
    assert_eq!(
        GET_OUTPUT_QUANTIZATION::IDENTIFIER,
        ControlCommandIdentifier::new_read::<QuantizationRequest, QuantizationEntry>(b'N', 0x09)
    );
    assert_eq!(
        LOAD::IDENTIFIER,
        ControlCommandIdentifier::new_write::<LoadRequest, ()>(b'N', 0x0A)
    );
}
