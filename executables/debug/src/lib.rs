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

use debug_protocol::{InputState, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Request};
use xila::{
    executable::{self, ExecutableTrait, Standard},
    file_system::AccessFlags,
    graphics::{
        self, EventKind, GET_RESOLUTION, Key, Point, ScreenshotRequest, State,
        WRITE_SCREENSHOT_TO_PATH,
    },
    task::{self, TaskIdentifier},
    virtual_file_system::{File, VirtualFileSystem},
};

pub struct DebugExecutable;

impl ExecutableTrait for DebugExecutable {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> executable::MainFuture {
        Box::pin(async move { run(standard, arguments).await })
    }
}

async fn run(standard: Standard<'static>, _: Vec<String>) -> Result<(), NonZeroUsize> {
    let context = standard.context;
    let graphics = graphics::ffi_manager();
    let mut window = {
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
    let mut screen = File::open(
        vfs,
        task,
        "/devices/debug/screen",
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut pointer = File::open(
        vfs,
        task,
        "/devices/debug/pointer",
        AccessFlags::Write.into(),
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut keyboard = File::open(
        vfs,
        task,
        "/devices/debug/keyboard",
        AccessFlags::Write.into(),
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
        let response = match parse_request(&request_buffer[..length]) {
            Ok(request) => {
                dispatch(request, context, &mut screen, &mut pointer, &mut keyboard).await
            }
            Err((code, message)) => error_response(code, &message),
        };
        let mut response = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
        if response.len() + 1 > MAX_RESPONSE_BYTES {
            response = serde_json::to_vec(&error_response(
                "response_too_large",
                "Response exceeds the 64 KiB limit; request a smaller UI tree.",
            ))
            .map_err(|error| error.to_string())?;
        }
        response.push(b'\n');
        transport
            .write(&response)
            .await
            .map_err(|error| error.to_string())?;
    }

    keyboard
        .close(vfs)
        .await
        .map_err(|error| error.to_string())?;
    pointer
        .close(vfs)
        .await
        .map_err(|error| error.to_string())?;
    screen.close(vfs).await.map_err(|error| error.to_string())?;
    transport
        .close(vfs)
        .await
        .map_err(|error| error.to_string())?;
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

async fn dispatch(
    request: Request,
    context: &'static xila::executable::ExecutableContext,
    screen: &mut File<'_>,
    pointer: &mut File<'_>,
    keyboard: &mut File<'_>,
) -> serde_json::Value {
    let result: Result<serde_json::Value, (&'static str, String)> = match request {
        Request::Diagnostics => diagnostics(context).await,
        Request::Resolution => resolution(screen)
            .await
            .map(|point| serde_json::json!({"width": point.get_x(), "height": point.get_y()})),
        Request::Screenshot { path } => {
            let Some(request) = ScreenshotRequest::new(&path) else {
                return error_response("invalid_screenshot_path", "Path exceeds the native limit.");
            };
            match screen.control(WRITE_SCREENSHOT_TO_PATH, &request).await {
                Ok(output) => {
                    let dimensions = output;
                    Ok(serde_json::json!({
                        "width": dimensions.width,
                        "height": dimensions.height,
                        "path": path,
                    }))
                }
                Err(error) => Err(("screenshot_failed", error.to_string())),
            }
        }
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
            let point = match resolution(screen).await {
                Ok(point) => point,
                Err(error) => return error_response(error.0, &error.1),
            };
            if u32::try_from(point.get_x()).map_or(true, |width| x >= width)
                || u32::try_from(point.get_y()).map_or(true, |height| y >= height)
            {
                Err((
                    "invalid_coordinates",
                    "Coordinates are outside the display.".into(),
                ))
            } else {
                let mut bytes = [0; 6];
                bytes[0] = 0;
                bytes[1..3].copy_from_slice(&(x as i16).to_le_bytes());
                bytes[3..5].copy_from_slice(&(y as i16).to_le_bytes());
                bytes[5] = State::Released.into();
                pointer
                    .write(&bytes)
                    .await
                    .map(|_| serde_json::json!({"x": x, "y": y}))
                    .map_err(|error| ("input_failed", error.to_string()))
            }
        }
        Request::PointerButton { state } => {
            let mut bytes = [0; 6];
            bytes[0] = 1;
            bytes[5] = state_to_graphics(state).into();
            pointer
                .write(&bytes)
                .await
                .map(|_| serde_json::json!({"state": state}))
                .map_err(|error| ("input_failed", error.to_string()))
        }
        Request::Key { key: name, state } => match parse_key(&name) {
            Some(key) => send_key(key, state, keyboard)
                .await
                .map(|()| serde_json::json!({"key": name, "state": state}))
                .map_err(|error| ("input_failed", error)),
            None => Err(("unsupported_key", "Key is not supported.".into())),
        },
        Request::Text { text } => {
            if !text.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
                Err((
                    "unsupported_character",
                    "Only printable ASCII is supported.".into(),
                ))
            } else {
                let mut count = 0;
                let mut error = None;
                for character in text.bytes() {
                    for state in [InputState::Pressed, InputState::Released] {
                        if let Err(value) =
                            send_key(Key::Character(character), state, keyboard).await
                        {
                            error = Some(value);
                            break;
                        }
                        count += 1;
                    }
                    if error.is_some() {
                        break;
                    }
                }
                match error {
                    Some(error) => Err((
                        "input_failed",
                        alloc::format!("{error}; queued_events={count}"),
                    )),
                    None => Ok(serde_json::json!({"queued_events": count})),
                }
            }
        }
    };

    match result {
        Ok(value) => serde_json::json!({"ok": true, "result": value}),
        Err((code, message)) => error_response(code, &message),
    }
}

async fn resolution(screen: &mut File<'_>) -> Result<Point, (&'static str, String)> {
    screen
        .control(GET_RESOLUTION, &())
        .await
        .map_err(|error| ("display_unavailable", error.to_string()))
}

async fn send_key(key: Key, state: InputState, keyboard: &mut File<'_>) -> Result<(), String> {
    let mut bytes = [0; 5];
    bytes[..4].copy_from_slice(&u32::from(key).to_le_bytes());
    bytes[4] = state_to_graphics(state).into();
    keyboard
        .write(&bytes)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
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
