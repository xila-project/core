use file_system::{BaseOperations, Context, ControlCommand, Error, Result};
use neural_network_driver::{Backend, COMPUTE, NeuralNetworkDevice, QuantizationEntry, TensorInfo};

struct Model(u32);
struct Execution {
    model: u32,
    tensor: [u8; 4],
}
impl Backend for Model {
    type Execution = Execution;
    type Input<'a> = &'a mut [u8];
    type Output<'a> = &'a [u8];
    fn create_execution(&self) -> Result<Execution> {
        Ok(Execution {
            model: self.0,
            tensor: [0; 4],
        })
    }
    fn owns_execution(&self, e: &Execution) -> bool {
        e.model == self.0
    }
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }
    fn input_info(&self, index: usize) -> Result<TensorInfo> {
        if index != 0 {
            return Err(Error::InvalidParameter);
        }
        Ok(TensorInfo {
            data_type: 3,
            rank: 1,
            dimensions: [4, 0, 0, 0, 0, 0],
            byte_len: 4,
            quantization_axis: -1,
            quantization_count: 0,
        })
    }
    fn output_info(&self, index: usize) -> Result<TensorInfo> {
        self.input_info(index)
    }
    fn input_quantization(&self, _: usize, _: usize) -> Result<QuantizationEntry> {
        Err(Error::InvalidParameter)
    }
    fn output_quantization(&self, _: usize, _: usize) -> Result<QuantizationEntry> {
        Err(Error::InvalidParameter)
    }
    fn input<'a>(&self, e: &'a mut Execution, index: usize) -> Result<&'a mut [u8]> {
        self.input_info(index)?;
        Ok(&mut e.tensor)
    }
    fn output<'a>(&self, e: &'a Execution, index: usize) -> Result<&'a [u8]> {
        self.output_info(index)?;
        Ok(&e.tensor)
    }
    fn compute(&self, e: &mut Execution) -> Result<()> {
        for v in &mut e.tensor {
            *v = v.wrapping_add(1);
        }
        if self.0 == 99 {
            return Err(Error::InputOutput);
        }
        Ok(())
    }
}
fn compute<B: Backend>(device: &NeuralNetworkDevice<B>, context: &mut Context) -> Result<()> {
    device.control_sync(
        context,
        COMPUTE::IDENTIFIER,
        shared::AnyByLayout::NONE,
        shared::AnyByLayout::NONE,
    )
}

#[test]
fn portable_adapter_owns_sessions_and_consumes_inputs() {
    let device = NeuralNetworkDevice::new(Model(1));
    let other = NeuralNetworkDevice::new(Model(2));
    let mut first = Context::new_empty();
    let mut second = Context::new_empty();
    device.open(&mut first).unwrap();
    device.open(&mut second).unwrap();
    assert_eq!(
        other.write_sync(&mut first, &[0], 0),
        Err(Error::InvalidContext)
    );
    assert_eq!(device.write_sync(&mut first, &[1, 2], 0), Ok(2));
    assert_eq!(compute(&device, &mut first), Err(Error::InvalidParameter));
    assert_eq!(device.write_sync(&mut first, &[3, 4], 2), Ok(2));
    compute(&device, &mut first).unwrap();
    let mut result = [0; 4];
    assert_eq!(device.read_sync(&mut first, &mut result, 0), Ok(4));
    assert_eq!(result, [2, 3, 4, 5]);
    assert_eq!(
        device.read_sync(&mut second, &mut result, 0),
        Err(Error::NotInitialized)
    );
    assert_eq!(compute(&device, &mut first), Err(Error::InvalidParameter));
    assert_eq!(
        device.read_sync(&mut first, &mut result, 0),
        Err(Error::NotInitialized)
    );
    device.close(&mut first).unwrap();
    device.close(&mut second).unwrap();
}

#[test]
fn failed_backend_execution_invalidates_results_and_consumes_input() {
    let device = NeuralNetworkDevice::new(Model(99));
    let mut state = Context::new_empty();
    device.open(&mut state).unwrap();
    device.write_sync(&mut state, &[1, 2, 3, 4], 0).unwrap();
    assert_eq!(compute(&device, &mut state), Err(Error::InputOutput));
    assert_eq!(
        device.read_sync(&mut state, &mut [0; 4], 0),
        Err(Error::NotInitialized)
    );
    assert_eq!(compute(&device, &mut state), Err(Error::InvalidParameter));
    device.close(&mut state).unwrap();
}
