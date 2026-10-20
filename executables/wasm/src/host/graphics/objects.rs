use super::canvas::{IndexedCanvas, MAX_CANVAS_PIXELS};
use super::{
    error::{GraphicsError, Result},
    marshal::ButtonMap,
};
use crate::host::translation::WasmUsize;
use alloc::{rc::Rc, vec::Vec};
use core::{cell::Cell, ptr::NonNull};
use xila::graphics::{Manager, Window, lvgl};

pub const MAX_OBJECTS: usize = 256;

/// Shared callback data never aliases the registry's mutable entries. LVGL holds
/// one Rc until DELETE, and the registry holds another until VM teardown.
struct Notification {
    live: Cell<*mut lvgl::lv_obj_t>,
    root: Option<Rc<Notification>>,
}

unsafe extern "C" fn deleted(event: *mut lvgl::lv_event_t) {
    let data = unsafe { lvgl::lv_event_get_user_data(event) };
    let notification = unsafe { Rc::from_raw(data.cast::<Notification>()) };
    let pointer = notification.live.replace(core::ptr::null_mut());
    if let Some(root) = &notification.root {
        let window = root.live.get();
        // The manager lock serializes deletion; a root's DELETE callback clears
        // its live pointer before it or its queue can be destroyed.
        if let Some(mut window) = unsafe { Window::from_raw(window) } {
            unsafe { window.as_mut().discard_events_for_target(pointer) };
        }
    }
    // Release LVGL's callback ownership exactly once, on deletion.
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Window,
    Object,
    Label,
    ButtonMatrix,
    Checkbox,
    TabView,
    IndexedCanvas,
    Textarea,
    Keyboard,
    Roller,
    Arc,
    List,
}

struct Entry {
    notification: Rc<Notification>,
    kind: Kind,
    map: Option<ButtonMap>,
    canvas: Option<Vec<IndexedCanvas>>,
}

pub struct GraphicsState {
    pub(crate) manager: Option<&'static Manager>,
    entries: Vec<Entry>,
}

impl GraphicsState {
    pub fn new(manager: Option<&'static Manager>) -> Self {
        Self {
            manager,
            entries: Vec::new(),
        }
    }

    pub(crate) fn ensure_capacity(&self) -> Result<()> {
        if self.entries.len() >= MAX_OBJECTS {
            Err(GraphicsError::ResourceLimit)
        } else {
            Ok(())
        }
    }

    /// Caller holds the manager lock after any native deletion has returned.
    /// DELETE callbacks only invalidate: LVGL can still read maps while tearing
    /// down an object, so release retained buffers here, not in the callback.
    pub(crate) fn reap_deleted_maps(&mut self) {
        for entry in &mut self.entries {
            if entry.notification.live.get().is_null() {
                entry.map = None;
                entry.canvas = None;
            }
        }
    }

    fn entry(&self, handle: WasmUsize) -> Result<&Entry> {
        let index = usize::try_from(handle)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .ok_or(GraphicsError::InvalidHandle)?;
        let entry = self
            .entries
            .get(index)
            .ok_or(GraphicsError::InvalidHandle)?;
        if entry.notification.live.get().is_null() {
            return Err(GraphicsError::InvalidHandle);
        }
        Ok(entry)
    }

    pub(crate) fn object(
        &self,
        handle: WasmUsize,
        kind: Option<Kind>,
    ) -> Result<*mut lvgl::lv_obj_t> {
        let entry = self.entry(handle)?;
        if kind.is_some_and(|kind| entry.kind != kind) {
            return Err(GraphicsError::InvalidHandle);
        }
        Ok(entry.notification.live.get())
    }

    pub(crate) fn handle_for(&self, pointer: *mut lvgl::lv_obj_t) -> WasmUsize {
        if pointer.is_null() {
            return 0;
        }
        self.entries
            .iter()
            .position(|entry| entry.notification.live.get() == pointer)
            .map_or(0, |index| (index + 1) as WasmUsize)
    }

    /// Caller holds the manager lock and owns a newly created native object.
    pub(crate) fn insert(
        &mut self,
        pointer: *mut lvgl::lv_obj_t,
        kind: Kind,
        parent: Option<WasmUsize>,
    ) -> Result<WasmUsize> {
        NonNull::new(pointer).ok_or(GraphicsError::NativeFailure)?;
        let root = parent
            .map(|handle| {
                let parent = self.entry(handle)?;
                Ok(parent
                    .notification
                    .root
                    .as_ref()
                    .unwrap_or(&parent.notification)
                    .clone())
            })
            .transpose()?;
        let notification = Rc::new(Notification {
            live: Cell::new(pointer),
            root,
        });
        // This is the capability's birth boundary. Queued events predate its
        // registration and may refer to an unregistered object's recycled
        // address. Preserve their codes, but never grant them this new handle.
        let window = notification
            .root
            .as_ref()
            .map_or(pointer, |root| root.live.get());
        if let Some(mut window) = unsafe { Window::from_raw(window) } {
            unsafe { window.as_mut().clear_event_targets_for(pointer) };
        }
        let callback = Rc::into_raw(notification.clone());
        let descriptor = unsafe {
            lvgl::lv_obj_add_event_cb(
                pointer,
                Some(deleted),
                lvgl::lv_event_code_t_LV_EVENT_DELETE,
                callback as *mut _,
            )
        };
        if descriptor.is_null() {
            unsafe {
                drop(Rc::from_raw(callback));
                lvgl::lv_obj_delete(pointer);
            }
            return Err(GraphicsError::NativeFailure);
        }
        self.entries.push(Entry {
            notification,
            kind,
            map: None,
            canvas: None,
        });
        Ok(self.entries.len() as WasmUsize)
    }

    pub(crate) fn set_map(&mut self, handle: WasmUsize, map: ButtonMap) {
        // Install first; LVGL may access the previous map during replacement.
        let entry = &mut self.entries[handle as usize - 1];
        unsafe { lvgl::lv_buttonmatrix_set_map(entry.notification.live.get(), map.as_ptr()) };
        entry.map = Some(map);
    }

    pub(crate) fn install_canvas(
        &mut self,
        handle: WasmUsize,
        mut canvas: Vec<IndexedCanvas>,
    ) -> Result<()> {
        let used: usize = self
            .entries
            .iter()
            .filter_map(|entry| entry.canvas.as_ref())
            .map(|canvas| canvas[0].pixels.len())
            .sum();
        if used + canvas[0].pixels.len() > MAX_CANVAS_PIXELS {
            return Err(GraphicsError::ResourceLimit);
        }
        let entry = &mut self.entries[handle as usize - 1];
        unsafe {
            IndexedCanvas::attach(canvas.as_mut_ptr(), entry.notification.live.get())?;
        }
        entry.canvas = Some(canvas);
        Ok(())
    }

    pub(crate) fn canvas_mut(&mut self, handle: WasmUsize) -> &mut IndexedCanvas {
        &mut self.entries[handle as usize - 1]
            .canvas
            .as_mut()
            .expect("canvas kind checked")[0]
    }
}

impl Drop for GraphicsState {
    fn drop(&mut self) {
        let Some(manager) = self.manager else { return };
        let _lock = manager.synchronous_lock();
        for entry in &self.entries {
            if entry.notification.root.is_none() {
                let pointer = entry.notification.live.get();
                if !pointer.is_null() {
                    unsafe { lvgl::lv_obj_delete(pointer) };
                }
            }
        }
        // Retained maps and notifications are freed only after native deletion.
    }
}
