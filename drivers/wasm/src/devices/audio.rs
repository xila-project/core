#[cfg(target_feature = "atomics")]
compile_error!("the browser audio bridge requires single-threaded browser WASM");

use alloc::boxed::Box;
use core::{
    array,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

use audio::{
    AudioSystem, Configuration, GET_CONFIGURATION, GET_VOLUME, GET_WRITABLE, OutputHandle,
    SET_CONFIGURATION, SET_VOLUME, START, STOP, SampleFormat, StreamHandle,
};
use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier, Error,
    MountOperations, Result, Size,
};
use js_sys::{Array, ArrayBuffer, Object, Reflect, Uint8Array};
use nanoaudio_web::{Format, Options, Output, Port, Source, Worklet};
use shared::AnyByLayout;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

const STREAMS: usize = 4;
const STREAM_BYTES: usize = 2048;
const MIX_FRAMES: usize = 128;
const OUTPUTS: usize = 1;
const SINK_BYTES: usize = MIX_FRAMES * 2 * size_of::<f32>() * 3;
const FRAME_BYTES: usize = 2 * size_of::<f32>();
const MIX_BLOCK_BYTES: usize = MIX_FRAMES * FRAME_BYTES;
const MAX_TRANSFER_BYTES: usize = MIX_BLOCK_BYTES;
const MAX_CONTROL_BYTES: usize = 256;
const MAX_CONTROL_MESSAGES: usize = 16;
const OUTPUT_CONFIGURATION: audio::Configuration = Configuration::new(48_000, 2, SampleFormat::F32);

static STARTED: AtomicBool = AtomicBool::new(false);

struct BridgeSlot {
    occupied: AtomicBool,
    id: AtomicU32,
    sent: AtomicU64,
    consumed: AtomicU64,
    ready: AtomicBool,
}

impl BridgeSlot {
    const fn new() -> Self {
        Self {
            occupied: AtomicBool::new(false),
            id: AtomicU32::new(0),
            sent: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            ready: AtomicBool::new(false),
        }
    }

    fn writable(&self) -> usize {
        if !self.ready.load(Ordering::Acquire) {
            return 0;
        }
        let outstanding = self
            .sent
            .load(Ordering::Acquire)
            .saturating_sub(self.consumed.load(Ordering::Acquire));
        STREAM_BYTES.saturating_sub(outstanding.min(STREAM_BYTES as u64) as usize)
    }

    fn reserve(&self, requested: usize) -> usize {
        if !self.ready.load(Ordering::Acquire) {
            return 0;
        }
        loop {
            let sent = self.sent.load(Ordering::Acquire);
            let consumed = self.consumed.load(Ordering::Acquire);
            let outstanding = sent.saturating_sub(consumed);
            let available =
                STREAM_BYTES.saturating_sub(outstanding.min(STREAM_BYTES as u64) as usize);
            let count = requested.min(available);
            if count == 0 {
                return 0;
            }
            if self
                .sent
                .compare_exchange(
                    sent,
                    sent.saturating_add(count as u64),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return count;
            }
        }
    }

    fn undo_reservation(&self, count: usize) {
        self.sent.fetch_sub(count as u64, Ordering::AcqRel);
    }

    fn report_consumed(&self, consumed: u64) {
        self.consumed.fetch_max(
            consumed.min(self.sent.load(Ordering::Acquire)),
            Ordering::AcqRel,
        );
    }
}

struct Bridge {
    slots: [BridgeSlot; STREAMS],
    next_id: AtomicU32,
    pending_controls: AtomicUsize,
    output: Output,
}

unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

impl Bridge {
    fn slot(&self, id: u32) -> Option<&BridgeSlot> {
        self.slots.iter().find(|slot| {
            slot.occupied.load(Ordering::Acquire) && slot.id.load(Ordering::Acquire) == id
        })
    }

    fn allocate(&self) -> Result<(usize, u32)> {
        let (index, slot) = self
            .slots
            .iter()
            .enumerate()
            .find(|(_, slot)| {
                slot.occupied
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            })
            .ok_or(Error::RessourceBusy)?;
        let id = self
            .next_id
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        slot.sent.store(0, Ordering::Relaxed);
        slot.consumed.store(0, Ordering::Relaxed);
        slot.ready.store(false, Ordering::Relaxed);
        slot.id.store(id, Ordering::Release);
        Ok((index, id))
    }

    fn release(&self, slot_index: usize, id: u32) {
        let slot = &self.slots[slot_index];
        if slot.id.load(Ordering::Acquire) == id {
            slot.ready.store(false, Ordering::Release);
            slot.occupied.store(false, Ordering::Release);
        }
    }

    fn send(&self, packet: &Object, transfer: &[ArrayBuffer]) -> Result<()> {
        let transfers = Array::new();
        for buffer in transfer {
            transfers.push(buffer);
        }
        let value: JsValue = packet.clone().into();
        self.output
            .post_with_transfer(&value, &transfers)
            .map_err(|_| Error::InputOutput)
    }

    fn send_control(
        &self,
        id: u32,
        command: ControlCommandIdentifier,
        input: &[u8],
        output_size: usize,
    ) -> Result<()> {
        let (bytes, buffer) = make_bytes(input);
        let packet = packet("control", id);
        set(&packet, "command", &JsValue::from_f64(command.raw() as f64))?;
        set(
            &packet,
            "outputSize",
            &JsValue::from_f64(output_size as f64),
        )?;
        set(&packet, "bytes", bytes.as_ref())?;
        self.pending_controls
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_CONTROL_MESSAGES).then_some(count + 1)
            })
            .map_err(|_| Error::RessourceBusy)?;
        if let Err(error) = self.send(&packet, &[buffer]) {
            self.pending_controls.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }
        Ok(())
    }

    fn on_message(&self, value: JsValue) {
        let Ok(op) = field(&value, "op") else {
            return;
        };
        if op.as_string().as_deref() == Some("control_ack") {
            let _ =
                self.pending_controls
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                        Some(count.saturating_sub(1))
                    });
            return;
        }
        let Ok(id) = number_field(&value, "id") else {
            return;
        };
        let Some(slot) = self.slot(id) else {
            return;
        };
        match op.as_string().as_deref() {
            Some("open_ack") => slot.ready.store(true, Ordering::Release),
            Some("consumed") => {
                if let Ok(consumed) = number_field64(&value, "bytes") {
                    slot.report_consumed(consumed);
                }
            }
            _ => {}
        }
    }
}

pub struct WebAudioDevice {
    bridge: Box<Bridge>,
}

impl WebAudioDevice {
    pub async fn start_from_user_gesture() -> Result<Self> {
        if STARTED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::RessourceBusy);
        }

        let worklet: fn() -> Worklet = xila_audio_worklet;
        let _ = worklet;
        let configuration = nanoaudio_web::Configuration::new(48_000, 2, Format::F32);
        let output = match Output::open_with(
            configuration,
            "xila_audio_worklet",
            Options::default(),
        )
        .await
        {
            Ok(output) => output,
            Err(_) => {
                STARTED.store(false, Ordering::Release);
                return Err(Error::InputOutput);
            }
        };
        if output.play().await.is_err() {
            drop(output);
            STARTED.store(false, Ordering::Release);
            return Err(Error::InputOutput);
        }

        let bridge = Box::new(Bridge {
            slots: array::from_fn(|_| BridgeSlot::new()),
            next_id: AtomicU32::new(0),
            pending_controls: AtomicUsize::new(0),
            output,
        });
        let bridge_ptr = (&*bridge) as *const Bridge;
        bridge.output.set_on_message(move |value| {
            let bridge = unsafe { &*bridge_ptr };
            bridge.on_message(value);
        });

        Ok(Self { bridge })
    }

    fn stream<'a>(&self, context: &'a Context) -> Result<&'a StreamProxy> {
        context
            .get_private_data_of_type::<StreamProxy>()
            .ok_or(Error::InvalidContext)
    }

    fn post_open(&self, id: u32) -> Result<()> {
        let packet = packet("open", id);
        self.bridge.send(&packet, &[])
    }

    fn post_close(&self, id: u32) {
        let packet = packet("close", id);
        let _ = self.bridge.send(&packet, &[]);
    }
}

impl Drop for WebAudioDevice {
    fn drop(&mut self) {
        STARTED.store(false, Ordering::Release);
    }
}

struct StreamProxy {
    slot: usize,
    id: u32,
    configuration: Configuration,
    volume: u8,
}

impl BaseOperations for WebAudioDevice {
    fn open(&self, context: &mut Context) -> Result<()> {
        if context.get_private_data().is_some() {
            return Err(Error::InvalidContext);
        }
        let (slot, id) = self.bridge.allocate()?;
        if let Err(error) = self.post_open(id) {
            self.bridge.release(slot, id);
            return Err(error);
        }
        context.set_private_data(Box::new(StreamProxy {
            slot,
            id,
            configuration: OUTPUT_CONFIGURATION,
            volume: 100,
        }));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        let proxy = context
            .take_private_data_of_type::<StreamProxy>()
            .ok_or(Error::InvalidContext)?;
        self.post_close(proxy.id);
        self.bridge.release(proxy.slot, proxy.id);
        Ok(())
    }

    fn read(&self, _context: &mut Context, _buffer: &mut [u8], _position: Size) -> Result<usize> {
        Err(Error::UnsupportedOperation)
    }

    fn write(&self, context: &mut Context, bytes: &[u8], _position: Size) -> Result<usize> {
        let proxy = self.stream(context)?;
        if !bytes.len().is_multiple_of(FRAME_BYTES) {
            return Err(Error::InvalidParameter);
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let slot = &self.bridge.slots[proxy.slot];
        let count = slot.reserve(bytes.len().min(MAX_TRANSFER_BYTES) / FRAME_BYTES * FRAME_BYTES);
        if count == 0 {
            return Err(Error::RessourceBusy);
        }
        let (data, buffer) = make_bytes(&bytes[..count]);
        let packet = packet("write", proxy.id);
        if set(&packet, "bytes", data.as_ref()).is_err()
            || self.bridge.send(&packet, &[buffer]).is_err()
        {
            slot.undo_reservation(count);
            return Err(Error::InputOutput);
        }
        Ok(count)
    }

    fn flush(&self, context: &mut Context) -> Result<()> {
        let proxy = self.stream(context)?;
        let slot = &self.bridge.slots[proxy.slot];
        if slot.writable() == STREAM_BYTES {
            Ok(())
        } else {
            Err(Error::RessourceBusy)
        }
    }

    fn control(
        &self,
        context: &mut Context,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        let proxy = context
            .get_private_data_mutable_of_type::<StreamProxy>()
            .ok_or(Error::InvalidContext)?;
        if input.get_size() > MAX_CONTROL_BYTES || output.get_size() > MAX_CONTROL_BYTES {
            return Err(Error::InvalidParameter);
        }
        let slot = &self.bridge.slots[proxy.slot];
        match command {
            SET_CONFIGURATION::IDENTIFIER => {
                validate::<SET_CONFIGURATION>(input, output)?;
                let value = *SET_CONFIGURATION::cast_input(input)?;
                value.validate()?;
                if value != OUTPUT_CONFIGURATION {
                    return Err(Error::UnsupportedOperation);
                }
                self.bridge
                    .send_control(proxy.id, command, any_bytes(input), output.get_size())?;
                proxy.configuration = value;
            }
            GET_CONFIGURATION::IDENTIFIER => {
                validate::<GET_CONFIGURATION>(input, output)?;
                *GET_CONFIGURATION::cast_output(output)? = proxy.configuration;
                self.bridge
                    .send_control(proxy.id, command, any_bytes(input), output.get_size())?;
            }
            GET_WRITABLE::IDENTIFIER => {
                validate::<GET_WRITABLE>(input, output)?;
                *GET_WRITABLE::cast_output(output)? = slot.writable().min(u32::MAX as usize) as u32;
                self.bridge
                    .send_control(proxy.id, command, any_bytes(input), output.get_size())?;
            }
            SET_VOLUME::IDENTIFIER => {
                validate::<SET_VOLUME>(input, output)?;
                let value = *SET_VOLUME::cast_input(input)?;
                if value > 100 {
                    return Err(Error::InvalidParameter);
                }
                self.bridge
                    .send_control(proxy.id, command, any_bytes(input), output.get_size())?;
                proxy.volume = value;
            }
            GET_VOLUME::IDENTIFIER => {
                validate::<GET_VOLUME>(input, output)?;
                *GET_VOLUME::cast_output(output)? = proxy.volume;
                self.bridge
                    .send_control(proxy.id, command, any_bytes(input), output.get_size())?;
            }
            START::IDENTIFIER => {
                validate::<START>(input, output)?;
                self.bridge
                    .send_control(proxy.id, command, any_bytes(input), output.get_size())?;
            }
            STOP::IDENTIFIER => {
                validate::<STOP>(input, output)?;
                self.bridge
                    .send_control(proxy.id, command, any_bytes(input), output.get_size())?;
            }
            _ => return Err(Error::UnsupportedOperation),
        }
        Ok(())
    }

    fn clone_context(&self, _context: &Context) -> Result<Context> {
        Err(Error::UnsupportedOperation)
    }
}

impl MountOperations for WebAudioDevice {}
impl CharacterDevice for WebAudioDevice {}

struct WorkletSource {
    system: Pin<&'static AudioSystem<STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>>,
    output: OutputHandle<'static, STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>,
    streams: [Option<StreamHandle<'static, STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>>;
        STREAMS],
    ids: [u32; STREAMS],
    accepted: [u64; STREAMS],
    reported: [u64; STREAMS],
    scratch: [u8; MIX_BLOCK_BYTES],
    control_input: AlignedControl,
    control_output: AlignedControl,
}

#[repr(align(16))]
struct AlignedControl([u8; MAX_CONTROL_BYTES]);

impl WorkletSource {
    fn new() -> Self {
        let system: &'static AudioSystem<STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES> =
            Box::leak(Box::new(
                AudioSystem::<STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>::new()
                    .expect("valid browser audio capacities"),
            ));
        let system = unsafe { Pin::new_unchecked(system) };
        let output = system
            .open_output()
            .expect("browser output capacity is available");
        Self {
            system,
            output,
            streams: array::from_fn(|_| None),
            ids: [0; STREAMS],
            accepted: [0; STREAMS],
            reported: [0; STREAMS],
            scratch: [0; MIX_BLOCK_BYTES],
            control_input: AlignedControl([0; MAX_CONTROL_BYTES]),
            control_output: AlignedControl([0; MAX_CONTROL_BYTES]),
        }
    }

    fn send_open_ack(&self, id: u32, port: &Port) {
        let packet = packet("open_ack", id);
        let _ = port.post(&packet.into());
    }

    fn dispatch(&mut self, value: JsValue, port: &Port) {
        let Ok(op) = field(&value, "op") else {
            return;
        };
        let Ok(id) = number_field(&value, "id") else {
            return;
        };
        match op.as_string().as_deref() {
            Some("open") => {
                if self.ids.contains(&id) || id == 0 {
                    return;
                }
                let Some(index) = self.ids.iter().position(|current| *current == 0) else {
                    return;
                };
                if let Ok(stream) = self.system.get_ref().open_stream() {
                    self.streams[index] = Some(stream);
                    self.ids[index] = id;
                    self.accepted[index] = 0;
                    self.reported[index] = 0;
                    self.send_open_ack(id, port);
                }
            }
            Some("write") => {
                let Some(index) = self.ids.iter().position(|current| *current == id) else {
                    return;
                };
                let Ok(bytes_value) = field(&value, "bytes") else {
                    return;
                };
                let bytes = Uint8Array::new(&bytes_value);
                let length = bytes.length() as usize;
                if length > MAX_TRANSFER_BYTES || !length.is_multiple_of(FRAME_BYTES) {
                    return;
                }
                bytes.copy_to(&mut self.scratch[..length]);
                let Some(stream) = self.streams[index].as_ref() else {
                    return;
                };
                if let Ok(count) = stream.write(&self.scratch[..length]) {
                    self.accepted[index] = self.accepted[index].saturating_add(count as u64);
                }
            }
            Some("close") => {
                if let Some(index) = self.ids.iter().position(|current| *current == id) {
                    self.streams[index] = None;
                    self.ids[index] = 0;
                    self.accepted[index] = 0;
                    self.reported[index] = 0;
                }
            }
            Some("control") => self.control(&value, id, port),
            _ => {}
        }
    }

    fn control(&mut self, value: &JsValue, id: u32, port: &Port) {
        let Some(index) = self.ids.iter().position(|current| *current == id) else {
            return;
        };
        let Ok(command_raw) = number_field(value, "command") else {
            return;
        };
        let Ok(output_size) = number_field(value, "outputSize") else {
            return;
        };
        let Ok(bytes_value) = field(value, "bytes") else {
            return;
        };
        let bytes = Uint8Array::new(&bytes_value);
        let input_size = bytes.length() as usize;
        let output_size = output_size as usize;
        if input_size > MAX_CONTROL_BYTES || output_size > MAX_CONTROL_BYTES {
            return;
        }
        bytes.copy_to(&mut self.control_input.0[..input_size]);
        self.control_output.0[..output_size].fill(0);
        let command = ControlCommandIdentifier::from_raw(command_raw);
        let input =
            unsafe { AnyByLayout::from_raw_parts(self.control_input.0.as_mut_ptr(), input_size) };
        let output =
            unsafe { AnyByLayout::from_raw_parts(self.control_output.0.as_mut_ptr(), output_size) };
        let result = match command {
            SET_CONFIGURATION::IDENTIFIER => apply::<SET_CONFIGURATION>(
                &self.streams[index],
                input,
                output,
                |stream, config, _| {
                    config.validate()?;
                    if *config != OUTPUT_CONFIGURATION {
                        return Err(Error::UnsupportedOperation);
                    }
                    let _ = stream.configuration();
                    Ok(())
                },
            ),
            GET_CONFIGURATION::IDENTIFIER => apply::<GET_CONFIGURATION>(
                &self.streams[index],
                input,
                output,
                |stream, _, result| {
                    *result = stream.configuration();
                    Ok(())
                },
            ),
            GET_WRITABLE::IDENTIFIER => {
                apply::<GET_WRITABLE>(&self.streams[index], input, output, |stream, _, result| {
                    *result = stream.writable().min(u32::MAX as usize) as u32;
                    Ok(())
                })
            }
            SET_VOLUME::IDENTIFIER => {
                apply::<SET_VOLUME>(&self.streams[index], input, output, |stream, volume, _| {
                    stream.set_volume(*volume)
                })
            }
            GET_VOLUME::IDENTIFIER => {
                apply::<GET_VOLUME>(&self.streams[index], input, output, |stream, _, result| {
                    *result = stream.volume()?;
                    Ok(())
                })
            }
            START::IDENTIFIER => {
                apply::<START>(&self.streams[index], input, output, |stream, _, _| {
                    stream.start()
                })
            }
            STOP::IDENTIFIER => {
                apply::<STOP>(&self.streams[index], input, output, |stream, _, _| {
                    stream.stop()
                })
            }
            _ => Err(Error::UnsupportedOperation),
        };
        let packet = packet("control_ack", id);
        let _ = set(&packet, "ok", &JsValue::from_bool(result.is_ok()));
        let _ = port.post(&packet.into());
    }

    fn report_consumed(&mut self, port: &Port) {
        for index in 0..STREAMS {
            let Some(stream) = self.streams[index].as_ref() else {
                continue;
            };
            let queued = STREAM_BYTES.saturating_sub(stream.writable());
            let consumed = self.accepted[index].saturating_sub(queued as u64);
            if consumed > self.reported[index] {
                self.reported[index] = consumed;
                let packet = packet("consumed", self.ids[index]);
                let _ = set(&packet, "bytes", &JsValue::from_f64(consumed as f64));
                let _ = port.post(&packet.into());
            }
        }
    }
}

impl Source for WorkletSource {
    fn fill(
        &mut self,
        producer: &mut nanoaudio_web::Producer<'_, { nanoaudio_web::CAPACITY }>,
        port: &Port,
    ) {
        let _ = self.system.as_ref().mix_next_block();
        let count = self.output.with_consumer(|consumer| {
            consumer.read(&mut self.scratch[..producer.free().min(MIX_BLOCK_BYTES)])
        });
        producer.write(&self.scratch[..count]);
        self.report_consumed(port);
    }

    fn message(&mut self, data: JsValue, port: &Port) {
        self.dispatch(data, port);
    }
}

#[wasm_bindgen]
pub fn xila_audio_worklet() -> Worklet {
    Worklet::output(WorkletSource::new())
}

fn apply<C: ControlCommand>(
    stream: &Option<StreamHandle<'static, STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>>,
    input: &AnyByLayout,
    output: &mut AnyByLayout,
    operation: impl FnOnce(
        &StreamHandle<'static, STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>,
        &C::Input,
        &mut C::Output,
    ) -> Result<()>,
) -> Result<()> {
    validate::<C>(input, output)?;
    let stream = stream.as_ref().ok_or(Error::InvalidContext)?;
    operation(stream, C::cast_input(input)?, C::cast_output(output)?)
}

fn validate<C: ControlCommand>(input: &AnyByLayout, output: &AnyByLayout) -> Result<()> {
    if input.get_size() != size_of::<C::Input>()
        || output.get_size() != size_of::<C::Output>()
        || !(core::ptr::from_ref(input).cast::<u8>() as usize)
            .is_multiple_of(core::mem::align_of::<C::Input>())
        || !(core::ptr::from_ref(output).cast::<u8>() as usize)
            .is_multiple_of(core::mem::align_of::<C::Output>())
    {
        return Err(Error::InvalidParameter);
    }
    Ok(())
}

fn set(object: &Object, key: &str, value: &JsValue) -> Result<()> {
    Reflect::set(object, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|_| Error::InputOutput)
}

fn field(value: &JsValue, key: &str) -> core::result::Result<JsValue, JsValue> {
    Reflect::get(value, &JsValue::from_str(key))
}

fn number_field(value: &JsValue, key: &str) -> core::result::Result<u32, JsValue> {
    let number = field(value, key)?
        .as_f64()
        .ok_or_else(|| JsValue::from_str("invalid number"))?;
    if !number.is_finite() || number < 0.0 || number > u32::MAX as f64 || number.fract() != 0.0 {
        return Err(JsValue::from_str("invalid number"));
    }
    Ok(number as u32)
}

fn number_field64(value: &JsValue, key: &str) -> core::result::Result<u64, JsValue> {
    let number = field(value, key)?
        .as_f64()
        .ok_or_else(|| JsValue::from_str("invalid number"))?;
    if !number.is_finite() || number < 0.0 || number > u64::MAX as f64 || number.fract() != 0.0 {
        return Err(JsValue::from_str("invalid number"));
    }
    Ok(number as u64)
}

fn packet(op: &str, id: u32) -> Object {
    let packet = Object::new();
    let _ = Reflect::set(&packet, &"op".into(), &op.into());
    let _ = Reflect::set(&packet, &"id".into(), &JsValue::from_f64(id as f64));
    packet
}

fn make_bytes(bytes: &[u8]) -> (Uint8Array, ArrayBuffer) {
    let array = Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let buffer = array.buffer();
    (array, buffer)
}

fn any_bytes(value: &AnyByLayout) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts((value as *const AnyByLayout).cast::<u8>(), value.get_size())
    }
}

#[cfg(test)]
mod tests {
    use super::BridgeSlot;

    #[test]
    fn bounds_outstanding_pcm_and_restores_only_consumed_credit() {
        let credit = BridgeSlot::new();
        credit
            .ready
            .store(true, core::sync::atomic::Ordering::Release);

        assert_eq!(credit.writable(), 2048);
        assert_eq!(credit.reserve(1536), 1536);
        assert_eq!(credit.writable(), 512);
        credit.report_consumed(512);
        assert_eq!(credit.writable(), 1024);
        assert_eq!(credit.reserve(2048), 1024);
        assert_eq!(credit.writable(), 0);
    }

    #[test]
    fn ignores_old_consumption_reports() {
        let credit = BridgeSlot::new();
        credit
            .ready
            .store(true, core::sync::atomic::Ordering::Release);

        credit.reserve(1024);
        credit.report_consumed(512);
        credit.report_consumed(256);

        assert_eq!(credit.writable(), 1536);
    }
}
