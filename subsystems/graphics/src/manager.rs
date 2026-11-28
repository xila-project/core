use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use file_system::DirectCharacterDevice;
use lvgl_rust::Lvgl;
use synchronization::blocking_mutex::raw::CriticalSectionRawMutex;
use synchronization::mutex::{Mutex, MutexGuard};
use synchronization::rwlock::RwLock;
use task::block_on;

use core::{cell::Cell, future::Future, marker::PhantomData};
use portable_atomic::{AtomicPtr, Ordering};

use core::time::Duration;

use super::lvgl;

use crate::input::InputQueue;
use crate::on_screen_keyboard::OnScreenKeyboard;
use crate::window::Window;
use crate::{Color, theme};
use crate::{Display, OwnedWindow};
use crate::{Error, Result};
use crate::{Input, InputKind, Key, State};

static GRAPHICS_LOCK: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());
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
    // Drop displays first because LVGL callbacks borrow the keyboard state.
    on_screen_keyboard: Option<Box<OnScreenKeyboard>>,
}

pub struct Manager {
    inner: RwLock<CriticalSectionRawMutex, Inner>,
    window_parent: AtomicPtr<lvgl::lv_obj_t>,
    injected_inputs: synchronization::Arc<InputQueue>,
}

/// Bounds for UI tree snapshots. These limits keep diagnostics safe on small targets.
pub const UI_TREE_MAX_ELEMENTS: usize = 256;
pub const UI_TREE_MAX_DEPTH: usize = 24;
use crate::UiTreeSnapshot;
use crate::inspection::inspect_children;
#[cfg(test)]
use crate::inspection::semantic_role;

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

        let mut guard = self.lock().await;
        guard.with_lvgl(|_| ())?;
        let inner = self.inner.read().await;
        if inner.displays.is_empty() {
            return Err(Error::NotAvailable);
        }

        let mut elements = Vec::with_capacity(max_elements);
        let mut truncated = false;
        // SAFETY: the lock covers traversal and the manager owns the display.
        unsafe {
            let screen = inner.displays[0].get_object();
            let display = lvgl::lv_obj_get_display(screen);
            for (root, layer) in [
                (screen, "screen"),
                (lvgl::lv_display_get_layer_top(display), "top"),
                (lvgl::lv_display_get_layer_sys(display), "system"),
            ] {
                lvgl::lv_obj_update_layout(root);
                inspect_children(
                    root,
                    None,
                    0,
                    (max_depth, max_elements),
                    &mut elements,
                    &mut truncated,
                    layer,
                );
            }
        }
        Ok(UiTreeSnapshot {
            elements,
            truncated,
        })
    }

    /// Resolve and hit-test a unique control while holding the graphics lock.
    /// Coordinates and raw object pointers never survive the operation.
    pub async fn click_ui(
        &self,
        role: Option<&str>,
        name: Option<&str>,
        text: Option<&str>,
        button: Option<&str>,
    ) -> core::result::Result<(i32, i32), &'static str> {
        if [role, name, text, button].iter().all(Option::is_none) {
            return Err("invalid_selector");
        }
        let mut guard = self.lock().await;
        guard.with_lvgl(|_| ()).map_err(|_| "not_available")?;
        let inner = self.inner.read().await;
        let display = inner.displays.first().ok_or("not_available")?;
        // SAFETY: the graphics lock is held and the manager owns the display.
        let screen = unsafe { display.get_object() };
        let display = display.get_lvgl_display();
        // SAFETY: the display stays live and the graphics lock protects its layers.
        let roots = unsafe {
            [
                (screen, "screen"),
                (lvgl::lv_display_get_layer_top(display), "top"),
                (lvgl::lv_display_get_layer_sys(display), "system"),
            ]
        };
        let mut elements = Vec::with_capacity(UI_TREE_MAX_ELEMENTS);
        let mut truncated = false;
        for (root, layer) in roots {
            // SAFETY: the graphics lock covers layout and traversal of the live root.
            unsafe {
                lvgl::lv_obj_update_layout(root);
                inspect_children(
                    root,
                    None,
                    0,
                    (UI_TREE_MAX_DEPTH, UI_TREE_MAX_ELEMENTS),
                    &mut elements,
                    &mut truncated,
                    layer,
                );
            }
        }
        if truncated {
            return Err("tree_truncated");
        }
        let mut found = None;
        for e in &elements {
            if !e.visible
                || e.disabled
                || role.is_some_and(|r| e.role != r && e.semantic_role != Some(r))
            {
                continue;
            }
            // A capped prefix may conceal another exact match. Refuse to claim
            // uniqueness rather than silently skipping that possible target.
            if name.is_some_and(|n| {
                e.name_truncated
                    && e.name
                        .as_deref()
                        .is_some_and(|prefix| n.starts_with(prefix))
            }) || text.is_some_and(|t| {
                e.text_truncated
                    && e.text
                        .as_deref()
                        .is_some_and(|prefix| t.starts_with(prefix))
            }) {
                return Err("text_truncated");
            }
            if name.is_some_and(|n| e.name.as_deref() != Some(n))
                || text.is_some_and(|t| e.text.as_deref() != Some(t))
            {
                continue;
            }
            let candidates: Vec<_> = if let Some(label) = button {
                if e.buttons_truncated {
                    return Err("buttons_truncated");
                }
                if e.buttons.iter().any(|b| {
                    b.text_truncated
                        && b.text
                            .as_deref()
                            .is_some_and(|prefix| label.starts_with(prefix))
                }) {
                    return Err("text_truncated");
                }

                e.buttons
                    .iter()
                    .filter(|b| {
                        !b.hidden
                            && !b.disabled
                            && !b.text_truncated
                            && b.text.as_deref() == Some(label)
                    })
                    .map(|b| b.bounds)
                    .collect()
            } else {
                if !e.clickable {
                    continue;
                }
                e.clipped_bounds.into_iter().collect()
            };
            for bounds in candidates {
                if found.is_some() {
                    return Err("ambiguous_target");
                }
                let Some(clip) = e.clipped_bounds else {
                    continue;
                };
                let bounds = (
                    bounds.0.max(clip.0),
                    bounds.1.max(clip.1),
                    bounds.2.min(clip.2),
                    bounds.3.min(clip.3),
                );
                if bounds.0 > bounds.2 || bounds.1 > bounds.3 {
                    continue;
                }
                found = Some((e.id, ((bounds.0 + bounds.2) / 2, (bounds.1 + bounds.3) / 2)));
            }
        }
        let (identifier, (x, y)) = found.ok_or("target_not_found")?;
        let mut remaining = identifier;
        let mut target = None;
        for (root, _) in roots {
            // SAFETY: the graphics lock covers traversal of the live root.
            if let Some(obj) = unsafe { crate::inspection::object_at(root, &mut remaining, 0) } {
                target = Some(obj);
                break;
            }
        }
        let target = target.ok_or("target_not_found")?;
        let mut point = lvgl::lv_point_t { x, y };
        let mut hit = core::ptr::null_mut();
        for (root, _) in roots.into_iter().rev() {
            // SAFETY: the root is live under the graphics lock and point is writable.
            hit = unsafe { lvgl::lv_indev_search_obj(root, &mut point) };
            if !hit.is_null() {
                break;
            }
        }
        // A labelled child may receive the pointer. Require it to belong to the target.
        let mut owner = hit;
        while !owner.is_null() && owner != target {
            // SAFETY: hit testing returned a live object and the lock protects its ancestors.
            owner = unsafe { lvgl::lv_obj_get_parent(owner) };
        }
        if owner != target {
            return Err("target_obscured");
        }
        let point = crate::Point::new(
            i16::try_from(x).map_err(|_| "invalid_coordinates")?,
            i16::try_from(y).map_err(|_| "invalid_coordinates")?,
        );
        // SAFETY: the graphics lock is held and the manager owns the display.
        validate_pointer_point(point, unsafe { inner.displays[0].get_resolution() })
            .map_err(|_| "invalid_coordinates")?;
        self.injected_inputs
            .pointer_click(point)
            .map_err(|_| "busy")?;
        Ok((x, y))
    }

    /// Capture the active LVGL screen into an owned, bounded RGBA8888 buffer.
    pub async fn capture_screen(&self) -> Result<crate::ScreenSnapshot> {
        let mut guard = self.lock().await;
        guard.with_lvgl(|_| ())?;
        let inner = self.inner.read().await;
        let display = inner.displays.first().ok_or(Error::NotAvailable)?;
        // SAFETY: the graphics lock is held and the manager owns the display.
        unsafe { display.capture_screen() }
    }

    /// Return the current active LVGL display resolution.
    pub async fn get_resolution(&self) -> Result<crate::Point> {
        let _lock = self.lock().await;
        let inner = self.inner.read().await;
        let display = inner.displays.first().ok_or(Error::NotAvailable)?;
        // SAFETY: the graphics lock is held and the manager owns the display.
        Ok(unsafe { display.get_resolution() })
    }

    /// Read the backlight brightness of the main screen, from
    /// [`BRIGHTNESS_OFF`](crate::BRIGHTNESS_OFF) to
    /// [`BRIGHTNESS_FULL`](crate::BRIGHTNESS_FULL).
    ///
    /// Returns [`Error::Unsupported`] when the screen has no brightness
    /// control. Brightness does not touch LVGL: callers may hold the LVGL lock
    /// or not.
    pub async fn get_brightness(&self) -> Result<u8> {
        let inner = self.inner.read().await;
        inner
            .displays
            .first()
            .ok_or(Error::NotAvailable)?
            .get_brightness()
    }

    /// Set the backlight brightness of the main screen; see
    /// [`Manager::get_brightness`]. Level [`BRIGHTNESS_OFF`](crate::BRIGHTNESS_OFF)
    /// turns the backlight off.
    pub async fn set_brightness(&self, level: u8) -> Result<()> {
        let inner = self.inner.read().await;
        inner
            .displays
            .first()
            .ok_or(Error::NotAvailable)?
            .set_brightness(level)
    }

    /// Queue a pointer move for the graphics-owned LVGL pointer input.
    pub async fn inject_pointer_move(&self, point: crate::Point) -> Result<()> {
        let _lock = self.lock().await;
        let inner = self.inner.read().await;
        let display = inner.displays.first().ok_or(Error::NotAvailable)?;
        // SAFETY: the graphics lock is held and the manager owns the display.
        let resolution = unsafe { display.get_resolution() };
        validate_pointer_point(point, resolution)?;
        self.injected_inputs.pointer_move(point)
    }

    /// Atomically queue a pointer move and complete click at a valid display point.
    pub async fn inject_pointer_click(&self, point: crate::Point) -> Result<()> {
        let _lock = self.lock().await;
        let inner = self.inner.read().await;
        let display = inner.displays.first().ok_or(Error::NotAvailable)?;
        // SAFETY: the graphics lock is held and the manager owns the display.
        let resolution = unsafe { display.get_resolution() };
        validate_pointer_point(point, resolution)?;
        self.injected_inputs.pointer_click(point)
    }

    /// Queue a pointer button state at the most recently injected pointer point.
    pub async fn inject_pointer_button(&self, state: State) -> Result<()> {
        let _lock = self.lock().await;
        self.injected_inputs.pointer_button(state)
    }

    /// Queue one keyboard key transition for the graphics-owned LVGL keypad.
    pub async fn inject_key(&self, key: Key, state: State) -> Result<()> {
        let _lock = self.lock().await;
        self.injected_inputs.key(key, state)
    }

    /// Queue a complete key press and release, or leave the queue unchanged.
    pub async fn inject_key_press(&self, key: Key) -> Result<()> {
        let _lock = self.lock().await;
        self.injected_inputs.key_press(key)
    }

    /// Queue printable ASCII text as ordered key press and release events.
    pub async fn inject_text(&self, text: &str) -> Result<usize> {
        let _lock = self.lock().await;
        self.injected_inputs.text(text)
    }

    /// Queue a bounded prefix of printable Unicode text, returning the accepted byte count.
    /// A queue-full error never enqueues a partial press/release pair.
    pub async fn inject_text_chunk(&self, text: &str) -> Result<usize> {
        let _lock = self.lock().await;
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

impl Drop for Manager {
    fn drop(&mut self) {
        let mut guard = block_on(lock_graphics());
        let inner = self.inner.get_mut();
        inner._inputs.clear();
        inner.displays.clear();
        let _ = guard
            .access
            .session()
            .and_then(|session| Lvgl::deinit(session).map_err(Error::from));
    }
}

/// Holds the process-wide graphics lock. Use [`Self::with_lvgl`] to run
/// operations that need a session. A guard can move between threads, and
/// only one thread can use it at a time.
///
/// ```compile_fail,E0277
/// fn require_sync<T: Sync>() {}
/// require_sync::<graphics::GraphicsGuard<'static>>();
/// ```
#[must_use = "Dropping the guard releases the graphics lock"]
pub struct GraphicsGuard<'a> {
    access: LvglGuard<'a>,
    not_sync: PhantomData<Cell<()>>,
}

impl GraphicsGuard<'_> {
    /// Run a synchronous operation in an LVGL session. The session token
    /// cannot escape the operation.
    ///
    /// ```compile_fail
    /// fn escape(guard: &mut graphics::GraphicsGuard<'_>) {
    ///     let _ = guard.with_lvgl(|lvgl| lvgl);
    /// }
    /// ```
    pub fn with_lvgl<T>(&mut self, function: impl FnOnce(&mut Lvgl) -> T) -> Result<T> {
        let mut session = self.access.session()?;
        crate::call::drain_retired_states(&mut session);
        Ok(function(&mut session))
    }
}

struct LvglGuard<'a> {
    _lock: MutexGuard<'a, CriticalSectionRawMutex, ()>,
}

// SAFETY: constructed only from GRAPHICS_LOCK, which serializes LVGL access
// and orders it between threads. The private guard is borrowed for sessions
// within this module. No borrow can escape with a 'static lifetime.
unsafe impl lvgl_rust::LvglAccess for LvglGuard<'_> {}

impl LvglGuard<'_> {
    fn session(&mut self) -> Result<lvgl_rust::Session<'_>> {
        Lvgl::enter(self).map_err(Error::from)
    }
}

pub(crate) async fn lock_graphics() -> GraphicsGuard<'static> {
    GraphicsGuard {
        access: LvglGuard {
            _lock: GRAPHICS_LOCK.lock().await,
        },
        not_sync: PhantomData,
    }
}

pub(crate) fn try_lock_graphics() -> Option<GraphicsGuard<'static>> {
    GRAPHICS_LOCK.try_lock().ok().map(|lock| GraphicsGuard {
        access: LvglGuard { _lock: lock },
        not_sync: PhantomData,
    })
}

extern "C" fn binding_tick_callback_function() -> u32 {
    let manager = TICK_TIME_MANAGER.load(Ordering::Acquire);
    assert!(!manager.is_null(), "Graphics time manager not initialized");
    // SAFETY: initialization publishes a static time manager before it sets this callback.
    unsafe { &*manager }.monotonic_milliseconds_wrapping()
}

// SAFETY: Manager owns its devices, and all access is through synchronized fields.
unsafe impl Send for Manager {}

// SAFETY: LVGL calls use GRAPHICS_LOCK, and Inner uses the reader-writer lock.
unsafe impl Sync for Manager {}

impl Manager {
    fn new(
        time_manager: &'static time::Manager<'static>,
        screen_device: &'static dyn DirectCharacterDevice,
        input_devices: &[(&'static dyn DirectCharacterDevice, InputKind)],
        buffer_size: usize,
        double_buffered: bool,
    ) -> Result<Self> {
        let injected_inputs = InputQueue::new()?;
        // Keep the lock until initialization and device registration complete.
        let mut guard = try_lock_graphics().ok_or(Error::AlreadyInitialized)?;
        let mut session = lvgl_rust::init(&mut guard.access).map_err(Error::from)?;
        TICK_TIME_MANAGER.store(time_manager as *const _ as *mut _, Ordering::Release);
        session.set_tick_cb(binding_tick_callback_function);
        crate::window::get_window_class();
        // SAFETY: the session holds the lock. The default group must exist
        // before input registration so keypads can join it.
        unsafe {
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
        theme::initialize(&display)?;

        Ok(Self {
            inner: RwLock::new(Inner {
                _inputs: inputs,
                displays: vec![display],
                on_screen_keyboard: None,
            }),
            window_parent: AtomicPtr::new(core::ptr::null_mut()),
            injected_inputs,
        })
    }

    /// Enable the system on-screen keyboard on the first display.
    ///
    /// Call during platform setup on touch-only devices, before creating any
    /// file pickers. A picker chooses its keyboard when it is created.
    /// Tapping a text area on the active screen opens the keyboard.
    pub async fn enable_on_screen_keyboard(&self) {
        let _guard = self.lock().await;
        let mut inner = self.inner.write().await;
        if inner.on_screen_keyboard.is_none() {
            let keyboard = Box::new(OnScreenKeyboard::new());
            // SAFETY: the display is live under the graphics lock. The box keeps
            // the keyboard state at a fixed address until display teardown.
            unsafe { keyboard.observe(inner.displays[0].get_lvgl_display()) };
            inner.on_screen_keyboard = Some(keyboard);
        }
    }

    pub async fn r#loop<F>(&self, sleep: impl Fn(Duration) -> F + Send + 'static) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut call_count: u8 = 0;

        loop {
            let time_until_next = {
                let mut guard = self.lock().await;
                let inner = self.inner.read().await;

                // Through the session: requests recorded by the safe layer
                // (dropped `Owned` values, `delete_later`) run right after.
                // One timer handler can drain up to 512 events per input device.
                // Draining a large paste therefore holds the graphics lock longer than an
                // ordinary frame; how long has not been measured on ESP32.
                let time_until_next = guard.with_lvgl(Lvgl::timer_handler)?;
                if let Some(keyboard) = &inner.on_screen_keyboard {
                    // SAFETY: the graphics lock protects the live display and keyboard.
                    unsafe { keyboard.update(inner.displays[0].get_lvgl_display()) };
                }

                if call_count >= 30 {
                    for display in &inner.displays {
                        // SAFETY: the graphics guard is still held.
                        unsafe { display.check_for_resizing() };
                    }
                    call_count = 0;
                }
                call_count = call_count.saturating_add(1);

                time_until_next
            };

            sleep(Duration::from_millis(time_until_next as u64)).await;
        }
    }

    /// Create the next windows inside `window_parent` instead of the screen.
    ///
    /// # Safety
    ///
    /// `window_parent` must be a live LVGL object that stays alive while it
    /// is the window parent and holds windows. It must not be inside a window.
    pub unsafe fn set_window_parent(&self, _lvgl: &mut Lvgl, window_parent: *mut lvgl::lv_obj_t) {
        self.window_parent.store(window_parent, Ordering::Relaxed);
    }

    fn window_parent(&self, lvgl: &Lvgl) -> Result<*mut lvgl::lv_obj_t> {
        let parent = self.window_parent.load(Ordering::Relaxed);
        if parent.is_null() {
            // The safe API can replace and delete the screen between sessions.
            self.get_current_screen(lvgl)
        } else {
            Ok(parent)
        }
    }

    /// Create a window inside the window parent. By default this is the
    /// active screen. Returns [`Error::AlreadyInUse`] if a pending screen
    /// transition can delete the parent. The manager must outlive the owner.
    ///
    /// ```compile_fail,E0061
    /// fn create(manager: &'static graphics::Manager) {
    ///     let _ = manager.create_window();
    /// }
    /// ```
    pub fn create_window(&'static self, lvgl: &mut Lvgl) -> Result<OwnedWindow> {
        let parent = self.window_parent(lvgl)?;
        // SAFETY: the session holds the graphics lock. The parent is the live
        // active screen or satisfies the set_window_parent lifetime contract.
        let can_be_deleted = unsafe {
            let screen = lvgl::lv_obj_get_screen(parent);
            let display = lvgl::lv_obj_get_display(parent);
            screen == lvgl::lv_display_get_screen_prev(display)
                || (screen == lvgl::lv_display_get_screen_active(display)
                    && !lvgl::lv_display_get_screen_loading(display).is_null())
        };
        if can_be_deleted {
            return Err(Error::AlreadyInUse);
        }

        // SAFETY: the session still holds the lock and the live parent is
        // outside the screens that the pending transition can delete.
        let window = unsafe { Window::new(parent)? };
        // SAFETY: the window was just created and the static manager keeps LVGL live.
        Ok(unsafe { OwnedWindow::from_raw(window) })
    }

    /// Register a physical input device under the graphics lock.
    pub async fn add_input_device(
        &self,
        input_device: &'static dyn DirectCharacterDevice,
        input_type: InputKind,
    ) -> Result<()> {
        let _guard = self.lock().await;
        let mut inner = self.inner.write().await;
        // SAFETY: LVGL is initialized and the lock covers registration.
        // The manager also holds the lock when it drops the input.
        let input = unsafe { Input::new(input_device, input_type)? };
        inner._inputs.push(input);
        Ok(())
    }

    fn create_display(
        screen_device: &'static dyn DirectCharacterDevice,
        input_devices: &[(&'static dyn DirectCharacterDevice, InputKind)],
        buffer_size: usize,
        double_buffered: bool,
        injected_inputs: synchronization::Arc<InputQueue>,
    ) -> Result<(Display, Vec<Input>)> {
        // SAFETY: initialization holds the lock and the manager drops the
        // display under the lock before deinitializing LVGL.
        let display = unsafe { Display::new(screen_device, buffer_size, double_buffered)? };
        // SAFETY: initialization holds the graphics lock, including on
        // failure. The manager holds it when dropping the returned inputs.
        let mut inputs = unsafe {
            vec![
                Input::new_virtual(injected_inputs.clone(), InputKind::Pointer)?,
                Input::new_virtual(injected_inputs, InputKind::Keypad)?,
            ]
        };
        for (device, kind) in input_devices {
            // SAFETY: initialization holds the graphics lock and the manager
            // also holds it when dropping inputs.
            inputs.push(unsafe { Input::new(*device, *kind)? });
        }
        Ok((display, inputs))
    }

    pub fn get_window_count(&self, lvgl: &Lvgl) -> Result<usize> {
        let parent = self.window_parent(lvgl)?;
        // SAFETY: the parent stays live and the session holds the lock.
        Ok(unsafe { lvgl::lv_obj_get_child_count(parent) } as usize)
    }

    pub fn get_window_icon(&self, lvgl: &Lvgl, index: usize) -> Result<(String, Color)> {
        let index = i32::try_from(index).map_err(|_| Error::InvalidWindowIdentifier)?;
        let parent = self.window_parent(lvgl)?;
        // SAFETY: the parent stays live and the session excludes changes to
        // the tree and icon fields throughout this operation.
        let (text, color) = unsafe {
            let child = lvgl::lv_obj_get_child(parent, index);
            let window = Window::from_raw(child).ok_or(Error::InvalidWindowIdentifier)?;
            Window::icon(window)
        };
        Ok((text.as_str().to_string(), color))
    }

    pub fn get_window_identifier(&self, lvgl: &Lvgl, index: usize) -> Result<usize> {
        let index = i32::try_from(index).map_err(|_| Error::InvalidWindowIdentifier)?;
        let parent = self.window_parent(lvgl)?;
        // SAFETY: the parent stays live and the session holds the lock.
        Ok(unsafe { lvgl::lv_obj_get_child(parent, index) } as usize)
    }

    fn find_window(&self, lvgl: &Lvgl, identifier: usize) -> Result<*mut lvgl::lv_obj_t> {
        let parent = self.window_parent(lvgl)?;
        // SAFETY: the parent stays live and the session excludes changes to its children.
        unsafe {
            (0..lvgl::lv_obj_get_child_count(parent) as i32)
                .map(|index| lvgl::lv_obj_get_child(parent, index))
                .find(|child| *child as usize == identifier)
                .ok_or(Error::InvalidWindowIdentifier)
        }
    }

    pub fn maximize_window(&self, lvgl: &mut Lvgl, identifier: usize) -> Result<()> {
        let window = self.find_window(lvgl, identifier)?;
        // SAFETY: find_window resolved a live child and the session still
        // holds the graphics lock.
        unsafe { lvgl::lv_obj_move_foreground(window) };
        Ok(())
    }

    /// Request that a window close. A pending request satisfies later calls
    /// until the window consumes it.
    ///
    /// Returns `InvalidWindowIdentifier` if the identifier does not identify
    /// a window, or `NotAvailable` if dispatch fails or leaves no pending request.
    pub fn send_window_close_request(&self, lvgl: &mut Lvgl, identifier: usize) -> Result<()> {
        let object = self.find_window(lvgl, identifier)?;
        // SAFETY: the session keeps the live child stable during lookup.
        let window = unsafe { Window::from_raw(object) }.ok_or(Error::InvalidWindowIdentifier)?;
        // SAFETY: the session excludes other tasks through dispatch. LVGL
        // returns INVALID if a handler deletes the object during dispatch.
        let result = unsafe {
            lvgl::lv_obj_send_event(
                object,
                crate::EventKind::CloseRequested.into_lvgl_code(),
                core::ptr::null_mut(),
            )
        };
        // A preprocessing handler can stop dispatch without returning INVALID.
        if result != lvgl::lv_result_t_LV_RESULT_OK || !unsafe { Window::has_close_request(window) }
        {
            return Err(Error::NotAvailable);
        }
        Ok(())
    }

    pub async fn lock_function<T>(&self, function: impl FnOnce() -> Result<T>) -> Result<T> {
        let _lock = self.lock().await;
        function()
    }

    pub async fn lock(&self) -> GraphicsGuard<'_> {
        lock_graphics().await
    }

    pub fn synchronous_lock(&self) -> GraphicsGuard<'_> {
        block_on(self.lock())
    }

    pub fn try_lock(&self) -> Option<GraphicsGuard<'_>> {
        try_lock_graphics()
    }

    /// Run a synchronous operation under the graphics lock, in an LVGL
    /// session. Use the existing guard if the lock is already held.
    pub async fn with_lvgl<T>(&self, function: impl FnOnce(&mut Lvgl) -> Result<T>) -> Result<T> {
        self.lock().await.with_lvgl(function)?
    }

    /// [`Manager::with_lvgl`] from synchronous code.
    pub fn with_lvgl_sync<T>(&self, function: impl FnOnce(&mut Lvgl) -> Result<T>) -> Result<T> {
        block_on(self.with_lvgl(function))
    }

    pub fn get_current_screen(&self, _lvgl: &Lvgl) -> Result<*mut lvgl::lv_obj_t> {
        // SAFETY: the session holds the graphics lock.
        let screen = unsafe { lvgl::lv_screen_active() };
        core::ptr::NonNull::new(screen)
            .map(|screen| screen.as_ptr())
            .ok_or(Error::NotAvailable)
    }

    pub async fn update_theme(
        &self,
        primary_color: Color,
        secondary_color: Color,
        is_dark: bool,
    ) -> Result<()> {
        let mut guard = self.lock().await;
        self.update_theme_locked(&mut guard, primary_color, secondary_color, is_dark)
            .await
    }

    pub(crate) async fn update_theme_locked(
        &self,
        _guard: &mut GraphicsGuard<'_>,
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

    /// Select how much the theme may spend on rendering, on every display.
    ///
    /// The mode is [`theme::RenderingMode::Default`] until a platform selects
    /// another one for a slow display, ideally before it creates the interface.
    pub async fn set_rendering_mode(&self, mode: theme::RenderingMode) -> Result<()> {
        let _lock = self.lock().await;
        for display in &self.inner.read().await.displays {
            theme::set_rendering_mode(display, mode)?;
        }
        Ok(())
    }

    /// The rendering mode in use.
    pub fn get_rendering_mode(&self) -> theme::RenderingMode {
        theme::get_rendering_mode()
    }
}
