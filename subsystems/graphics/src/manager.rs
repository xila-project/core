use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::str;
use file_system::DirectCharacterDevice;
use synchronization::blocking_mutex::raw::CriticalSectionRawMutex;
use synchronization::mutex::{Mutex, MutexGuard};
use synchronization::rwlock::RwLock;
use task::block_on;

use core::future::Future;
use core::sync::atomic::{AtomicPtr, Ordering};

use core::time::Duration;

use super::lvgl;

use crate::input::InputQueue;
use crate::window::Window;
use crate::{Color, theme};
use crate::{Display, OwnedWindow};
use crate::{Error, Result};
use crate::{Input, InputKind, Key, State};

static TICK_TIME_MANAGER: AtomicPtr<time::Manager<'static>> = AtomicPtr::new(core::ptr::null_mut());
static FFI_GRAPHICS_MANAGER: AtomicPtr<Manager> = AtomicPtr::new(core::ptr::null_mut());

/// Register the manager used by C/wasm callbacks that cannot carry Rust references.
pub fn set_ffi_manager(manager: &'static Manager) {
    FFI_GRAPHICS_MANAGER.store(manager as *const _ as *mut _, Ordering::Release);
}

/// Resolve the graphics capability at an FFI callback boundary.
pub fn ffi_manager() -> &'static Manager {
    let manager = FFI_GRAPHICS_MANAGER.load(Ordering::Acquire);
    assert!(!manager.is_null(), "FFI graphics manager not initialized");
    unsafe { &*manager }
}

/// Return the graphics capability when initialized, without panicking during startup.
pub fn try_ffi_manager() -> Option<&'static Manager> {
    let manager = FFI_GRAPHICS_MANAGER.load(Ordering::Acquire);
    if manager.is_null() {
        None
    } else {
        Some(unsafe { &*manager })
    }
}

pub async fn initialize(
    time_manager: &'static time::Manager<'static>,
    screen_device: &'static dyn DirectCharacterDevice,
    input_device: &'static dyn DirectCharacterDevice,
    input_device_type: InputKind,
    buffer_size: usize,
    double_buffered: bool,
) -> Manager {
    let input_devices = [(input_device, input_device_type)];
    initialize_with_input_devices(
        time_manager,
        screen_device,
        &input_devices,
        buffer_size,
        double_buffered,
    )
    .await
}

/// Initialize graphics with zero or more physical input devices. Graphics-owned
/// virtual pointer and keypad devices are always installed for subsystem input.
pub async fn initialize_with_input_devices(
    time_manager: &'static time::Manager<'static>,
    screen_device: &'static dyn DirectCharacterDevice,
    input_devices: &[(&'static dyn DirectCharacterDevice, InputKind)],
    buffer_size: usize,
    double_buffered: bool,
) -> Manager {
    TICK_TIME_MANAGER.store(time_manager as *const _ as *mut _, Ordering::Release);
    Manager::new(
        time_manager,
        screen_device,
        input_devices,
        buffer_size,
        double_buffered,
    )
    .expect("Failed to create manager instance")
}

struct Inner {
    _inputs: Vec<Input>,
    displays: Vec<Display>,
    window_parent: *mut lvgl::lv_obj_t,
}

pub struct Manager {
    inner: RwLock<CriticalSectionRawMutex, Inner>,
    global_lock: Mutex<CriticalSectionRawMutex, ()>,
    injected_inputs: synchronization::Arc<InputQueue>,
}

/// A bounded, pointer-free description of one LVGL object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiElement {
    /// Position in the depth-first snapshot, valid only for that snapshot.
    pub id: u32,
    pub parent_id: Option<u32>,
    /// LVGL class name, retained for compatibility and custom widgets.
    pub role: String,
    /// Semantic role for supported widgets; absent for unknown classes.
    pub semantic_role: Option<&'static str>,
    /// Direct label or placeholder, capped to 128 input bytes.
    pub name: Option<String>,
    /// Raw text from labels and text areas, as in the original snapshot API.
    pub text: Option<String>,
    /// The object or an ancestor is marked hidden (independent of clipping).
    pub hidden: bool,
    /// The object is marked disabled by LVGL.
    pub disabled: bool,
    pub focused: bool,
    /// LVGL clickable flag, not a guarantee that the object can be hit.
    pub clickable: bool,
    pub checked: Option<bool>,
    /// Inclusive screen coordinates `(x1, y1, x2, y2)`.
    pub bounds: (i32, i32, i32, i32),
}

/// One bounded UI-tree snapshot and whether additional objects were omitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiTreeSnapshot {
    pub elements: Vec<UiElement>,
    pub truncated: bool,
}

/// Bounds for UI tree snapshots. These limits keep diagnostics safe on small targets.
pub const UI_TREE_MAX_ELEMENTS: usize = 256;
pub const UI_TREE_MAX_DEPTH: usize = 24;
const UI_ELEMENT_MAX_TEXT_BYTES: usize = 512;
const UI_ELEMENT_MAX_NAME_BYTES: usize = 128;
const UI_NAME_MAX_CHILDREN: u32 = 16;

impl Manager {
    /// Snapshot a bounded depth-first view of the currently mounted UI tree.
    ///
    /// Element IDs are indexes into this snapshot and must not be retained across
    /// snapshots. No LVGL pointer is returned to the caller.
    pub async fn inspect_ui_tree(
        &self,
        max_elements: usize,
        max_depth: usize,
    ) -> Result<UiTreeSnapshot> {
        validate_ui_tree_limits(max_elements, max_depth)?;

        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        if inner.window_parent.is_null() {
            return Err(Error::NotAvailable);
        }

        let mut elements = Vec::with_capacity(max_elements);
        let mut truncated = false;
        unsafe {
            inspect_children(
                inner.window_parent,
                None,
                0,
                max_depth,
                max_elements,
                &mut elements,
                &mut truncated,
            );
        }
        Ok(UiTreeSnapshot {
            elements,
            truncated,
        })
    }

    /// Capture the active LVGL screen into an owned, bounded RGBA8888 buffer.
    pub async fn capture_screen(&self) -> Result<crate::ScreenSnapshot> {
        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        inner
            .displays
            .first()
            .ok_or(Error::NotAvailable)?
            .capture_screen()
    }

    /// Return the current active LVGL display resolution.
    pub async fn get_resolution(&self) -> Result<crate::Point> {
        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        inner
            .displays
            .first()
            .map(Display::get_resolution)
            .ok_or(Error::NotAvailable)
    }

    /// Queue a pointer move for the graphics-owned LVGL pointer input.
    pub async fn inject_pointer_move(&self, point: crate::Point) -> Result<()> {
        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        let resolution = inner
            .displays
            .first()
            .map(Display::get_resolution)
            .ok_or(Error::NotAvailable)?;
        validate_pointer_point(point, resolution)?;
        self.injected_inputs.pointer_move(point)
    }

    /// Atomically queue a pointer move and complete click at a valid display point.
    pub async fn inject_pointer_click(&self, point: crate::Point) -> Result<()> {
        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        let resolution = inner
            .displays
            .first()
            .map(Display::get_resolution)
            .ok_or(Error::NotAvailable)?;
        validate_pointer_point(point, resolution)?;
        self.injected_inputs.pointer_click(point)
    }

    /// Queue a pointer button state at the most recently injected pointer point.
    pub async fn inject_pointer_button(&self, state: State) -> Result<()> {
        let _lock = self.global_lock.lock().await;
        self.injected_inputs.pointer_button(state)
    }

    /// Queue one keyboard key transition for the graphics-owned LVGL keypad.
    pub async fn inject_key(&self, key: Key, state: State) -> Result<()> {
        let _lock = self.global_lock.lock().await;
        self.injected_inputs.key(key, state)
    }

    /// Queue a complete key press and release, or leave the queue unchanged.
    pub async fn inject_key_press(&self, key: Key) -> Result<()> {
        let _lock = self.global_lock.lock().await;
        self.injected_inputs.key_press(key)
    }

    /// Queue printable ASCII text as ordered key press and release events.
    pub async fn inject_text(&self, text: &str) -> Result<usize> {
        let _lock = self.global_lock.lock().await;
        self.injected_inputs.text(text)
    }

    /// Queue a bounded prefix of printable Unicode text, returning the accepted byte count.
    /// A queue-full error never enqueues a partial press/release pair.
    pub async fn inject_text_chunk(&self, text: &str) -> Result<usize> {
        let _lock = self.global_lock.lock().await;
        self.injected_inputs.text_chunk(text)
    }

    pub fn lock_function_sync<T>(&self, function: impl FnOnce() -> Result<T>) -> Result<T> {
        task::block_on(self.lock_function(function))
    }
}

fn validate_pointer_point(point: crate::Point, resolution: crate::Point) -> Result<()> {
    if point.get_x() < 0
        || point.get_y() < 0
        || point.get_x() >= resolution.get_x()
        || point.get_y() >= resolution.get_y()
    {
        Err(Error::InvalidParameter)
    } else {
        Ok(())
    }
}

fn validate_ui_tree_limits(max_elements: usize, max_depth: usize) -> Result<()> {
    if max_elements == 0
        || max_elements > UI_TREE_MAX_ELEMENTS
        || max_depth == 0
        || max_depth > UI_TREE_MAX_DEPTH
    {
        Err(Error::InvalidParameter)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod ui_tree_tests {
    use super::*;

    #[test]
    fn semantic_roles_cover_common_widgets_and_preserve_unknown_classes() {
        assert_eq!(semantic_role("lv_button"), Some("button"));
        assert_eq!(semantic_role("lv_textarea"), Some("textbox"));
        assert_eq!(semantic_role("lv_checkbox"), Some("checkbox"));
        assert_eq!(semantic_role("lv_switch"), Some("switch"));
        assert_eq!(semantic_role("lv_slider"), Some("slider"));
        assert_eq!(semantic_role("lv_dropdown"), Some("combobox"));
        assert_eq!(semantic_role("lv_label"), Some("text"));
        assert_eq!(semantic_role("my_custom_widget"), None);
    }

    #[test]
    fn tree_limits_are_bounded() {
        assert_eq!(validate_ui_tree_limits(1, 1), Ok(()));
        assert_eq!(
            validate_ui_tree_limits(UI_TREE_MAX_ELEMENTS + 1, 1),
            Err(Error::InvalidParameter)
        );
        assert_eq!(
            validate_ui_tree_limits(1, UI_TREE_MAX_DEPTH + 1),
            Err(Error::InvalidParameter)
        );
        assert_eq!(validate_ui_tree_limits(0, 1), Err(Error::InvalidParameter));
    }
}

#[cfg(test)]
mod injected_input_tests {
    use super::*;

    #[test]
    fn injected_pointer_rejects_right_and_bottom_edges() {
        let resolution = crate::Point::new(640, 480);
        assert_eq!(
            validate_pointer_point(crate::Point::new(639, 479), resolution),
            Ok(())
        );
        assert_eq!(
            validate_pointer_point(crate::Point::new(640, 10), resolution),
            Err(Error::InvalidParameter)
        );
        assert_eq!(
            validate_pointer_point(crate::Point::new(10, 480), resolution),
            Err(Error::InvalidParameter)
        );
    }
}

fn semantic_role(class: &str) -> Option<&'static str> {
    match class {
        "lv_button" | "lv_list_button" => Some("button"),
        "lv_textarea" => Some("textbox"),
        "lv_checkbox" => Some("checkbox"),
        "lv_switch" => Some("switch"),
        "lv_slider" => Some("slider"),
        "lv_dropdown" => Some("combobox"),
        "lv_label" | "lv_list_text" => Some("text"),
        "lv_list" => Some("list"),
        _ => None,
    }
}

/// Flags report explicit LVGL state; they do not assert visibility through
/// clipping, occlusion, or whether a click would hit this object.
unsafe fn is_hidden_by_ancestor(mut object: *mut lvgl::lv_obj_t) -> bool {
    while !object.is_null() {
        if unsafe { lvgl::lv_obj_has_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN) } {
            return true;
        }
        object = unsafe { lvgl::lv_obj_get_parent(object) };
    }
    false
}

/// Use only unambiguous associations: textarea placeholder, checkbox text, or
/// the first direct label in a button. Never search arbitrary siblings.
unsafe fn widget_name(object: *mut lvgl::lv_obj_t, semantic_role: Option<&str>) -> Option<String> {
    unsafe {
        match semantic_role {
            Some("textbox") if lvgl::lv_obj_has_class(object, &lvgl::lv_textarea_class) => {
                bounded_c_string_with_limit(
                    lvgl::lv_textarea_get_placeholder_text(object),
                    UI_ELEMENT_MAX_NAME_BYTES,
                )
            }
            Some("checkbox") if lvgl::lv_obj_has_class(object, &lvgl::lv_checkbox_class) => {
                bounded_c_string_with_limit(
                    lvgl::lv_checkbox_get_text(object),
                    UI_ELEMENT_MAX_NAME_BYTES,
                )
            }
            Some("button") if lvgl::lv_obj_has_class(object, &lvgl::lv_button_class) => {
                for index in 0..lvgl::lv_obj_get_child_count(object).min(UI_NAME_MAX_CHILDREN) {
                    let child = lvgl::lv_obj_get_child(object, index as i32);
                    if lvgl::lv_obj_has_class(child, &lvgl::lv_label_class) {
                        return bounded_c_string_with_limit(
                            lvgl::lv_label_get_text(child),
                            UI_ELEMENT_MAX_NAME_BYTES,
                        );
                    }
                }
                None
            }
            _ => None,
        }
    }
}

unsafe fn inspect_children(
    object: *mut lvgl::lv_obj_t,
    parent_id: Option<u32>,
    depth: usize,
    max_depth: usize,
    max_elements: usize,
    elements: &mut Vec<UiElement>,
    truncated: &mut bool,
) {
    if object.is_null() {
        return;
    }
    if depth >= max_depth || elements.len() >= max_elements {
        *truncated = true;
        return;
    }

    let id = elements.len() as u32;
    let class = unsafe { lvgl::lv_obj_get_class(object) };
    let role = if class.is_null() {
        String::from("unknown")
    } else {
        unsafe { bounded_c_string((*class).name) }.unwrap_or_else(|| String::from("unknown"))
    };

    let mut area = lvgl::lv_area_t::default();
    unsafe { lvgl::lv_obj_get_coords(object, &mut area) };

    let text = unsafe {
        if lvgl::lv_obj_has_class(object, &lvgl::lv_label_class) {
            bounded_c_string(lvgl::lv_label_get_text(object))
        } else if lvgl::lv_obj_has_class(object, &lvgl::lv_textarea_class) {
            bounded_c_string(lvgl::lv_textarea_get_text(object))
        } else {
            None
        }
    };

    let semantic_role = semantic_role(&role);
    let (name, hidden, disabled, focused, clickable, checked) = unsafe {
        (
            widget_name(object, semantic_role),
            is_hidden_by_ancestor(object),
            lvgl::lv_obj_has_state(object, lvgl::lv_state_t_LV_STATE_DISABLED),
            lvgl::lv_obj_has_state(object, lvgl::lv_state_t_LV_STATE_FOCUSED),
            lvgl::lv_obj_has_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE),
            if matches!(semantic_role, Some("checkbox" | "switch")) {
                Some(lvgl::lv_obj_has_state(
                    object,
                    lvgl::lv_state_t_LV_STATE_CHECKED,
                ))
            } else {
                None
            },
        )
    };

    elements.push(UiElement {
        id,
        parent_id,
        role,
        semantic_role,
        name,
        text,
        hidden,
        disabled,
        focused,
        clickable,
        checked,
        bounds: (area.x1, area.y1, area.x2, area.y2),
    });

    let child_count = unsafe { lvgl::lv_obj_get_child_count(object) };
    if child_count == 0 {
        return;
    }
    if depth + 1 >= max_depth || elements.len() >= max_elements {
        *truncated = true;
        return;
    }
    for index in 0..child_count {
        if elements.len() >= max_elements {
            *truncated = true;
            break;
        }
        let child = unsafe { lvgl::lv_obj_get_child(object, index as i32) };
        unsafe {
            inspect_children(
                child,
                Some(id),
                depth + 1,
                max_depth,
                max_elements,
                elements,
                truncated,
            );
        }
    }
}

/// Copy a valid C string with a hard byte cap without scanning past its NUL.
unsafe fn bounded_c_string(pointer: *const core::ffi::c_char) -> Option<String> {
    unsafe { bounded_c_string_with_limit(pointer, UI_ELEMENT_MAX_TEXT_BYTES) }
}

unsafe fn bounded_c_string_with_limit(
    pointer: *const core::ffi::c_char,
    limit: usize,
) -> Option<String> {
    if pointer.is_null() {
        return None;
    }

    let bytes = pointer.cast::<u8>();
    let mut length = 0;
    while length < limit && unsafe { *bytes.add(length) } != 0 {
        length += 1;
    }

    let slice = unsafe { core::slice::from_raw_parts(bytes, length) };
    Some(match str::from_utf8(slice) {
        Ok(text) => text.into(),
        Err(_) => String::from_utf8_lossy(slice).into_owned(),
    })
}

impl Drop for Manager {
    fn drop(&mut self) {
        unsafe {
            lvgl::lv_deinit();
        }
    }
}

extern "C" fn binding_tick_callback_function() -> u32 {
    let manager = TICK_TIME_MANAGER.load(Ordering::Acquire);
    assert!(!manager.is_null(), "Graphics time manager not initialized");
    unsafe { &*manager }.monotonic_milliseconds_wrapping()
}

unsafe impl Send for Manager {}

unsafe impl Sync for Manager {}

impl Manager {
    fn new(
        _: &time::Manager,
        screen_device: &'static dyn DirectCharacterDevice,
        input_devices: &[(&'static dyn DirectCharacterDevice, InputKind)],
        buffer_size: usize,
        double_buffered: bool,
    ) -> Result<Self> {
        let injected_inputs = InputQueue::new()?;
        unsafe {
            lvgl::lv_init();

            if !lvgl::lv_is_initialized() {
                panic!("Failed to initialize lvgl");
            }

            lvgl::lv_tick_set_cb(Some(binding_tick_callback_function));
            // LVGL assigns newly created keypads to the default group. Create
            // it before display creation also creates the virtual keypad.
            let group = lvgl::lv_group_create();
            lvgl::lv_group_set_default(group);
        }

        let (display, inputs) = Self::create_display(
            screen_device,
            input_devices,
            buffer_size,
            double_buffered,
            injected_inputs.clone(),
        )?;

        let screen = display.get_object();

        theme::initialize(&display)?;

        Ok(Self {
            inner: RwLock::new(Inner {
                _inputs: inputs,
                displays: vec![display],
                window_parent: screen,
            }),
            global_lock: Mutex::new(()),
            injected_inputs,
        })
    }

    pub async fn r#loop<F>(&self, sleep: impl Fn(Duration) -> F + Send + 'static) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut call_count: u8 = 0;

        loop {
            let time_until_next = unsafe {
                let _lock = self.global_lock.lock().await;
                let inner = self.inner.read().await;

                let time_until_next = lvgl::lv_timer_handler();

                if call_count >= 30 {
                    for display in &inner.displays {
                        display.check_for_resizing();
                    }
                    call_count = 0;
                }
                call_count = call_count.saturating_add(1);

                time_until_next
            };

            sleep(Duration::from_millis(time_until_next as u64)).await;
        }
    }

    pub async fn set_window_parent(&self, window_parent: *mut lvgl::lv_obj_t) -> Result<()> {
        self.inner.write().await.window_parent = window_parent;

        Ok(())
    }

    pub async fn create_window(&self) -> Result<OwnedWindow> {
        let parent_object = self.inner.write().await.window_parent;

        let window = unsafe { Window::new(parent_object)? };

        Ok(OwnedWindow::new(window))
    }

    pub async fn add_input_device(
        &self,

        input_device: &'static dyn DirectCharacterDevice,
        input_type: InputKind,
    ) -> Result<()> {
        let input = Input::new(input_device, input_type)?;

        self.inner.write().await._inputs.push(input);

        Ok(())
    }

    fn create_display(
        screen_device: &'static dyn DirectCharacterDevice,
        input_devices: &[(&'static dyn DirectCharacterDevice, InputKind)],
        buffer_size: usize,
        double_buffered: bool,
        injected_inputs: synchronization::Arc<InputQueue>,
    ) -> Result<(Display, Vec<Input>)> {
        let display = Display::new(screen_device, buffer_size, double_buffered)?;
        let mut inputs = vec![
            Input::new_virtual(injected_inputs.clone(), InputKind::Pointer)?,
            Input::new_virtual(injected_inputs, InputKind::Keypad)?,
        ];
        for (device, kind) in input_devices {
            inputs.push(Input::new(*device, *kind)?);
        }
        Ok((display, inputs))
    }

    pub async fn get_window_count(&self) -> Result<usize> {
        let window_parent = self.inner.read().await.window_parent;
        unsafe { Ok(lvgl::lv_obj_get_child_count(window_parent) as usize) }
    }

    pub async fn get_window_icon(&self, index: usize) -> Result<(String, Color)> {
        let window_parent = self.inner.read().await.window_parent;

        let window = unsafe {
            let child = lvgl::lv_obj_get_child(window_parent, index as i32);

            Window::from_raw(child).ok_or(Error::InvalidWindowIdentifier)?
        };

        let icon = unsafe { window.as_ref().get_icon() };

        let icon = (icon.0.to_string(), icon.1);

        Ok(icon)
    }

    pub async fn get_window_identifier(&self, index: usize) -> Result<usize> {
        let window_parent = self.inner.read().await.window_parent;

        let window = unsafe { lvgl::lv_obj_get_child(window_parent, index as i32) as usize };

        Ok(window)
    }

    pub async fn maximize_window(&self, identifier: usize) -> Result<()> {
        let window_count = self.get_window_count().await?;

        let window_parent = self.inner.read().await.window_parent;

        let found = (0..window_count).find(|index| unsafe {
            let child = lvgl::lv_obj_get_child(window_parent, *index as i32);

            child == identifier as *mut lvgl::lv_obj_t
        });

        if found.is_some() {
            unsafe {
                lvgl::lv_obj_move_foreground(identifier as *mut lvgl::lv_obj_t);
            }

            Ok(())
        } else {
            Err(Error::InvalidWindowIdentifier)
        }
    }

    pub async fn send_window_close_request(&self, identifier: usize) -> Result<()> {
        let window_count = self.get_window_count().await?;

        let window_parent = self.inner.read().await.window_parent;

        let found = (0..window_count).any(|index| unsafe {
            let child = lvgl::lv_obj_get_child(window_parent, index as i32);

            child == identifier as *mut lvgl::lv_obj_t
        });

        if !found {
            return Err(Error::InvalidWindowIdentifier);
        }

        unsafe {
            lvgl::lv_obj_send_event(
                identifier as *mut lvgl::lv_obj_t,
                crate::EventKind::CloseRequested.into_lvgl_code(),
                core::ptr::null_mut(),
            );
        }

        Ok(())
    }

    pub async fn lock_function<T>(&self, function: impl FnOnce() -> Result<T>) -> Result<T> {
        let _lock = self.global_lock.lock().await;

        function()
    }

    pub async fn lock(&self) -> MutexGuard<'_, CriticalSectionRawMutex, ()> {
        self.global_lock.lock().await
    }

    pub fn synchronous_lock(&self) -> MutexGuard<'_, CriticalSectionRawMutex, ()> {
        block_on(self.lock())
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, CriticalSectionRawMutex, ()>> {
        self.global_lock.try_lock().ok()
    }

    pub fn get_current_screen(&self) -> Result<*mut lvgl::lv_obj_t> {
        Ok(unsafe { lvgl::lv_screen_active() })
    }

    pub async fn update_theme(
        &self,
        primary_color: Color,
        secondary_color: Color,
        is_dark: bool,
    ) -> Result<()> {
        let _lock = self.global_lock.lock().await;
        self.update_theme_locked(primary_color, secondary_color, is_dark)
            .await
    }

    /// Apply a palette when the caller already holds the global LVGL lock.
    pub(crate) async fn update_theme_locked(
        &self,
        primary_color: Color,
        secondary_color: Color,
        is_dark: bool,
    ) -> Result<()> {
        let displays = &self.inner.read().await.displays;

        for display in displays {
            theme::update(display, primary_color, secondary_color, is_dark);
        }

        Ok(())
    }
}
