use alloc::{boxed::Box, string::String};
use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
use graphics::{InputData, Point, State};
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};
use task::block_on;
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::{HtmlCanvasElement, MouseEvent, TouchEvent};

struct Inner {
    position: Point,
    state: State,
}

pub struct MouseDevice(Arc<RwLock<CriticalSectionRawMutex, Inner>>);

impl MouseDevice {
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        let inner = Arc::new(RwLock::new(Inner {
            position: Point::new(0, 0),
            state: State::default(),
        }));

        let inner_clone = inner.clone();
        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let mut inner = block_on(inner_clone.write());

            inner.position = Point::new(event.client_x() as _, event.client_y() as _);
            inner.state = if event.buttons() == 0 {
                State::default()
            } else {
                State::Pressed
            };
        }) as Box<dyn FnMut(MouseEvent)>);

        canvas
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add mousedown event listener")?;
        canvas
            .add_event_listener_with_callback("mouseup", closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add mouseup event listener")?;
        canvas
            .add_event_listener_with_callback("mousemove", closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add mousemove event listener")?;
        canvas
            .add_event_listener_with_callback("mouseleave", closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add mouseleave event listener")?;
        closure.forget(); // Prevent memory leak by keeping the closure alive

        // Add touch event support
        let inner_clone = inner.clone();
        let touch_closure = Closure::wrap(Box::new(move |event: TouchEvent| {
            event.prevent_default(); // Prevent default touch behavior

            let mut inner = block_on(inner_clone.write());

            // Get the first touch point
            if let Some(touch) = event.touches().get(0) {
                inner.position = Point::new(touch.client_x() as _, touch.client_y() as _);

                // Determine state based on event type
                inner.state = match event.type_().as_str() {
                    "touchstart" | "touchmove" => State::Pressed,
                    _ => State::default(), // touchend, touchcancel
                };
            } else if event.type_() == "touchend" || event.type_() == "touchcancel" {
                // No touches remaining, release
                inner.state = State::default();
            }
        }) as Box<dyn FnMut(TouchEvent)>);

        canvas
            .add_event_listener_with_callback("touchstart", touch_closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add touchstart event listener")?;
        canvas
            .add_event_listener_with_callback("touchend", touch_closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add touchend event listener")?;
        canvas
            .add_event_listener_with_callback("touchmove", touch_closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add touchmove event listener")?;
        canvas
            .add_event_listener_with_callback("touchcancel", touch_closure.as_ref().unchecked_ref())
            .map_err(|_| "Failed to add touchcancel event listener")?;
        touch_closure.forget(); // Prevent memory leak by keeping the closure alive

        Ok(Self(inner))
    }
}

impl DirectBaseOperations for MouseDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            let data: &mut InputData = buffer
                .try_into()
                .map_err(|_| file_system::Error::InvalidParameter)?;

            let inner = block_on(self.0.read());

            data.set(inner.position, inner.state);

            Ok(0)
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::Error::UnsupportedOperation))
    }
}

impl MountOperations for MouseDevice {}

impl DirectCharacterDevice for MouseDevice {}
