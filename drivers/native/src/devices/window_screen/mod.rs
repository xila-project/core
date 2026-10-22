pub mod control;
mod event_loop;
mod inner_window;
mod keyboard;
mod pointer;
mod screen;
mod screenshot;
mod window;

use graphics::{InputData, Key, Point, State};
use synchronization::rwlock::RwLock;
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use inner_window::*;
use keyboard::*;
use pointer::*;
use screen::*;
use window::*;

use crate::window_screen::event_loop::Runner;

#[cfg(unix)]
pub use control::device::DebugTransportDevice;
pub use control::{ControlError, ControlOperations};
pub use inner_window::FramebufferSnapshot;
pub use screenshot::{encode_png, write_png};

pub fn control_operations() -> ControlOperations<'static> {
    ControlOperations::new(&INNER_WINDOW, &POINTER_RWLOCK, KEYBOARD_CHANNEL.sender())
}

pub async fn framebuffer_snapshot() -> Result<FramebufferSnapshot, String> {
    INNER_WINDOW.snapshot_rgba().await
}

pub fn debug_screen_device() -> ScreenDevice<'static> {
    ScreenDevice::new(&INNER_WINDOW)
}

pub fn debug_pointer_device() -> PointerDevice {
    PointerDevice::new(&POINTER_RWLOCK)
}

pub fn debug_keyboard_device() -> KeyboardDevice<CriticalSectionRawMutex, 512> {
    KeyboardDevice::new(KEYBOARD_CHANNEL.receiver(), KEYBOARD_CHANNEL.sender())
}

static KEYBOARD_CHANNEL: Channel<CriticalSectionRawMutex, (Key, State), 512> = Channel::new();
static POINTER_RWLOCK: RwLock<CriticalSectionRawMutex, InputData> =
    RwLock::new(InputData::default_constant());
static INNER_WINDOW: InnerWindow = InnerWindow::new();

pub async fn new(
    resolution: Point,
) -> Result<
    (
        ScreenDevice<'static>,
        PointerDevice,
        KeyboardDevice<CriticalSectionRawMutex, 512>,
        Runner<'static>,
    ),
    String,
> {
    let window = Window::new(
        resolution,
        &INNER_WINDOW,
        KEYBOARD_CHANNEL.sender(),
        &POINTER_RWLOCK,
    );

    let event_loop = Runner::new(window);

    let screen_device = ScreenDevice::new(&INNER_WINDOW);

    let pointer_device = PointerDevice::new(&POINTER_RWLOCK);

    let keyboard_device =
        KeyboardDevice::new(KEYBOARD_CHANNEL.receiver(), KEYBOARD_CHANNEL.sender());

    Ok((screen_device, pointer_device, keyboard_device, event_loop))
}
