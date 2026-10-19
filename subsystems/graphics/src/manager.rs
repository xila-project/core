use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};
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

impl Manager {
    pub fn lock_function_sync<T>(&self, function: impl FnOnce() -> Result<T>) -> Result<T> {
        task::block_on(self.lock_function(function))
    }
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
