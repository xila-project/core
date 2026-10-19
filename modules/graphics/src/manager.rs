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

use crate::Input;
use crate::InputKind;
use crate::window::Window;
use crate::{Color, theme};
use crate::{Display, OwnedWindow};
use crate::{Error, Result};

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
    TICK_TIME_MANAGER.store(time_manager as *const _ as *mut _, Ordering::Release);
    Manager::new(
        time_manager,
        screen_device,
        input_device,
        input_device_type,
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
}

/// A bounded, pointer-free description of one LVGL object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiElement {
    /// Position in the depth-first snapshot, valid only for that snapshot.
    pub id: u32,
    pub parent_id: Option<u32>,
    pub role: String,
    pub text: Option<String>,
    /// Inclusive screen coordinates `(x1, y1, x2, y2)`.
    pub bounds: (i32, i32, i32, i32),
}

/// Bounds for UI tree snapshots. These limits keep diagnostics safe on small targets.
pub const UI_TREE_MAX_ELEMENTS: usize = 256;
pub const UI_TREE_MAX_DEPTH: usize = 24;
const UI_ELEMENT_MAX_TEXT_BYTES: usize = 512;

impl Manager {
    /// Snapshot a bounded depth-first view of the currently mounted UI tree.
    ///
    /// Element IDs are indexes into this snapshot and must not be retained across
    /// snapshots. No LVGL pointer is returned to the caller.
    pub async fn inspect_ui_tree(
        &self,
        max_elements: usize,
        max_depth: usize,
    ) -> Result<Vec<UiElement>> {
        validate_ui_tree_limits(max_elements, max_depth)?;

        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        if inner.window_parent.is_null() {
            return Err(Error::NotAvailable);
        }

        let mut elements = Vec::with_capacity(max_elements);
        unsafe {
            inspect_children(
                inner.window_parent,
                None,
                0,
                max_depth,
                max_elements,
                &mut elements,
            );
        }
        Ok(elements)
    }

    pub fn lock_function_sync<T>(&self, function: impl FnOnce() -> Result<T>) -> Result<T> {
        task::block_on(self.lock_function(function))
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

unsafe fn inspect_children(
    object: *mut lvgl::lv_obj_t,
    parent_id: Option<u32>,
    depth: usize,
    max_depth: usize,
    max_elements: usize,
    elements: &mut Vec<UiElement>,
) {
    if object.is_null() || depth >= max_depth || elements.len() >= max_elements {
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
        if lvgl::lv_obj_check_type(object, &lvgl::lv_label_class) {
            bounded_c_string(lvgl::lv_label_get_text(object))
        } else if lvgl::lv_obj_check_type(object, &lvgl::lv_textarea_class) {
            bounded_c_string(lvgl::lv_textarea_get_text(object))
        } else {
            None
        }
    };

    elements.push(UiElement {
        id,
        parent_id,
        role,
        text,
        bounds: (area.x1, area.y1, area.x2, area.y2),
    });

    if depth + 1 >= max_depth || elements.len() >= max_elements {
        return;
    }

    let child_count = unsafe { lvgl::lv_obj_get_child_count(object) };
    for index in 0..child_count {
        if elements.len() >= max_elements {
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
            );
        }
    }
}

/// Copy a valid C string with a hard byte cap without scanning past its NUL.
unsafe fn bounded_c_string(pointer: *const core::ffi::c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }

    let bytes = pointer.cast::<u8>();
    let mut length = 0;
    while length < UI_ELEMENT_MAX_TEXT_BYTES && unsafe { *bytes.add(length) } != 0 {
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
    unsafe { &*manager }
        .get_current_time()
        .unwrap_or_default()
        .as_millis() as u32
}

unsafe impl Send for Manager {}

unsafe impl Sync for Manager {}

impl Manager {
    fn new(
        _: &time::Manager,
        screen_device: &'static dyn DirectCharacterDevice,
        input_device: &'static dyn DirectCharacterDevice,
        input_device_type: InputKind,
        buffer_size: usize,
        double_buffered: bool,
    ) -> Result<Self> {
        unsafe {
            lvgl::lv_init();

            if !lvgl::lv_is_initialized() {
                panic!("Failed to initialize lvgl");
            }

            lvgl::lv_tick_set_cb(Some(binding_tick_callback_function));
        }

        let (display, input) = Self::create_display(
            screen_device,
            buffer_size,
            input_device,
            input_device_type,
            double_buffered,
        )?;

        let screen = display.get_object();

        unsafe {
            let group = lvgl::lv_group_create();
            lvgl::lv_group_set_default(group);
            theme::initialize(&display)?;
        }

        Ok(Self {
            inner: RwLock::new(Inner {
                _inputs: vec![input],
                displays: vec![display],
                window_parent: screen,
            }),
            global_lock: Mutex::new(()),
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
        buffer_size: usize,
        input_device: &'static dyn DirectCharacterDevice,
        input_device_type: InputKind,
        double_buffered: bool,
    ) -> Result<(Display, Input)> {
        let display = Display::new(screen_device, buffer_size, double_buffered)?;

        let input = Input::new(input_device, input_device_type)?;

        Ok((display, input))
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
