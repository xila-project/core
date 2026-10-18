use device::neural_network::{
    Accelerator, Error, InputBinding, MemoryRequirements, ModelFormat, ModelSource,
    ModelSourceKind, NeuralNetworkDevice, Quantization, TensorData, TensorSpec, TensorType,
    TensorView, Workspace,
};
use drivers_tflite_micro::TfliteMicroDevice;

#[repr(align(16))]
struct Aligned<const N: usize>([u8; N]);

static MODEL: Aligned<{ include_bytes!("data/quantized_add.tflite").len() }> =
    Aligned(*include_bytes!("data/quantized_add.tflite"));

fn source() -> ModelSource<'static> {
    ModelSource::Buffer {
        data: &MODEL.0,
        format: ModelFormat::Tflite,
    }
}

fn spec<'a>(shape: &'a [usize]) -> TensorSpec<'a> {
    TensorSpec::try_new(
        shape,
        TensorType::I8,
        Some(Quantization::PerTensor {
            scale: 1.0,
            zero_point: 0,
        }),
    )
    .unwrap()
}

fn workspace() -> Aligned<16384> {
    Aligned([0; 16384])
}

#[test]
fn capabilities_and_model_source_validation() {
    let mut device = TfliteMicroDevice::new();
    let caps = device.capabilities();
    assert_eq!(caps.formats, &[ModelFormat::Tflite]);
    assert_eq!(caps.source_modes, &[ModelSourceKind::Buffer]);
    assert_eq!(caps.tensor_types, &[TensorType::I8]);
    assert_eq!(caps.accelerator, Accelerator::Cpu);
    assert!(!caps.asynchronous && !caps.zero_copy && caps.quantization);
    assert!(
        device
            .memory_requirements(ModelSource::Buffer {
                data: &MODEL.0,
                format: ModelFormat::Onnx
            })
            .is_err()
    );
    assert!(
        device
            .memory_requirements(ModelSource::External(
                device::neural_network::ExternalModel {
                    location: "model.tflite",
                    format: ModelFormat::Tflite
                }
            ))
            .is_err()
    );
    assert!(
        device
            .memory_requirements(ModelSource::Static(device::neural_network::StaticModel {
                model: &(),
                format: ModelFormat::Tflite
            }))
            .is_err()
    );
    assert!(
        device
            .load_model(ModelSource::Buffer {
                data: &MODEL.0,
                format: ModelFormat::Onnx
            })
            .is_err()
    );
    assert!(
        device
            .load_model(ModelSource::Buffer {
                data: b"bad model",
                format: ModelFormat::Tflite
            })
            .is_err()
    );
}

#[test]
fn borrows_model_and_reports_conservative_requirements() {
    let mut device = TfliteMicroDevice::new();
    let requirements = device.memory_requirements(source()).unwrap();
    assert_eq!(requirements.model_bytes, 0);
    assert!(requirements.context_bytes > 0);
    assert!(requirements.workspace_bytes > 0);
    assert!(requirements.alignment >= 16 && requirements.alignment.is_power_of_two());
    let model = device.load_model(source()).unwrap();
    assert_eq!(model.bytes().as_ptr(), MODEL.0.as_ptr());
    device.unload_model(model).unwrap();
}

#[test]
fn runs_inference_and_exposes_output_only_after_successful_compute() {
    let mut device = TfliteMicroDevice::new();
    let model = device.load_model(source()).unwrap();
    let requirements = device.memory_requirements(source()).unwrap();
    let mut memory = workspace();
    let input_spec = spec(&[1, 4]);
    let mut context = device
        .create_context(
            &model,
            &[input_spec],
            Some(Workspace::try_new(&mut memory.0, requirements).unwrap()),
        )
        .unwrap();
    assert_eq!(device.output(&context, 0).unwrap_err(), Error::Backend);
    let input = [-3_i8, 0, 4, 10];
    let binding = InputBinding::new(
        0,
        TensorView::try_new(input_spec, TensorData::I8(&input)).unwrap(),
    );
    device.compute(&mut context, &[binding]).unwrap();
    assert_eq!(
        device.output(&context, 0).unwrap().data(),
        TensorData::I8(&[-2, 2, 7, 14])
    );
    assert_eq!(
        device.output(&context, 1).unwrap_err(),
        Error::InvalidTensor
    );
    assert_eq!(device.compute(&mut context, &[]), Err(Error::InvalidTensor));
    assert_eq!(device.output(&context, 0).unwrap_err(), Error::Backend);
    device.destroy_context(context).unwrap();
    device.unload_model(model).unwrap();
}

#[test]
fn requires_exact_input_metadata_and_workspace() {
    let mut device = TfliteMicroDevice::new();
    let model = device.load_model(source()).unwrap();
    let requirements = device.memory_requirements(source()).unwrap();
    let mut too_small = Aligned([0; 1]);
    assert_eq!(
        Workspace::try_new(
            &mut too_small.0,
            MemoryRequirements {
                workspace_bytes: 2,
                ..requirements
            },
        )
        .err()
        .unwrap(),
        Error::InsufficientMemory,
    );
    assert_eq!(
        device
            .create_context(&model, &[spec(&[1, 4])], None)
            .err()
            .unwrap(),
        Error::InsufficientMemory
    );
    let mut memory = workspace();
    assert_eq!(
        device
            .create_context(
                &model,
                &[spec(&[4])],
                Some(Workspace::try_new(&mut memory.0, requirements).unwrap())
            )
            .err()
            .unwrap(),
        Error::InvalidTensor
    );
    let wrong_type = TensorSpec::try_new(
        &[1, 4],
        TensorType::U8,
        Some(Quantization::PerTensor {
            scale: 1.0,
            zero_point: 0,
        }),
    )
    .unwrap();
    let mut memory = workspace();
    assert_eq!(
        device
            .create_context(
                &model,
                &[wrong_type],
                Some(Workspace::try_new(&mut memory.0, requirements).unwrap()),
            )
            .err()
            .unwrap(),
        Error::InvalidTensor
    );
    let mut memory = workspace();
    let wrong_quant = TensorSpec::try_new(
        &[1, 4],
        TensorType::I8,
        Some(Quantization::PerTensor {
            scale: 0.5,
            zero_point: 0,
        }),
    )
    .unwrap();
    assert_eq!(
        device
            .create_context(
                &model,
                &[wrong_quant],
                Some(Workspace::try_new(&mut memory.0, requirements).unwrap())
            )
            .err()
            .unwrap(),
        Error::InvalidTensor
    );
    let mut memory = workspace();
    assert_eq!(
        device
            .create_context(
                &model,
                &[],
                Some(Workspace::try_new(&mut memory.0, requirements).unwrap())
            )
            .err()
            .unwrap(),
        Error::InvalidTensor
    );
    device.unload_model(model).unwrap();
}

#[test]
fn rejects_bad_binding_sets_and_invalidates_prior_output() {
    let mut device = TfliteMicroDevice::new();
    let model = device.load_model(source()).unwrap();
    let req = device.memory_requirements(source()).unwrap();
    let mut memory = workspace();
    let input_spec = spec(&[1, 4]);
    let mut context = device
        .create_context(
            &model,
            &[input_spec],
            Some(Workspace::try_new(&mut memory.0, req).unwrap()),
        )
        .unwrap();
    let input = [1_i8, 2, 3, 4];
    let view = TensorView::try_new(input_spec, TensorData::I8(&input)).unwrap();
    let binding = InputBinding::new(0, view);
    device.compute(&mut context, &[binding]).unwrap();
    assert_eq!(
        device.compute(&mut context, &[InputBinding::new(1, view)]),
        Err(Error::InvalidTensor)
    );
    assert_eq!(device.output(&context, 0).unwrap_err(), Error::Backend);
    assert_eq!(
        device.compute(&mut context, &[binding, binding]),
        Err(Error::InvalidTensor)
    );
    let bad_data = TensorView::try_new(input_spec, TensorData::U8(&[1, 2, 3, 4]));
    assert!(bad_data.is_err());
    let wrong_shape = TensorView::try_new(spec(&[4]), TensorData::I8(&input)).unwrap();
    assert_eq!(
        device.compute(&mut context, &[InputBinding::new(0, wrong_shape)]),
        Err(Error::InvalidTensor)
    );
    device.destroy_context(context).unwrap();
    device.unload_model(model).unwrap();
}

#[test]
fn rejects_misaligned_model_and_workspace() {
    let mut device = TfliteMicroDevice::new();
    let req = device.memory_requirements(source()).unwrap();
    let mut backing = Aligned([0_u8; { include_bytes!("data/quantized_add.tflite").len() } + 1]);
    backing.0[1..].copy_from_slice(&MODEL.0);
    let unaligned_bytes =
        unsafe { core::slice::from_raw_parts(backing.0.as_ptr().add(1), MODEL.0.len()) };
    assert_eq!(
        device
            .load_model(ModelSource::Buffer {
                data: unaligned_bytes,
                format: ModelFormat::Tflite
            })
            .err()
            .unwrap(),
        Error::InvalidModel,
    );
    let model = device.load_model(source()).unwrap();
    let mut raw = [0_u8; 16385];
    let ptr = unsafe { raw.as_mut_ptr().add(1) };
    let misaligned = unsafe { core::slice::from_raw_parts_mut(ptr, 16384) };
    let err = Workspace::try_new(misaligned, req);
    assert_eq!(err.err().unwrap(), Error::InvalidWorkspace);
    let mut short = Aligned([0_u8; 1024]);
    let understated = MemoryRequirements {
        workspace_bytes: short.0.len(),
        ..req
    };
    let short_workspace = Workspace::try_new(&mut short.0, understated).unwrap();
    assert_eq!(
        device
            .create_context(&model, &[spec(&[1, 4])], Some(short_workspace))
            .err()
            .unwrap(),
        Error::InsufficientMemory
    );
    let mut wrongly_aligned = Aligned([0_u8; 16385]);
    let ptr = unsafe { wrongly_aligned.0.as_mut_ptr().add(1) };
    let bad_alignment = unsafe { core::slice::from_raw_parts_mut(ptr, 16384) };
    let understated_alignment = MemoryRequirements {
        workspace_bytes: bad_alignment.len(),
        alignment: 1,
        ..req
    };
    let bad_workspace = Workspace::try_new(bad_alignment, understated_alignment).unwrap();
    assert_eq!(
        device
            .create_context(&model, &[spec(&[1, 4])], Some(bad_workspace))
            .err()
            .unwrap(),
        Error::InvalidWorkspace
    );
    device.unload_model(model).unwrap();
}
