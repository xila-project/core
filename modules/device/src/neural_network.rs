/// Serialized or backend-generated model representation accepted by a backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFormat {
    Tflite,
    Onnx,
    Burn,
    EspDl,
    Other(&'static str),
}

#[derive(Clone, Copy)]
pub struct StaticModel<'a> {
    pub model: &'a dyn AotModel,
    pub format: ModelFormat,
}

/// Marker implemented by generated or statically linked model objects.
pub trait AotModel {}

#[derive(Clone, Copy, Debug)]
pub struct ExternalModel<'a> {
    pub location: &'a str,
    pub format: ModelFormat,
}

#[derive(Clone, Copy)]
pub enum ModelSource<'a> {
    Buffer { data: &'a [u8], format: ModelFormat },
    Static(StaticModel<'a>),
    External(ExternalModel<'a>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSourceKind {
    Buffer,
    StaticAot,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TensorType {
    F32,
    F16,
    I8,
    U8,
    I16,
    I32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantization {
    pub scale: f32,
    pub zero_point: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct TensorSpec<'a> {
    pub shape: &'a [usize],
    pub data_type: TensorType,
    pub quantization: Option<Quantization>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TensorData<'a> {
    F32(&'a [f32]),
    F16(&'a [u16]),
    I8(&'a [i8]),
    U8(&'a [u8]),
    I16(&'a [i16]),
    I32(&'a [i32]),
}

#[derive(Clone, Copy, Debug)]
pub struct TensorView<'a> {
    pub spec: TensorSpec<'a>,
    pub data: TensorData<'a>,
}

/// A typed input binding valid for the duration of a synchronous compute call.
#[derive(Clone, Copy, Debug)]
pub struct InputBinding<'a> {
    pub index: usize,
    pub tensor: TensorView<'a>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub formats: &'static [ModelFormat],
    pub tensor_types: &'static [TensorType],
    pub source_modes: &'static [ModelSourceKind],
    pub accelerator: Accelerator,
    pub asynchronous: bool,
    pub zero_copy: bool,
    pub quantization: bool,
    pub runtime_models: bool,
    pub static_models: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Accelerator {
    #[default]
    Cpu,
    Gpu,
    Npu,
    Dsp,
    Other,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryRequirements {
    pub model_bytes: usize,
    pub context_bytes: usize,
    pub workspace_bytes: usize,
    pub alignment: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    InvalidModel,
    InvalidTensor,
    InsufficientMemory,
    Backend,
}

/// Synchronous, backend-owned neural-network model lifecycle.
pub trait NeuralNetworkDevice {
    type Model<'model>
    where
        Self: 'model;
    type Context;

    fn capabilities(&self) -> Capabilities;
    fn memory_requirements(&self, source: ModelSource<'_>) -> Result<MemoryRequirements, Error>;
    fn load_model<'model>(
        &mut self,
        source: ModelSource<'model>,
    ) -> Result<Self::Model<'model>, Error>;
    fn unload_model(&mut self, model: Self::Model<'_>) -> Result<(), Error>;
    fn create_context(
        &mut self,
        model: &Self::Model<'_>,
        tensors: &[TensorSpec<'_>],
    ) -> Result<Self::Context, Error>;
    fn destroy_context(&mut self, context: Self::Context) -> Result<(), Error>;
    fn bind_input<'input>(&self, index: usize, tensor: TensorView<'input>) -> InputBinding<'input> {
        InputBinding { index, tensor }
    }
    fn compute(
        &mut self,
        context: &mut Self::Context,
        inputs: &[InputBinding<'_>],
    ) -> Result<(), Error>;
    fn output<'a>(
        &'a self,
        context: &'a Self::Context,
        index: usize,
    ) -> Result<TensorView<'a>, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GeneratedModel;
    impl AotModel for GeneratedModel {}

    struct MockModel<'a> {
        source: ModelSource<'a>,
    }
    struct MockContext {
        output: [i8; 3],
        input_address: Option<usize>,
    }
    struct Mock;

    impl NeuralNetworkDevice for Mock {
        type Model<'a> = MockModel<'a>;
        type Context = MockContext;

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                tensor_types: &[TensorType::I8],
                source_modes: &[
                    ModelSourceKind::Buffer,
                    ModelSourceKind::StaticAot,
                    ModelSourceKind::External,
                ],
                ..Capabilities::default()
            }
        }
        fn memory_requirements(&self, _: ModelSource<'_>) -> Result<MemoryRequirements, Error> {
            Ok(MemoryRequirements::default())
        }
        fn load_model<'a>(&mut self, source: ModelSource<'a>) -> Result<Self::Model<'a>, Error> {
            Ok(MockModel { source })
        }
        fn unload_model(&mut self, _: Self::Model<'_>) -> Result<(), Error> {
            Ok(())
        }
        fn create_context(
            &mut self,
            _: &Self::Model<'_>,
            _: &[TensorSpec<'_>],
        ) -> Result<Self::Context, Error> {
            Ok(MockContext {
                output: [0; 3],
                input_address: None,
            })
        }
        fn destroy_context(&mut self, _: Self::Context) -> Result<(), Error> {
            Ok(())
        }
        fn bind_input<'a>(&self, index: usize, tensor: TensorView<'a>) -> InputBinding<'a> {
            InputBinding { index, tensor }
        }
        fn compute(
            &mut self,
            context: &mut MockContext,
            inputs: &[InputBinding<'_>],
        ) -> Result<(), Error> {
            let input = inputs.first().ok_or(Error::InvalidTensor)?;
            match input.tensor.data {
                TensorData::I8(values) => {
                    context.input_address = Some(values.as_ptr() as usize);
                    context.output.copy_from_slice(values);
                    Ok(())
                }
                _ => Err(Error::InvalidTensor),
            }
        }
        fn output<'a>(
            &'a self,
            context: &'a MockContext,
            _: usize,
        ) -> Result<TensorView<'a>, Error> {
            Ok(TensorView {
                spec: TensorSpec {
                    shape: &[3],
                    data_type: TensorType::I8,
                    quantization: None,
                },
                data: TensorData::I8(&context.output),
            })
        }
    }

    #[test]
    fn all_model_sources_survive_load_context_and_unload_without_copying() {
        let mut device = Mock;
        let model_data = [1_u8, 2, 3];
        let generated = GeneratedModel;
        let sources = [
            ModelSource::Buffer {
                data: &model_data,
                format: ModelFormat::Tflite,
            },
            ModelSource::Static(StaticModel {
                model: &generated,
                format: ModelFormat::Burn,
            }),
            ModelSource::External(ExternalModel {
                location: "flash:/model",
                format: ModelFormat::Tflite,
            }),
        ];
        for source in sources {
            let model = device.load_model(source).unwrap();
            assert!(matches!(
                model.source,
                ModelSource::Buffer { .. } | ModelSource::Static(_) | ModelSource::External(_)
            ));
            match model.source {
                ModelSource::Buffer { data, format } => {
                    assert!(core::ptr::eq(data.as_ptr(), model_data.as_ptr()));
                    assert_eq!(format, ModelFormat::Tflite);
                }
                ModelSource::Static(static_model) => {
                    assert!(core::ptr::eq(
                        static_model.model,
                        &generated as &dyn AotModel
                    ));
                    assert_eq!(static_model.format, ModelFormat::Burn);
                }
                ModelSource::External(external) => {
                    assert!(core::ptr::eq(
                        external.location.as_ptr(),
                        "flash:/model".as_ptr()
                    ));
                    assert_eq!(external.format, ModelFormat::Tflite);
                }
            }
            let shape = [1, 3];
            let spec = TensorSpec {
                shape: &shape,
                data_type: TensorType::I8,
                quantization: Some(Quantization {
                    scale: 0.25,
                    zero_point: -3,
                }),
            };
            let mut context = device.create_context(&model, &[spec]).unwrap();
            let input = [5_i8, 6, 7];
            let bindings = [device.bind_input(
                0,
                TensorView {
                    spec,
                    data: TensorData::I8(&input),
                },
            )];
            assert_eq!(device.capabilities().tensor_types, &[TensorType::I8]);
            assert_eq!(
                device.capabilities().source_modes,
                &[
                    ModelSourceKind::Buffer,
                    ModelSourceKind::StaticAot,
                    ModelSourceKind::External
                ]
            );
            device.compute(&mut context, &bindings).unwrap();
            assert_eq!(context.input_address, Some(input.as_ptr() as usize));
            assert_eq!(
                device.output(&context, 0).unwrap().data,
                TensorData::I8(&[5, 6, 7])
            );
            device.destroy_context(context).unwrap();
            device.unload_model(model).unwrap();
        }
    }
}
