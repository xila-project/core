#![cfg(target_arch = "wasm32")]

use abi_definitions as _;
use drivers_wasm::devices::graphics::{CanvasScreenDevice, KeyboardDevice};
use file_system::{ControlCommand, DirectBaseOperations};
use graphics::{Area, ColorARGB8888, InputData, Key, Point, SET_DRAWING_AREA, State, WAS_RESIZED};
use shared::AnyByLayout;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, KeyboardEvent, KeyboardEventInit};

wasm_bindgen_test_configure!(run_in_browser);
drivers_wasm::memory::instantiate_global_allocator!();

fn canvas() -> HtmlCanvasElement {
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas: HtmlCanvasElement = document
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_tab_index(0);
    document.body().unwrap().append_child(&canvas).unwrap();
    canvas.focus().unwrap();
    canvas
}

fn key(canvas: &web_sys::EventTarget, key: &str, modifier: &str) -> KeyboardEvent {
    let options = KeyboardEventInit::new();
    options.set_key(key);
    options.set_bubbles(true);
    options.set_cancelable(true);
    options.set_ctrl_key(modifier == "Control");
    options.set_meta_key(modifier == "Meta");
    options.set_alt_key(modifier == "Alt");
    options.set_is_composing(modifier == "Composing");
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &options).unwrap();
    canvas.dispatch_event(&event).unwrap();
    event
}

fn read(device: &KeyboardDevice) -> InputData {
    let mut data = InputData::default();
    device.read_sync(data.as_mut(), 0).unwrap();
    data
}

#[wasm_bindgen_test]
fn application_keys_cancel_browser_defaults_and_reach_xila() {
    let canvas = canvas();
    let device = Box::leak(Box::new(KeyboardDevice::new(&canvas).unwrap()));
    // Browser quick-find, scrolling, history navigation, and cursor movement.
    for name in ["/", " ", "ArrowDown", "PageDown", "Home", "Backspace"] {
        assert!(key(&canvas, name, "").default_prevented(), "{name}");
        assert_eq!(read(device).state, State::Pressed);
        assert_eq!(read(device).state, State::Released);
    }
    canvas.remove();
}

#[wasm_bindgen_test]
fn shortcuts_composition_and_external_controls_stay_with_browser() {
    let canvas = canvas();
    let device = Box::leak(Box::new(KeyboardDevice::new(&canvas).unwrap()));
    for modifier in ["Control", "Meta", "Alt", "Composing"] {
        assert!(!key(&canvas, "f", modifier).default_prevented());
        assert_eq!(read(device).key, Key::Character(0));
    }
    let options = KeyboardEventInit::new();
    options.set_key("/");
    options.set_cancelable(true);
    let handled_key =
        KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &options).unwrap();
    handled_key.prevent_default();
    canvas.dispatch_event(&handled_key).unwrap();
    assert_eq!(read(device).key, Key::Character(0));
    // Tab lets users leave the canvas using the keyboard.
    assert!(!key(&canvas, "Tab", "").default_prevented());
    let document = web_sys::window().unwrap().document().unwrap();
    for tag in ["input", "textarea", "button", "div"] {
        let element = document.create_element(tag).unwrap();
        if tag == "div" {
            element.set_attribute("contenteditable", "true").unwrap();
        }
        document.body().unwrap().append_child(&element).unwrap();
        element
            .dyn_ref::<web_sys::HtmlElement>()
            .unwrap()
            .focus()
            .unwrap();
        assert!(!key(element.unchecked_ref(), " ", "").default_prevented());
        assert_eq!(read(device).key, Key::Character(0));
        element.remove();
    }
    canvas.remove();
}

#[wasm_bindgen_test]
async fn unchanged_resize_preserves_the_canvas_bitmap() {
    let canvas = canvas();
    let window = web_sys::window().unwrap();
    canvas.set_width(window.inner_width().unwrap().as_f64().unwrap() as u32);
    canvas.set_height(window.inner_height().unwrap().as_f64().unwrap() as u32);
    let device = CanvasScreenDevice::new(canvas.clone()).await.unwrap();
    let context: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into()
        .unwrap();
    context.set_fill_style_str("#ff0000");
    context.fill_rect(0.0, 0.0, 1.0, 1.0);
    window
        .dispatch_event(&web_sys::Event::new("resize").unwrap())
        .unwrap();
    let pixel = context.get_image_data(0.0, 0.0, 1.0, 1.0).unwrap().data();
    assert_eq!(&pixel.0[..3], &[255, 0, 0]);
    let mut resized = false;
    device
        .control_sync(
            WAS_RESIZED::IDENTIFIER,
            AnyByLayout::NONE,
            AnyByLayout::from_mutable(&mut resized),
        )
        .unwrap();
    assert!(!resized);
    canvas.remove();
}

#[wasm_bindgen_test]
fn paste_is_delivered_only_to_the_application_canvas() {
    let canvas = canvas();
    let device = Box::leak(Box::new(KeyboardDevice::new(&canvas).unwrap()));
    let document = web_sys::window().unwrap().document().unwrap();
    let input = document.create_element("input").unwrap();
    document.body().unwrap().append_child(&input).unwrap();
    input
        .dyn_ref::<web_sys::HtmlElement>()
        .unwrap()
        .focus()
        .unwrap();
    let clipboard = web_sys::DataTransfer::new().unwrap();
    clipboard.set_data("text", "abc").unwrap();
    let options = web_sys::ClipboardEventInit::new();
    options.set_clipboard_data(Some(&clipboard));
    options.set_bubbles(true);
    options.set_cancelable(true);
    let event = web_sys::ClipboardEvent::new_with_event_init_dict("paste", &options).unwrap();
    input.dispatch_event(&event).unwrap();
    assert!(!event.default_prevented());
    assert_eq!(read(device).key, Key::Character(0));
    assert!(!key(&canvas, " ", "").default_prevented());
    let unfocused_paste =
        web_sys::ClipboardEvent::new_with_event_init_dict("paste", &options).unwrap();
    canvas.dispatch_event(&unfocused_paste).unwrap();
    assert!(!unfocused_paste.default_prevented());
    assert_eq!(read(device).key, Key::Character(0));
    canvas.focus().unwrap();
    let handled_paste =
        web_sys::ClipboardEvent::new_with_event_init_dict("paste", &options).unwrap();
    handled_paste.prevent_default();
    canvas.dispatch_event(&handled_paste).unwrap();
    assert_eq!(read(device).key, Key::Character(0));
    let event = web_sys::ClipboardEvent::new_with_event_init_dict("paste", &options).unwrap();
    canvas.dispatch_event(&event).unwrap();
    assert!(event.default_prevented());
    for character in b"abc" {
        assert_eq!(read(device).key, Key::Character(*character));
        assert_eq!(read(device).state, State::Released);
    }
    input.remove();
    canvas.remove();
}

#[wasm_bindgen_test]
async fn full_screen_canvas_avoids_baseline_overflow_and_restores_pointer_focus() {
    let devices = Box::leak(Box::new(
        drivers_wasm::devices::graphics::new().await.unwrap(),
    ));
    let document = web_sys::window().unwrap().document().unwrap();
    assert_eq!(
        document.active_element().unwrap(),
        devices.canvas.clone().unchecked_into::<web_sys::Element>()
    );
    // Isolate layout from the test runner's live output above the canvas.
    let wrapper = document.create_element("div").unwrap();
    document.body().unwrap().append_child(&wrapper).unwrap();
    wrapper.append_child(&devices.canvas).unwrap();
    let following_block = document.create_element("div").unwrap();
    wrapper.append_child(&following_block).unwrap();
    assert_eq!(
        following_block
            .dyn_ref::<web_sys::HtmlElement>()
            .unwrap()
            .offset_top(),
        devices.canvas.offset_top() + devices.canvas.height() as i32
    );
    let input = document.create_element("input").unwrap();
    document.body().unwrap().append_child(&input).unwrap();
    input
        .dyn_ref::<web_sys::HtmlElement>()
        .unwrap()
        .focus()
        .unwrap();
    devices
        .canvas
        .dispatch_event(&web_sys::Event::new("pointerdown").unwrap())
        .unwrap();
    assert_eq!(
        document.active_element().unwrap(),
        devices.canvas.clone().unchecked_into::<web_sys::Element>()
    );
    input.remove();
    wrapper.remove();
}

#[wasm_bindgen_test]
async fn changed_viewport_updates_resolution_and_requests_redraw() {
    let canvas = canvas();
    canvas.set_width(1);
    canvas.set_height(1);
    let device = CanvasScreenDevice::new(canvas.clone()).await.unwrap();
    let window = web_sys::window().unwrap();
    window
        .dispatch_event(&web_sys::Event::new("resize").unwrap())
        .unwrap();
    assert_eq!(
        canvas.width(),
        window.inner_width().unwrap().as_f64().unwrap() as u32
    );
    assert_eq!(
        canvas.height(),
        window.inner_height().unwrap().as_f64().unwrap() as u32
    );
    let mut resized = false;
    device
        .control_sync(
            WAS_RESIZED::IDENTIFIER,
            AnyByLayout::NONE,
            AnyByLayout::from_mutable(&mut resized),
        )
        .unwrap();
    assert!(resized);
    device
        .control_sync(
            WAS_RESIZED::IDENTIFIER,
            AnyByLayout::NONE,
            AnyByLayout::from_mutable(&mut resized),
        )
        .unwrap();
    assert!(!resized);
    canvas.remove();
}

#[wasm_bindgen_test]
async fn dirty_rectangle_with_unaligned_width_preserves_canvas_color_order() {
    const WIDTH: u32 = 5;
    const HEIGHT: u32 = 3;

    let canvas = canvas();
    canvas.set_width(WIDTH);
    canvas.set_height(HEIGHT);
    let device = CanvasScreenDevice::new(canvas.clone()).await.unwrap();
    let mut area = Area::new(
        Point::new(0, 0),
        Point::new(WIDTH as i16 - 1, HEIGHT as i16 - 1),
    );
    device
        .control(
            SET_DRAWING_AREA::IDENTIFIER,
            AnyByLayout::from_mutable(&mut area),
            AnyByLayout::NONE,
        )
        .unwrap();

    let colors: Vec<_> = (0..WIDTH * HEIGHT)
        .map(|index| {
            ColorARGB8888::new(
                255,
                (index * 37 + 11) as u8,
                (index * 53 + 19) as u8,
                (index * 71 + 23) as u8,
            )
        })
        .collect();
    let source = unsafe {
        core::slice::from_raw_parts(
            colors.as_ptr().cast::<u8>(),
            colors.len() * core::mem::size_of::<ColorARGB8888>(),
        )
    };
    assert_eq!(device.write(source, 0).unwrap(), colors.len());

    let context: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into()
        .unwrap();
    let pixels = context
        .get_image_data(0.0, 0.0, WIDTH as f64, HEIGHT as f64)
        .unwrap()
        .data();
    for (index, color) in colors.iter().enumerate() {
        let offset = index * 4;
        assert_eq!(
            &pixels.0[offset..offset + 4],
            &[color.get_red(), color.get_green(), color.get_blue(), 255],
            "pixel {index} in {WIDTH}x{HEIGHT} dirty rectangle",
        );
    }
    canvas.remove();
}
