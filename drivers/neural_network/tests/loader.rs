extern crate alloc;

use alloc::{sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use drivers_neural_network::{
    Backend, COMPUTE, GET_INPUT_COUNT, GET_INPUT_INFO, LOAD, LoadRequest, Loader, LoaderDevice,
    ModelBuffer, ModelEncoding, NeuralNetworkDevice, QuantizationEntry, SET_INPUT_INDEX,
    SET_OUTPUT_INDEX, TensorInfo,
};
use file_system::{BaseOperations, Context, ControlCommand, Error, Result};
use shared::AnyByLayout;

struct DropGuard(Arc<AtomicBool>);
impl Drop for DropGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct MockBackend {
    id: u32,
    tensor_len: usize,
    bias: u8,
    _drop_guard: Option<DropGuard>,
}

struct MockExecution {
    backend_id: u32,
    tensor: Vec<u8>,
    _drop_guard: Option<DropGuard>,
}

impl Backend for MockBackend {
    type Execution = MockExecution;
    type Input<'a> = &'a mut [u8];
    type Output<'a> = &'a [u8];

    fn create_execution(&self) -> Result<Self::Execution> {
        Ok(MockExecution {
            backend_id: self.id,
            tensor: vec![0; self.tensor_len],
            _drop_guard: self._drop_guard.as_ref().map(|g| DropGuard(g.0.clone())),
        })
    }

    fn owns_execution(&self, execution: &Self::Execution) -> bool {
        execution.backend_id == self.id
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
        let mut dimensions = [0; drivers_neural_network::MAX_TENSOR_RANK];
        dimensions[0] = self.tensor_len as i32;
        Ok(TensorInfo {
            data_type: 3,
            rank: 1,
            dimensions,
            byte_len: self.tensor_len as u32,
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

    fn input<'a>(&self, execution: &'a mut Self::Execution, index: usize) -> Result<&'a mut [u8]> {
        self.input_info(index)?;
        Ok(&mut execution.tensor)
    }

    fn output<'a>(&self, execution: &'a Self::Execution, index: usize) -> Result<&'a [u8]> {
        self.output_info(index)?;
        Ok(&execution.tensor)
    }

    fn compute(&self, execution: &mut Self::Execution) -> Result<()> {
        for byte in &mut execution.tensor {
            *byte = byte.wrapping_add(self.bias);
        }
        Ok(())
    }
}

struct MockLoader {
    max_model_size: usize,
    drop_flag: Option<Arc<AtomicBool>>,
}

impl Loader for MockLoader {
    type Backend = MockBackend;

    fn load(&self, encoding: ModelEncoding, model: ModelBuffer) -> Result<Self::Backend> {
        if encoding != ModelEncoding::TensorFlowLite {
            return Err(Error::InvalidParameter);
        }
        if model.len() < 9 {
            return Err(Error::InvalidParameter);
        }
        let id = u32::from_le_bytes(model[0..4].try_into().unwrap());
        let tensor_len = u32::from_le_bytes(model[4..8].try_into().unwrap()) as usize;
        let bias = model[8];
        Ok(MockBackend {
            id,
            tensor_len,
            bias,
            _drop_guard: self.drop_flag.as_ref().map(|f| DropGuard(f.clone())),
        })
    }

    fn maximum_model_size(&self) -> usize {
        self.max_model_size
    }
}

fn create_model_bytes(id: u32, tensor_len: u32, bias: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&tensor_len.to_le_bytes());
    bytes.push(bias);
    bytes
}

fn send_load(
    device: &LoaderDevice<MockLoader>,
    context: &mut Context,
    request: LoadRequest,
) -> Result<()> {
    device.control_sync(
        context,
        LOAD::IDENTIFIER,
        AnyByLayout::from(&request),
        AnyByLayout::NONE,
    )
}

fn send_compute(device: &LoaderDevice<MockLoader>, context: &mut Context) -> Result<()> {
    device.control_sync(
        context,
        COMPUTE::IDENTIFIER,
        AnyByLayout::NONE,
        AnyByLayout::NONE,
    )
}

fn get_input_count(device: &LoaderDevice<MockLoader>, context: &mut Context) -> Result<u32> {
    let mut count = 0u32;
    device.control_sync(
        context,
        GET_INPUT_COUNT::IDENTIFIER,
        AnyByLayout::NONE,
        AnyByLayout::from_mutable(&mut count),
    )?;
    Ok(count)
}

fn get_input_info(
    device: &LoaderDevice<MockLoader>,
    context: &mut Context,
    index: u32,
) -> Result<TensorInfo> {
    let mut info = TensorInfo::default();
    device.control_sync(
        context,
        GET_INPUT_INFO::IDENTIFIER,
        AnyByLayout::from(&index),
        AnyByLayout::from_mutable(&mut info),
    )?;
    Ok(info)
}

#[test]
fn full_happy_path_chunked_staging_and_inference() {
    let loader = MockLoader {
        max_model_size: 128,
        drop_flag: None,
    };
    let device = LoaderDevice::new(loader);
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();

    let model = create_model_bytes(42, 4, 10);
    assert_eq!(model.len(), 9);

    // Write model in 3 chunks
    assert_eq!(device.write_sync(&mut context, &model[0..3], 0), Ok(3));
    assert_eq!(device.write_sync(&mut context, &model[3..7], 3), Ok(4));
    assert_eq!(device.write_sync(&mut context, &model[7..9], 7), Ok(2));

    // Send LOAD command
    send_load(&device, &mut context, LoadRequest::tensorflow_lite()).unwrap();

    // Query info
    assert_eq!(get_input_count(&device, &mut context), Ok(1));
    let info = get_input_info(&device, &mut context, 0).unwrap();
    assert_eq!(info.byte_len, 4);

    // Select input index 0
    device
        .control_sync(
            &mut context,
            SET_INPUT_INDEX::IDENTIFIER,
            AnyByLayout::from(&0u32),
            AnyByLayout::NONE,
        )
        .unwrap();

    // Write input tensor bytes
    assert_eq!(device.write_sync(&mut context, &[1, 2, 3, 4], 0), Ok(4));

    // Compute
    send_compute(&device, &mut context).unwrap();

    // Select output index 0
    device
        .control_sync(
            &mut context,
            SET_OUTPUT_INDEX::IDENTIFIER,
            AnyByLayout::from(&0u32),
            AnyByLayout::NONE,
        )
        .unwrap();

    // Read output tensor bytes
    let mut output = [0u8; 4];
    assert_eq!(device.read_sync(&mut context, &mut output, 0), Ok(4));
    assert_eq!(output, [11, 12, 13, 14]);

    device.close(&mut context).unwrap();
}

#[test]
fn oversize_model_is_rejected() {
    let loader = MockLoader {
        max_model_size: 10,
        drop_flag: None,
    };
    let device = LoaderDevice::new(loader);
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();

    // Direct write exceeding limit
    let large_chunk = [0u8; 11];
    assert_eq!(
        device.write_sync(&mut context, &large_chunk, 0),
        Err(Error::FileTooLarge)
    );

    // Cumulative write exceeding limit
    assert_eq!(device.write_sync(&mut context, &[0; 6], 0), Ok(6));
    assert_eq!(
        device.write_sync(&mut context, &[0; 5], 6),
        Err(Error::FileTooLarge)
    );

    device.close(&mut context).unwrap();
}

#[test]
fn commands_before_load_are_rejected() {
    let loader = MockLoader {
        max_model_size: 128,
        drop_flag: None,
    };
    let device = LoaderDevice::new(loader);
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();

    // Control commands before LOAD return InvalidContext
    assert_eq!(
        get_input_count(&device, &mut context),
        Err(Error::InvalidContext)
    );
    assert_eq!(
        send_compute(&device, &mut context),
        Err(Error::InvalidContext)
    );
    assert_eq!(
        device.control_sync(
            &mut context,
            SET_INPUT_INDEX::IDENTIFIER,
            AnyByLayout::from(&0u32),
            AnyByLayout::NONE,
        ),
        Err(Error::InvalidContext)
    );

    // Read before LOAD returns InvalidContext
    let mut buf = [0u8; 4];
    assert_eq!(
        device.read_sync(&mut context, &mut buf, 0),
        Err(Error::InvalidContext)
    );

    device.close(&mut context).unwrap();
}

#[test]
fn double_load_is_rejected() {
    let loader = MockLoader {
        max_model_size: 128,
        drop_flag: None,
    };
    let device = LoaderDevice::new(loader);
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();

    let model = create_model_bytes(1, 4, 1);
    device.write_sync(&mut context, &model, 0).unwrap();
    send_load(&device, &mut context, LoadRequest::tensorflow_lite()).unwrap();

    // Second LOAD must return InvalidContext
    assert_eq!(
        send_load(&device, &mut context, LoadRequest::tensorflow_lite()),
        Err(Error::InvalidContext)
    );

    device.close(&mut context).unwrap();
}

#[test]
fn non_contiguous_write_is_rejected() {
    let loader = MockLoader {
        max_model_size: 128,
        drop_flag: None,
    };
    let device = LoaderDevice::new(loader);
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();

    // First write with offset > 0 fails
    assert_eq!(
        device.write_sync(&mut context, &[1, 2], 2),
        Err(Error::InvalidParameter)
    );

    // Initial write at 0 succeeds
    assert_eq!(device.write_sync(&mut context, &[1, 2], 0), Ok(2));

    // Next write leaving a gap fails (offset 4 when len is 2)
    assert_eq!(
        device.write_sync(&mut context, &[3, 4], 4),
        Err(Error::InvalidParameter)
    );

    device.close(&mut context).unwrap();
}

#[test]
fn two_handles_on_same_node_with_different_models_are_independent() {
    let loader = MockLoader {
        max_model_size: 128,
        drop_flag: None,
    };
    let device = LoaderDevice::new(loader);
    let mut first = Context::new_empty();
    let mut second = Context::new_empty();
    device.open(&mut first).unwrap();
    device.open(&mut second).unwrap();

    // Model A: bias +5
    let model_a = create_model_bytes(100, 4, 5);
    device.write_sync(&mut first, &model_a, 0).unwrap();
    send_load(&device, &mut first, LoadRequest::tensorflow_lite()).unwrap();

    // Model B: bias +50
    let model_b = create_model_bytes(200, 4, 50);
    device.write_sync(&mut second, &model_b, 0).unwrap();
    send_load(&device, &mut second, LoadRequest::tensorflow_lite()).unwrap();

    // Write input and compute on first
    device.write_sync(&mut first, &[1, 2, 3, 4], 0).unwrap();
    send_compute(&device, &mut first).unwrap();

    // Write input and compute on second
    device
        .write_sync(&mut second, &[10, 20, 30, 40], 0)
        .unwrap();
    send_compute(&device, &mut second).unwrap();

    let mut out_a = [0u8; 4];
    device.read_sync(&mut first, &mut out_a, 0).unwrap();
    assert_eq!(out_a, [6, 7, 8, 9]);

    let mut out_b = [0u8; 4];
    device.read_sync(&mut second, &mut out_b, 0).unwrap();
    assert_eq!(out_b, [60, 70, 80, 90]);

    device.close(&mut first).unwrap();
    device.close(&mut second).unwrap();
}

#[test]
fn close_frees_state() {
    let dropped = Arc::new(AtomicBool::new(false));
    let loader = MockLoader {
        max_model_size: 128,
        drop_flag: Some(dropped.clone()),
    };
    let device = LoaderDevice::new(loader);
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();

    let model = create_model_bytes(1, 4, 1);
    device.write_sync(&mut context, &model, 0).unwrap();
    send_load(&device, &mut context, LoadRequest::tensorflow_lite()).unwrap();

    assert!(!dropped.load(Ordering::SeqCst));
    device.close(&mut context).unwrap();
    assert!(dropped.load(Ordering::SeqCst));

    // Access after close fails
    assert_eq!(
        device.write_sync(&mut context, &[0], 0),
        Err(Error::InvalidContext)
    );
    assert_eq!(device.close(&mut context), Err(Error::InvalidContext));
}

#[test]
fn neural_network_device_rejects_load() {
    let backend = MockBackend {
        id: 1,
        tensor_len: 4,
        bias: 1,
        _drop_guard: None,
    };
    let device = NeuralNetworkDevice::new(backend);
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();

    let request = LoadRequest::tensorflow_lite();
    assert_eq!(
        device.control_sync(
            &mut context,
            LOAD::IDENTIFIER,
            AnyByLayout::from(&request),
            AnyByLayout::NONE,
        ),
        Err(Error::UnsupportedOperation)
    );

    device.close(&mut context).unwrap();
}
