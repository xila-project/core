use super::canvas::{IndexedCanvas, MAX_CANVAS_PIXELS};
use super::{
    error::{GraphicsError, Result},
    marshal::{
        ButtonMap, GridTracks, MAX_WIDGET_OPTIONS_BYTES_PER_VM, updated_widget_options_usage,
    },
};
use crate::host::translation::WasmUsize;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::{cell::Cell, ptr::NonNull};
use xila::graphics::{Manager, Window, lvgl};

pub const MAX_OBJECTS: usize = 256;
pub const MAX_GRID_TRACKS_PER_VM: usize = 512;

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
    FilePicker,
    Object,
    Label,
    ButtonMatrix,
    Checkbox,
    TabView,
    IndexedCanvas,
    Textarea,
    Keyboard,
    Arc,
    Bar,
    Slider,
    Switch,
    Spinner,
    Dropdown,
    Roller,
    Led,
    Calendar,
    Spinbox,
    Chart,
    Line,
    List,
    ListText,
    ListButton,
    Tileview,
    Tile,
    Button,
    Badge,
    Card,
}

struct Entry {
    notification: Rc<Notification>,
    kind: Kind,
    tile_position: Option<(u8, u8)>,
    map: Option<ButtonMap>,
    canvas: Option<Vec<IndexedCanvas>>,
    grid: Option<GridTracks>,
    option_bytes: u16,
    widget: Option<Box<WidgetResource>>,
}

enum WidgetResource {
    Calendar {
        highlights: Option<Vec<lvgl::lv_calendar_date_t>>,
        charged_bytes: u16,
    },
    Chart {
        series: [Option<NonNull<lvgl::lv_chart_series_t>>; 8],
        series_count: u8,
    },
    Line {
        points: Option<Vec<lvgl::lv_point_precise_t>>,
        charged_bytes: u16,
    },
}

#[cfg(test)]
const _: () = assert!(core::mem::size_of::<Entry>() <= 192);

pub struct GraphicsState {
    pub(crate) manager: Option<&'static Manager>,
    entries: Vec<Entry>,
    grid_tracks: usize,
    widget_options_bytes: usize,
    /// Trusted native context, refreshed from GlobalStore at the VM boundary.
    pub(crate) file_system: Option<(
        &'static xila::virtual_file_system::VirtualFileSystem,
        xila::task::TaskIdentifier,
    )>,
}

impl GraphicsState {
    pub fn new(manager: Option<&'static Manager>) -> Self {
        Self {
            manager,
            entries: Vec::new(),
            grid_tracks: 0,
            widget_options_bytes: 0,
            file_system: None,
        }
    }

    /// Supply the caller identity when using the VM-independent dispatcher.
    /// The Wasmi adapter always supplies its own trusted store context.
    pub fn with_file_system(
        manager: Option<&'static Manager>,
        vfs: &'static xila::virtual_file_system::VirtualFileSystem,
        task: xila::task::TaskIdentifier,
    ) -> Self {
        let mut state = Self::new(manager);
        state.file_system = Some((vfs, task));
        state
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
        let mut deleted_grid_tracks = 0;
        let mut deleted_option_bytes = 0;
        for entry in &mut self.entries {
            if entry.notification.live.get().is_null() {
                entry.map = None;
                entry.canvas = None;
                deleted_grid_tracks += entry.grid.take().map_or(0, |grid| grid.track_count());
                deleted_option_bytes += core::mem::replace(&mut entry.option_bytes, 0) as usize;
                if let Some(resource) = entry.widget.take() {
                    deleted_option_bytes += match *resource {
                        WidgetResource::Calendar { charged_bytes, .. }
                        | WidgetResource::Line { charged_bytes, .. } => usize::from(charged_bytes),
                        WidgetResource::Chart { .. } => 0,
                    };
                }
            }
        }
        self.grid_tracks = self.grid_tracks.saturating_sub(deleted_grid_tracks);
        self.widget_options_bytes = self
            .widget_options_bytes
            .saturating_sub(deleted_option_bytes);
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

    pub(crate) fn kind(&self, handle: WasmUsize) -> Result<Kind> {
        Ok(self.entry(handle)?.kind)
    }

    pub(crate) fn is_child_of(
        &self,
        child: WasmUsize,
        parent: WasmUsize,
        kind: Kind,
    ) -> Result<bool> {
        let child_pointer = self.object(child, Some(kind))?;
        let parent_pointer = self.object(parent, None)?;
        Ok(unsafe { lvgl::lv_obj_get_parent(child_pointer) } == parent_pointer)
    }

    pub(crate) fn tile_position_in_use(
        &self,
        view: WasmUsize,
        column: u8,
        row: u8,
    ) -> Result<bool> {
        let view_pointer = self.object(view, Some(Kind::Tileview))?;
        Ok(self.entries.iter().any(|entry| {
            !entry.notification.live.get().is_null()
                && entry.kind == Kind::Tile
                && entry.tile_position == Some((column, row))
                && unsafe { lvgl::lv_obj_get_parent(entry.notification.live.get()) } == view_pointer
        }))
    }

    pub(crate) fn set_tile_position(
        &mut self,
        tile_handle: WasmUsize,
        position: (u8, u8),
    ) -> Result<()> {
        let index = usize::try_from(tile_handle)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .ok_or(GraphicsError::InvalidHandle)?;
        let tile = self
            .entries
            .get_mut(index)
            .ok_or(GraphicsError::InvalidHandle)?;
        if tile.notification.live.get().is_null() || tile.kind != Kind::Tile {
            return Err(GraphicsError::InvalidHandle);
        }
        tile.tile_position = Some(position);
        Ok(())
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
            tile_position: None,
            map: None,
            canvas: None,
            grid: None,
            option_bytes: 0,
            widget: None,
        });
        Ok(self.entries.len() as WasmUsize)
    }

    pub(crate) fn set_map(&mut self, handle: WasmUsize, map: ButtonMap) {
        // Install first; LVGL may access the previous map during replacement.
        let entry = &mut self.entries[handle as usize - 1];
        unsafe { lvgl::lv_buttonmatrix_set_map(entry.notification.live.get(), map.as_ptr()) };
        entry.map = Some(map);
    }

    pub(crate) fn set_grid_tracks(&mut self, handle: WasmUsize, tracks: GridTracks) -> Result<()> {
        let index = usize::try_from(handle)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .ok_or(GraphicsError::InvalidHandle)?;
        let entry = self
            .entries
            .get_mut(index)
            .ok_or(GraphicsError::InvalidHandle)?;
        let previous = entry.grid.as_ref().map_or(0, GridTracks::track_count);
        let used = self
            .grid_tracks
            .checked_sub(previous)
            .and_then(|used| used.checked_add(tracks.track_count()))
            .ok_or(GraphicsError::ResourceLimit)?;
        if used > MAX_GRID_TRACKS_PER_VM {
            return Err(GraphicsError::ResourceLimit);
        }
        let object = entry.notification.live.get();
        if object.is_null() {
            return Err(GraphicsError::InvalidHandle);
        }
        // LVGL switches descriptors synchronously; the old host allocation is
        // dropped only after this call returns.
        unsafe {
            lvgl::lv_obj_set_grid_dsc_array(object, tracks.columns_ptr(), tracks.rows_ptr());
        }
        entry.grid = Some(tracks);
        self.grid_tracks = used;
        Ok(())
    }

    pub(crate) fn validate_widget_options_size(
        &self,
        handle: WasmUsize,
        new_bytes: usize,
    ) -> Result<()> {
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(GraphicsError::ResourceLimit);
        }
        let entry = self.entry(handle)?;
        updated_widget_options_usage(
            self.widget_options_bytes,
            entry.option_bytes as usize,
            new_bytes,
        )?;
        Ok(())
    }

    pub(crate) fn validate_new_widget_options_size(&self, new_bytes: usize) -> Result<()> {
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(GraphicsError::ResourceLimit);
        }
        updated_widget_options_usage(self.widget_options_bytes, 0, new_bytes)?;
        Ok(())
    }

    pub(crate) fn commit_widget_options_size(
        &mut self,
        handle: WasmUsize,
        new_bytes: usize,
    ) -> Result<()> {
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(GraphicsError::ResourceLimit);
        }
        let index = usize::try_from(handle)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .ok_or(GraphicsError::InvalidHandle)?;
        let entry = self
            .entries
            .get_mut(index)
            .ok_or(GraphicsError::InvalidHandle)?;
        let previous = entry.option_bytes as usize;
        let charge = previous.max(new_bytes);
        let used = updated_widget_options_usage(self.widget_options_bytes, previous, new_bytes)?;
        entry.option_bytes = charge as u16;
        self.widget_options_bytes = used;
        Ok(())
    }

    pub(crate) fn replace_calendar_highlights(
        &mut self,
        handle: WasmUsize,
        mut dates: Vec<lvgl::lv_calendar_date_t>,
    ) -> Result<(
        *mut lvgl::lv_calendar_date_t,
        Option<Vec<lvgl::lv_calendar_date_t>>,
    )> {
        let new_bytes = dates
            .len()
            .checked_mul(core::mem::size_of::<lvgl::lv_calendar_date_t>())
            .ok_or(GraphicsError::ResourceLimit)?;
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(GraphicsError::ResourceLimit);
        }
        let index = usize::try_from(handle)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .ok_or(GraphicsError::InvalidHandle)?;
        let entry = self
            .entries
            .get(index)
            .ok_or(GraphicsError::InvalidHandle)?;
        if entry.notification.live.get().is_null() || entry.kind != Kind::Calendar {
            return Err(GraphicsError::InvalidHandle);
        }
        let previous = match entry.widget.as_deref() {
            None => 0,
            Some(WidgetResource::Calendar { charged_bytes, .. }) => usize::from(*charged_bytes),
            Some(_) => return Err(GraphicsError::InvalidHandle),
        };
        let charge = previous.max(new_bytes);
        let used = updated_widget_options_usage(self.widget_options_bytes, previous, new_bytes)?;
        let pointer = if dates.is_empty() {
            core::ptr::null_mut()
        } else {
            dates.as_mut_ptr()
        };
        let entry = &mut self.entries[index];
        let replacement = if dates.is_empty() { None } else { Some(dates) };
        let resource = entry.widget.get_or_insert_with(|| {
            Box::new(WidgetResource::Calendar {
                highlights: None,
                charged_bytes: 0,
            })
        });
        let WidgetResource::Calendar {
            highlights,
            charged_bytes,
        } = &mut **resource
        else {
            return Err(GraphicsError::InvalidHandle);
        };
        let old = core::mem::replace(highlights, replacement);
        *charged_bytes = u16::try_from(charge).map_err(|_| GraphicsError::ResourceLimit)?;
        self.widget_options_bytes = used;
        Ok((pointer, old))
    }

    pub(crate) fn chart_series_count(&self, handle: WasmUsize) -> Result<usize> {
        let entry = self.entry(handle)?;
        if entry.kind != Kind::Chart {
            return Err(GraphicsError::InvalidHandle);
        }
        match entry.widget.as_deref() {
            None => Ok(0),
            Some(WidgetResource::Chart { series_count, .. }) => Ok(usize::from(*series_count)),
            Some(_) => Err(GraphicsError::InvalidHandle),
        }
    }

    pub(crate) fn add_chart_series(
        &mut self,
        handle: WasmUsize,
        series_pointer: NonNull<lvgl::lv_chart_series_t>,
    ) -> Result<u32> {
        let index = usize::try_from(handle)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .ok_or(GraphicsError::InvalidHandle)?;
        let entry = self
            .entries
            .get_mut(index)
            .ok_or(GraphicsError::InvalidHandle)?;
        if entry.notification.live.get().is_null() {
            return Err(GraphicsError::InvalidHandle);
        }
        if entry.kind != Kind::Chart {
            return Err(GraphicsError::InvalidHandle);
        }
        let resource = entry.widget.get_or_insert_with(|| {
            Box::new(WidgetResource::Chart {
                series: [None; 8],
                series_count: 0,
            })
        });
        let WidgetResource::Chart {
            series,
            series_count,
        } = &mut **resource
        else {
            return Err(GraphicsError::InvalidHandle);
        };
        let slot = usize::from(*series_count);
        if slot >= series.len() {
            return Err(GraphicsError::ResourceLimit);
        }
        series[slot] = Some(series_pointer);
        *series_count += 1;
        u32::try_from(slot).map_err(|_| GraphicsError::ResourceLimit)
    }

    pub(crate) fn chart_series(
        &self,
        handle: WasmUsize,
        slot: WasmUsize,
    ) -> Result<NonNull<lvgl::lv_chart_series_t>> {
        let entry = self.entry(handle)?;
        if entry.kind != Kind::Chart {
            return Err(GraphicsError::InvalidHandle);
        }
        let index = usize::try_from(slot).map_err(|_| GraphicsError::InvalidHandle)?;
        match entry.widget.as_deref() {
            Some(WidgetResource::Chart { series, .. }) => series
                .get(index)
                .copied()
                .flatten()
                .ok_or(GraphicsError::InvalidHandle),
            _ => Err(GraphicsError::InvalidHandle),
        }
    }

    pub(crate) fn replace_line_points(
        &mut self,
        handle: WasmUsize,
        points: Vec<lvgl::lv_point_precise_t>,
    ) -> Result<(
        *const lvgl::lv_point_precise_t,
        Option<Vec<lvgl::lv_point_precise_t>>,
    )> {
        let new_bytes = points
            .len()
            .checked_mul(core::mem::size_of::<lvgl::lv_point_precise_t>())
            .ok_or(GraphicsError::ResourceLimit)?;
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(GraphicsError::ResourceLimit);
        }
        let index = usize::try_from(handle)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .ok_or(GraphicsError::InvalidHandle)?;
        let entry = self
            .entries
            .get(index)
            .ok_or(GraphicsError::InvalidHandle)?;
        if entry.notification.live.get().is_null() || entry.kind != Kind::Line {
            return Err(GraphicsError::InvalidHandle);
        }
        let previous = match entry.widget.as_deref() {
            None => 0,
            Some(WidgetResource::Line { charged_bytes, .. }) => usize::from(*charged_bytes),
            Some(_) => return Err(GraphicsError::InvalidHandle),
        };
        let charge = previous.max(new_bytes);
        let used = updated_widget_options_usage(self.widget_options_bytes, previous, new_bytes)?;
        let pointer = if points.is_empty() {
            core::ptr::null()
        } else {
            points.as_ptr()
        };
        let charge = u16::try_from(charge).map_err(|_| GraphicsError::ResourceLimit)?;
        let entry = &mut self.entries[index];
        let replacement = if points.is_empty() {
            None
        } else {
            Some(points)
        };
        let resource = entry.widget.get_or_insert_with(|| {
            Box::new(WidgetResource::Line {
                points: None,
                charged_bytes: 0,
            })
        });
        let WidgetResource::Line {
            points,
            charged_bytes,
        } = &mut **resource
        else {
            return Err(GraphicsError::InvalidHandle);
        };
        let old = core::mem::replace(points, replacement);
        *charged_bytes = charge;
        self.widget_options_bytes = used;
        Ok((pointer, old))
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
