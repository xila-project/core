#![no_std]

extern crate alloc;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicBool, Ordering};

use debug_protocol::{
    InputState, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Request, ScreenshotMetadata,
    ScreenshotPixelFormat,
};
use xila::{
    executable::{self, ExecutableTrait, Standard},
    file_system::AccessFlags,
    graphics::{self, EventKind, Key, State},
    task::{self, TaskIdentifier},
    virtual_file_system::{File, VirtualFileSystem},
};

pub struct DebugExecutable;

impl ExecutableTrait for DebugExecutable {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> executable::MainFuture {
        Box::pin(async move { run(standard, arguments).await })
    }
}

async fn run(standard: Standard<'static>, arguments: Vec<String>) -> Result<(), NonZeroUsize> {
    let context = standard.context;
    let graphics = graphics::ffi_manager();
    let background = arguments.iter().any(|argument| argument == "--background");
    let mut window = if background {
        None
    } else {
        Some({
            let _lock = graphics.lock().await;
            let mut window = graphics.create_window().await.map_err(|_| code(1))?;
            window.set_icon("Db", graphics::Color::BLACK);
            let label = unsafe { graphics::lvgl::lv_label_create(window.as_object_mutable()) };
            if label.is_null() {
                return Err(code(1));
            }
            unsafe {
                graphics::lvgl::lv_label_set_text(
                    label,
                    c"Debug agent is ready. Use xila-debug from the host.".as_ptr(),
                );
                graphics::lvgl::lv_obj_center(label);
            }
            window
        })
    };

    let vfs = context.virtual_file_system;
    let task_manager = context.task_manager;
    let task = task_manager.get_current_task_identifier().await;
    let stopping = Arc::new(AtomicBool::new(false));
    let task_stopping = stopping.clone();
    let (service_handle, _) = task_manager
        .spawn(
            task,
            "Debug agent transport",
            None,
            async move |service_task| {
                if let Err(error) = serve_agent(vfs, context, service_task, task_stopping).await {
                    log::error!("Debug agent transport stopped: {error}");
                }
            },
        )
        .await
        .map_err(|_| code(1))?;

    let mut should_close = false;
    loop {
        if let Some(window) = window.as_mut() {
            graphics::lock!(graphics, {
                while let Some(event) = window.pop_event() {
                    if matches!(event.code, EventKind::Delete | EventKind::CloseRequested)
                        && event.target == window.as_object_mutable()
                    {
                        should_close = true;
                        break;
                    }
                }
            });
        }
        if should_close {
            stopping.store(true, Ordering::Release);
            service_handle.join().await;
            return Ok(());
        }
        task::sleep(core::time::Duration::from_millis(50)).await;
    }
}

async fn serve_agent(
    vfs: &'static VirtualFileSystem,
    context: &'static xila::executable::ExecutableContext,
    task: TaskIdentifier,
    stopping: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut transport = File::open(
        vfs,
        task,
        "/devices/debug/transport",
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut request_buffer = alloc::vec![0; MAX_REQUEST_BYTES + 1];
    loop {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let length = match transport
            .as_synchronous_file_mut()
            .read(&mut request_buffer)
        {
            Ok(length) => length,
            Err(xila::virtual_file_system::Error::RessourceBusy)
            | Err(xila::virtual_file_system::Error::FileSystem(
                xila::file_system::Error::RessourceBusy,
            )) => {
                task::sleep(core::time::Duration::from_millis(10)).await;
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        let mut response = match parse_request(&request_buffer[..length]) {
            Ok(request) => dispatch(request, context).await,
            Err((code, message)) => DispatchResponse::json(error_response(code, &message)),
        };
        let mut response_bytes =
            serde_json::to_vec(&response.value).map_err(|error| error.to_string())?;
        if response_bytes.len() + 1 > MAX_RESPONSE_BYTES {
            response_bytes = serde_json::to_vec(&error_response(
                "response_too_large",
                "Response exceeds the 64 KiB limit; request a smaller UI tree.",
            ))
            .map_err(|error| error.to_string())?;
            response.payload = None;
        }
        response_bytes.push(b'\n');
        write_all(&mut transport, &response_bytes).await?;
        if let Some(payload) = response.payload {
            write_all(&mut transport, &payload).await?;
        }
    }

    transport
        .close(vfs)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn write_all(file: &mut File<'_>, bytes: &[u8]) -> Result<(), String> {
    let mut offset = 0;
    while offset < bytes.len() {
        let written = file
            .write(&bytes[offset..])
            .await
            .map_err(|error| error.to_string())?;
        if written == 0 {
            return Err("Transport returned a zero-length write.".into());
        }
        offset += written;
    }
    Ok(())
}

fn parse_request(bytes: &[u8]) -> Result<Request, (&'static str, String)> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err((
            "request_too_large",
            "Request exceeds the 16 KiB limit.".into(),
        ));
    }
    let request: Request = serde_json::from_slice(bytes)
        .map_err(|_| ("invalid_json", "Request is not valid JSON.".into()))?;
    debug_protocol::validate_request(&request)
        .map_err(|error| (error.code(), "Request validation failed.".into()))?;
    Ok(request)
}

struct DispatchResponse {
    value: serde_json::Value,
    payload: Option<Vec<u8>>,
}

impl DispatchResponse {
    fn json(value: serde_json::Value) -> Self {
        Self {
            value,
            payload: None,
        }
    }
}

async fn dispatch(
    request: Request,
    context: &'static xila::executable::ExecutableContext,
) -> DispatchResponse {
    let graphics_manager = graphics::ffi_manager();
    let mut payload = None;
    let result: Result<serde_json::Value, (&'static str, String)> = match request {
        Request::Diagnostics => diagnostics(context).await,
        Request::Resolution => graphics_manager
            .get_resolution()
            .await
            .map(|point| serde_json::json!({"width": point.get_x(), "height": point.get_y()}))
            .map_err(|error| ("display_unavailable", error.to_string())),
        Request::Screenshot { path: _ } => match graphics_manager.capture_screen().await {
            Ok(snapshot) => {
                let metadata = ScreenshotMetadata {
                    width: snapshot.width,
                    height: snapshot.height,
                    format: ScreenshotPixelFormat::Rgba8888,
                    payload_bytes: snapshot.pixels_rgba.len(),
                };
                if debug_protocol::validate_screenshot_metadata(metadata).is_err() {
                    Err((
                        "screenshot_failed",
                        "Snapshot has invalid dimensions.".into(),
                    ))
                } else {
                    payload = Some(snapshot.pixels_rgba);
                    Ok(serde_json::json!({
                        "width": metadata.width,
                        "height": metadata.height,
                        "format": metadata.format,
                        "payload_bytes": metadata.payload_bytes,
                    }))
                }
            }
            Err(error) => Err(("screenshot_failed", error.to_string())),
        },
        Request::UiTree {
            max_elements,
            max_depth,
        } => match graphics::ffi_manager()
            .inspect_ui_tree(max_elements, max_depth)
            .await
        {
            Ok(snapshot) => {
                let elements: Vec<serde_json::Value> = snapshot
                    .elements
                    .into_iter()
                    .map(|element| {
                        serde_json::json!({
                            "id": element.id,
                            "parent_id": element.parent_id,
                            "role": element.role,
                            "text": element.text,
                            "bounds": element.bounds,
                        })
                    })
                    .collect();
                Ok(serde_json::json!({
                    "elements": elements,
                    "truncated": snapshot.truncated
                }))
            }
            Err(error) => Err(("ui_tree_failed", error.to_string())),
        },
        Request::PointerMove { x, y } => {
            let point = match (i16::try_from(x), i16::try_from(y)) {
                (Ok(x), Ok(y)) => graphics::Point::new(x, y),
                _ => {
                    return DispatchResponse::json(error_response(
                        "invalid_coordinates",
                        "Coordinates are outside the display.",
                    ));
                }
            };
            match graphics_manager.inject_pointer_move(point).await {
                Ok(()) => Ok(serde_json::json!({"x": x, "y": y})),
                Err(graphics::Error::InvalidParameter) => Err((
                    "invalid_coordinates",
                    "Coordinates are outside the display.".into(),
                )),
                Err(error) => Err(input_error(error)),
            }
        }
        Request::PointerButton { state } => graphics_manager
            .inject_pointer_button(state_to_graphics(state))
            .await
            .map(|()| serde_json::json!({"state": state}))
            .map_err(input_error),
        Request::Key { key: name, state } => match parse_key(&name) {
            Some(key) => graphics_manager
                .inject_key(key, state_to_graphics(state))
                .await
                .map(|()| serde_json::json!({"key": name, "state": state}))
                .map_err(input_error),
            None => Err(("unsupported_key", "Key is not supported.".into())),
        },
        Request::Text { text } => match graphics_manager.inject_text(&text).await {
            Ok(count) => Ok(serde_json::json!({"queued_events": count * 2})),
            Err(graphics::Error::InvalidParameter) => Err((
                "unsupported_character",
                "Only printable ASCII is supported.".into(),
            )),
            Err(error) => Err(input_error(error)),
        },
    };

    let value = match result {
        Ok(value) => serde_json::json!({"ok": true, "result": value}),
        Err((code, message)) => error_response(code, &message),
    };
    DispatchResponse { value, payload }
}

fn input_error(error: graphics::Error) -> (&'static str, String) {
    match error {
        graphics::Error::AlreadyInUse => (
            "busy",
            "The input queue is full; retry after the UI processes pending input.".into(),
        ),
        error => ("input_failed", error.to_string()),
    }
}

fn parse_key(value: &str) -> Option<Key> {
    Some(match value {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "escape" => Key::Escape,
        "delete" => Key::Delete,
        "backspace" => Key::Backspace,
        "enter" => Key::Enter,
        "next" => Key::Next,
        "previous" => Key::Previous,
        "home" => Key::Home,
        "end" => Key::End,
        value if value.len() == 1 && value.as_bytes()[0].is_ascii_graphic() => {
            Key::Character(value.as_bytes()[0])
        }
        _ => return None,
    })
}

fn state_to_graphics(state: InputState) -> State {
    match state {
        InputState::Pressed => State::Pressed,
        InputState::Released => State::Released,
    }
}

async fn diagnostics(
    context: &'static xila::executable::ExecutableContext,
) -> Result<serde_json::Value, (&'static str, String)> {
    let memory = xila::memory::global_manager();
    let mut stack = context
        .task_manager
        .get_children(task::Manager::ROOT_TASK_IDENTIFIER)
        .await
        .map_err(|error| ("diagnostics_failed", error.to_string()))?;
    let mut tasks = Vec::new();
    while let Some(task) = stack.pop() {
        if tasks.len() >= 64 {
            break;
        }
        let name = context
            .task_manager
            .get_name(task)
            .await
            .unwrap_or_else(|_| "unknown".into());
        let children = context
            .task_manager
            .get_children(task)
            .await
            .unwrap_or_default();
        stack.extend(children);
        let parent = context
            .task_manager
            .get_parent(task)
            .await
            .map(|parent| usize::from(parent))
            .ok();
        tasks.push(serde_json::json!({
            "id": usize::from(task),
            "parent_id": parent,
            "name": name
        }));
    }
    Ok(serde_json::json!({
        "tasks": tasks,
        "truncated": !stack.is_empty(),
        "memory": {
            "total_bytes": memory.get_total_size(),
            "used_bytes": memory.get_used(),
            "free_bytes": memory.get_free(),
            "page_size": memory.get_page_size(),
        }
    }))
}

fn error_response(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({"ok": false, "error": {"code": code, "message": message}})
}

fn code(value: u8) -> NonZeroUsize {
    NonZeroUsize::new(value as usize).unwrap()
}
