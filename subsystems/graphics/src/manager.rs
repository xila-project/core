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

        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        if inner.displays.is_empty() {
            return Err(Error::NotAvailable);
        }

        let mut elements = Vec::with_capacity(max_elements);
        let mut truncated = false;
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
        let _lock = self.global_lock.lock().await;
        let inner = self.inner.read().await;
        let screen = inner.displays.first().ok_or("not_available")?.get_object();
        let display = unsafe { lvgl::lv_obj_get_display(screen) };
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
        let (id, (x, y)) = found.ok_or("target_not_found")?;
        let mut remaining = id;
        let mut target = None;
        for (root, _) in roots {
            if let Some(obj) = unsafe { crate::inspection::object_at(root, &mut remaining, 0) } {
                target = Some(obj);
                break;
            }
        }
        let target = target.ok_or("target_not_found")?;
        let mut point = lvgl::lv_point_t { x, y };
        let mut hit = core::ptr::null_mut();
        for (root, _) in roots.into_iter().rev() {
            hit = unsafe { lvgl::lv_indev_search_obj(root, &mut point) };
            if !hit.is_null() {
                break;
            }
        }
        // A labelled child may receive the pointer. Require it to belong to the target.
        let mut owner = hit;
        while !owner.is_null() && owner != target {
            owner = unsafe { lvgl::lv_obj_get_parent(owner) };
        }
        if owner != target {
            return Err("target_obscured");
        }
        let point = crate::Point::new(
            i16::try_from(x).map_err(|_| "invalid_coordinates")?,
            i16::try_from(y).map_err(|_| "invalid_coordinates")?,
        );
        validate_pointer_point(point, inner.displays[0].get_resolution())
            .map_err(|_| "invalid_coordinates")?;
        self.injected_inputs
            .pointer_click(point)
            .map_err(|_| "busy")?;
        Ok((x, y))
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

impl Drop for Manager {
    fn drop(&mut self) {
        // `Lvgl::deinit` also tells the safe layer that LVGL is gone.
        let deinitialized = self.try_lock_lvgl().is_some_and(|mut guard| {
            lvgl_rust::Lvgl::enter(&mut guard)
                .and_then(lvgl_rust::Lvgl::deinit)
                .is_ok()
        });
        if !deinitialized {
            unsafe {
                lvgl::lv_deinit();
            }
        }
    }
}

/// The graphics lock, held for the safe `lvgl_rust` API: proof of exclusive
/// access to LVGL ([`lvgl_rust::LvglAccess`]). Private: values live on the
/// stack of the manager's methods, so none can be leaked for `'static`.
struct LvglGuard<'a>(#[allow(dead_code)] MutexGuard<'a, CriticalSectionRawMutex, ()>);

// SAFETY: built only from the manager's `global_lock`, the mutex that
// serializes every LVGL call, and dedicated to it. Releasing the mutex
// synchronizes-with the next acquisition on any thread. Values never leave
// the manager's methods, so no session can be borrowed for `'static`.
unsafe impl lvgl_rust::LvglAccess for LvglGuard<'_> {}

impl LvglGuard<'_> {
    /// Starts a session of the safe API ([`lvgl_rust::Lvgl::enter`]): fails
    /// if LVGL was not initialized through the manager, or if a session or
    /// a Rust callback is active.
    fn session(&mut self) -> Result<lvgl_rust::Session<'_>> {
        lvgl_rust::Lvgl::enter(self).map_err(Error::from)
    }
}

/// Proof that the caller of [`Manager::with_lvgl_locked`] holds the graphics
/// lock. `!Send`, and local to that method.
struct Held(core::marker::PhantomData<*mut ()>);

// SAFETY: `with_lvgl_locked`'s contract: the caller holds `global_lock` while
// the value exists, so the mutex orders successive holders. The value is a
// local of that method and is never borrowed for `'static`.
unsafe impl lvgl_rust::LvglAccess for Held {}

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
        let global_lock = Mutex::new(());
        {
            // `lvgl_rust::init` runs `lv_init` and starts the safe layer's
            // registry; nothing else can reach LVGL before `Self` exists.
            let mut guard = LvglGuard(
                global_lock
                    .try_lock()
                    .map_err(|_| Error::AlreadyInitialized)?,
            );
            let mut session = lvgl_rust::init(&mut guard).map_err(Error::from)?;
            session.set_tick_cb(binding_tick_callback_function);
            unsafe {
                // LVGL assigns newly created keypads to the default group.
                // Create it before display creation also creates the
                // virtual keypad.
                let group = lvgl::lv_group_create();
                lvgl::lv_group_set_default(group);
            }
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
            global_lock,
            injected_inputs,
        })
    }

    pub async fn r#loop<F>(&self, sleep: impl Fn(Duration) -> F + Send + 'static) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut call_count: u8 = 0;

        loop {
            let time_until_next = {
                let mut guard = self.lock_lvgl().await;
                let inner = self.inner.read().await;

                // Through the session: requests recorded by the safe layer
                // (dropped `Owned` values, `delete_later`) run right after.
                let time_until_next = guard.session()?.timer_handler();

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

    async fn lock_lvgl(&self) -> LvglGuard<'_> {
        LvglGuard(self.global_lock.lock().await)
    }

    fn try_lock_lvgl(&self) -> Option<LvglGuard<'_>> {
        self.global_lock.try_lock().ok().map(LvglGuard)
    }

    /// Runs `function` with the safe LVGL API ([`lvgl_rust`]), under the
    /// graphics lock (the one [`Manager::lock`] takes). Do not call from
    /// callbacks or while holding that lock: acquiring it again would deadlock.
    pub async fn with_lvgl<T>(
        &self,
        function: impl FnOnce(&mut lvgl_rust::Lvgl) -> Result<T>,
    ) -> Result<T> {
        let mut guard = self.lock_lvgl().await;
        let mut session = guard.session()?;
        function(&mut session)
    }

    /// [`Manager::with_lvgl`] from synchronous code.
    pub fn with_lvgl_sync<T>(
        &self,
        function: impl FnOnce(&mut lvgl_rust::Lvgl) -> Result<T>,
    ) -> Result<T> {
        block_on(self.with_lvgl(function))
    }

    /// [`Manager::with_lvgl`] for code that already holds the graphics lock
    /// (a guard of [`Manager::lock`]), such as a dispatcher that mixes raw
    /// calls and the safe API.
    ///
    /// # Safety
    ///
    /// The caller must hold this manager's graphics lock for the whole call.
    pub unsafe fn with_lvgl_locked<T>(
        &self,
        function: impl FnOnce(&mut lvgl_rust::Lvgl) -> Result<T>,
    ) -> Result<T> {
        let mut held = Held(core::marker::PhantomData);
        let mut session = lvgl_rust::Lvgl::enter(&mut held).map_err(Error::from)?;
        function(&mut session)
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
