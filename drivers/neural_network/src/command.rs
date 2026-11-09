use core::mem::{align_of, size_of};
use file_system::{ControlCommand, Error, Result};
use shared::AnyByLayout;
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
    use device::neural_network::*;

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
