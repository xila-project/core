//! The per-process object identity table.
//!
//! A caller never sees an LVGL pointer. It holds handles: the ID LVGL stores in
//! each object's own `id` field, which `lvgl_rust` allocates per process. The
//! table keeps every live handle of one process in a sorted array, so a lookup
//! is a binary search and an exited process's objects are known exactly.
//! Resources LVGL borrows from the host (button maps, grid descriptors, canvas
//! buffers) live in a side map keyed by handle and are freed with the object.

use super::{
    Word,
    canvas::{IndexedCanvas, MAX_CANVAS_PIXELS},
    error::{Error, Result},
    marshal::{
        ButtonMap, GridTracks, MAX_WIDGET_OPTIONS_BYTES_PER_VM, updated_widget_options_usage,
    },
};
use crate::{Event, Manager, OwnedWindow, Window, lvgl};
use alloc::{boxed::Box, vec::Vec};
use core::{cell::UnsafeCell, ptr::NonNull};
use lvgl_rust::{self as safe, WidgetExt};

pub const MAX_GRID_TRACKS_PER_VM: usize = 512;

// Accessed only under Manager's process-wide graphics lock. Boxes keep these
// addresses stable when State moves. The observer never calls LVGL.
struct Sides(UnsafeCell<Vec<*mut Side>>);
unsafe impl Sync for Sides {}
static SIDES: Sides = Sides(UnsafeCell::new(Vec::new()));

fn freed(event: safe::Freed) {
    let raw = event.handle().into_raw();
    // SAFETY: the graphics lock serializes Side registration, callbacks and drop.
    // The observer never calls LVGL and never mutates SIDES.
    let sides = unsafe { &*SIDES.0.get() };
    for &side_ptr in sides {
        // SAFETY: every pointer in SIDES points to a live, Box-allocated Side
        // that is unregistered before being freed.
        let side = unsafe { &mut *side_ptr };
        if let Ok(index) = side.handles.binary_search(&raw) {
            side.handles.remove(index);
            // The OBJ_ID hook runs at the END of the destructor, after LVGL has
            // finished reading maps, grid tracks and widget buffers.
            if let Some(entry) = side.entries.remove(&raw) {
                side.widget_options_bytes -= usize::from(entry.option_bytes);
                if let Some(resource) = entry.resource {
                    match &resource {
                        Resource::Grid(grid) => side.grid_tracks -= grid.track_count(),
                        Resource::Canvas(canvas) => side.canvas_pixels -= canvas.len(),
                        Resource::GridWith { grid, inner } => {
                            side.grid_tracks -= grid.track_count();
                            if let Resource::Canvas(canvas) = &**inner {
                                side.canvas_pixels -= canvas.len();
                            }
                        }
                        _ => {}
                    }
                }
            }
            break;
        }
    }
}

#[derive(Default)]
struct Side {
    handles: Vec<u32>,
    entries: Entries,
    grid_tracks: usize,
    widget_options_bytes: usize,
    canvas_pixels: usize,
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

struct ChartSeries {
    series: [Option<NonNull<lvgl::lv_chart_series_t>>; 8],
    count: u8,
}

enum Resource {
    Window(NonNull<Window>),
    Map(#[allow(dead_code)] ButtonMap),
    Grid(GridTracks),
    Canvas(IndexedCanvas),
    Calendar(#[allow(dead_code)] Vec<lvgl::lv_calendar_date_t>),
    Line(#[allow(dead_code)] Vec<lvgl::lv_point_precise_t>),
    Chart(Box<ChartSeries>),
    GridWith {
        grid: GridTracks,
        inner: Box<Resource>,
    },
}

/// Side entries by handle, sorted. Few objects carry one, so a sorted array
/// beats a tree: no node overhead, and a lookup is a binary search.
#[derive(Default)]
struct Entries(Vec<(u32, Entry)>);

impl Entries {
    fn position(&self, raw: u32) -> core::result::Result<usize, usize> {
        self.0.binary_search_by_key(&raw, |(handle, _)| *handle)
    }

    fn get(&self, raw: &u32) -> Option<&Entry> {
        Some(&self.0[self.position(*raw).ok()?].1)
    }

    fn get_mut(&mut self, raw: &u32) -> Option<&mut Entry> {
        let index = self.position(*raw).ok()?;
        Some(&mut self.0[index].1)
    }

    fn or_default(&mut self, raw: u32) -> &mut Entry {
        let index = self.position(raw).unwrap_or_else(|index| {
            self.0.insert(index, (raw, Entry::default()));
            index
        });
        &mut self.0[index].1
    }

    /// Never allocates: the free observer calls it.
    fn remove(&mut self, raw: &u32) -> Option<Entry> {
        let index = self.position(*raw).ok()?;
        Some(self.0.remove(index).1)
    }

    fn iter(&self) -> impl Iterator<Item = (&u32, &Entry)> {
        self.0.iter().map(|(handle, entry)| (handle, entry))
    }
}

#[derive(Default)]
struct Entry {
    kind: Option<Kind>,
    tile_position: Option<(u8, u8)>,
    option_bytes: u16,
    resource: Option<Resource>,
}

impl Entry {
    fn window(&self) -> Option<NonNull<Window>> {
        match self.inner_resource()? {
            Resource::Window(window) => Some(*window),
            _ => None,
        }
    }

    fn grid(&self) -> Option<&GridTracks> {
        match self.resource.as_ref()? {
            Resource::Grid(grid) | Resource::GridWith { grid, .. } => Some(grid),
            _ => None,
        }
    }

    fn set_grid(&mut self, tracks: GridTracks) {
        match self.resource.take() {
            None | Some(Resource::Grid(_)) => {
                self.resource = Some(Resource::Grid(tracks));
            }
            Some(Resource::GridWith { inner, .. }) => {
                self.resource = Some(Resource::GridWith {
                    grid: tracks,
                    inner,
                });
            }
            Some(other) => {
                self.resource = Some(Resource::GridWith {
                    grid: tracks,
                    inner: Box::new(other),
                });
            }
        }
    }

    fn inner_resource(&self) -> Option<&Resource> {
        match self.resource.as_ref()? {
            Resource::Grid(_) => None,
            Resource::GridWith { inner, .. } => Some(inner),
            other => Some(other),
        }
    }

    fn inner_resource_mut(&mut self) -> Option<&mut Resource> {
        match self.resource.as_mut()? {
            Resource::Grid(_) => None,
            Resource::GridWith { inner, .. } => Some(inner),
            other => Some(other),
        }
    }

    fn set_inner_resource(&mut self, resource: Resource) {
        match self.resource.take() {
            Some(Resource::Grid(grid)) => {
                self.resource = Some(Resource::GridWith {
                    grid,
                    inner: Box::new(resource),
                });
            }
            Some(Resource::GridWith { grid, .. }) => {
                self.resource = Some(Resource::GridWith {
                    grid,
                    inner: Box::new(resource),
                });
            }
            _ => {
                self.resource = Some(resource);
            }
        }
    }

    fn take_inner_resource(&mut self) -> Option<Resource> {
        match self.resource.take() {
            None => None,
            Some(Resource::Grid(grid)) => {
                self.resource = Some(Resource::Grid(grid));
                None
            }
            Some(Resource::GridWith { grid, inner }) => {
                self.resource = Some(Resource::Grid(grid));
                Some(*inner)
            }
            Some(other) => Some(other),
        }
    }
}

/// One process's graphics identity table and the resources behind it.
///
/// Dropping it deletes every object the process still owns, under the
/// graphics lock, so an exited or cancelled process leaves nothing behind.
pub struct State {
    pub(crate) manager: Option<&'static Manager>,
    side: Option<Box<UnsafeCell<Side>>>,
    /// Trusted native context of the calling process: the file system and task
    /// that file pickers use. The transport refreshes it before each call.
    pub(crate) file_system: Option<(
        &'static virtual_file_system::VirtualFileSystem,
        task::TaskIdentifier,
    )>,
}

impl State {
    pub fn new(manager: Option<&'static Manager>) -> Self {
        Self {
            manager,
            side: None,
            file_system: None,
        }
    }

    /// A state whose file-system context is already known.
    pub fn with_file_system(
        manager: Option<&'static Manager>,
        vfs: &'static virtual_file_system::VirtualFileSystem,
        task: task::TaskIdentifier,
    ) -> Self {
        let mut state = Self::new(manager);
        state.file_system = Some((vfs, task));
        state
    }

    /// The manager this state runs on, if any.
    pub fn manager(&self) -> Option<&'static Manager> {
        self.manager
    }

    /// The task whose identity file pickers act with: the calling process's, as of its last
    /// call. It lives only as long as the state, so a process that ends, or is cancelled,
    /// leaves no identity behind for a reused task identifier to inherit.
    pub fn file_system_task(&self) -> Option<task::TaskIdentifier> {
        self.file_system.map(|(_, task)| task)
    }

    /// Replace the file-system context of the calling process.
    pub fn set_file_system(
        &mut self,
        vfs: &'static virtual_file_system::VirtualFileSystem,
        task: task::TaskIdentifier,
    ) {
        self.file_system = Some((vfs, task));
    }

    pub(crate) fn initialize(&mut self, lv: &mut safe::Lvgl) -> Result<()> {
        if self.side.is_some() {
            return Ok(());
        }
        // The free observer is process-wide and every VM's state installs the
        // same function: `Busy` means an earlier VM already did. Install it
        // before registering this VM's side table so a failure leaves no state.
        match lv.set_free_observer(freed) {
            Ok(()) | Err(safe::Error::Busy) => {}
            Err(_) => return Err(Error::NativeFailure),
        }
        let side = Box::new(UnsafeCell::new(Side::default()));
        // SAFETY: called only inside the manager session under the graphics lock.
        let sides = unsafe { &mut *SIDES.0.get() };
        if sides.try_reserve(1).is_err() {
            return Err(Error::ResourceLimit);
        }
        sides.push(side.get());
        self.side = Some(side);
        Ok(())
    }

    pub(crate) fn ensure_capacity(&self) -> Result<()> {
        self.side.as_ref().map(|_| ()).ok_or(Error::ResourceLimit)
    }

    fn side(&self) -> &Side {
        // SAFETY: dispatcher holds the graphics lock; no borrow spans deletion.
        unsafe { &*self.side.as_ref().expect("initialized graphics").get() }
    }

    fn side_mut(&mut self) -> &mut Side {
        // SAFETY: as above. UnsafeCell permits the free observer to access the
        // stable allocation when no host resource borrow is outstanding.
        unsafe { &mut *self.side.as_ref().expect("initialized graphics").get() }
    }

    fn lv(&self) -> &safe::Lvgl {
        // SAFETY: all dispatcher helpers run inside the manager's session.
        unsafe { safe::Lvgl::assume() }
    }

    #[allow(clippy::useless_conversion)]
    fn raw(handle: Word) -> Result<u32> {
        u32::try_from(handle).map_err(|_| Error::InvalidHandle)
    }

    fn resolve_raw(&self, handle: Word) -> Result<(u32, *mut lvgl::lv_obj_t)> {
        let side = self.side.as_ref().ok_or(Error::InvalidHandle)?;
        let raw = Self::raw(handle)?;
        // SAFETY: dispatcher holds the graphics lock; no borrow spans deletion.
        if unsafe { &*side.get() }.handles.binary_search(&raw).is_err() {
            return Err(Error::InvalidHandle);
        }
        let handle = safe::Handle::<safe::Obj>::from_raw(raw).ok_or(Error::InvalidHandle)?;
        let object = handle.get(self.lv()).map_err(|_| Error::InvalidHandle)?;
        // The allocation pointer retained for a window covers its Rust
        // fields. The pointer from an LVGL view cannot access those fields.
        let pointer = self
            .side()
            .entries
            .get(&raw)
            .and_then(Entry::window)
            .map_or_else(|| object.as_ptr(), |window| window.as_ptr().cast());
        Ok((raw, pointer))
    }

    fn kind_of(&self, raw: u32, pointer: *mut lvgl::lv_obj_t) -> Result<Kind> {
        if let Some(kind) = self.side().entries.get(&raw).and_then(|e| e.kind) {
            return Ok(kind);
        }
        // Exact classes preserve distinctions such as Slider versus Bar and
        // Keyboard versus ButtonMatrix. Alias kinds live only in the side table.
        let class = unsafe { lvgl::lv_obj_get_class(pointer) };
        if core::ptr::eq(class, crate::get_window_class()) {
            return Ok(Kind::Window);
        }
        if core::ptr::eq(class, crate::get_file_picker_class()) {
            return Ok(Kind::FilePicker);
        }
        macro_rules! classes {
            ($($kind:ident => $class:ident),* $(,)?) => { $(
                if core::ptr::eq(class, &raw const lvgl::$class) { return Ok(Kind::$kind); }
            )* };
        }
        classes! {
            Object => lv_obj_class, Label => lv_label_class, Button => lv_button_class,
            ButtonMatrix => lv_buttonmatrix_class, Checkbox => lv_checkbox_class,
            TabView => lv_tabview_class, Textarea => lv_textarea_class, Keyboard => lv_keyboard_class,
            Arc => lv_arc_class, Bar => lv_bar_class, Slider => lv_slider_class,
            Switch => lv_switch_class, Spinner => lv_spinner_class, Dropdown => lv_dropdown_class,
            Roller => lv_roller_class, Led => lv_led_class, Calendar => lv_calendar_class,
            Spinbox => lv_spinbox_class, Chart => lv_chart_class, Line => lv_line_class,
            List => lv_list_class, Tileview => lv_tileview_class,
        }
        Err(Error::InvalidHandle)
    }

    pub(crate) fn object(
        &self,
        handle: Word,
        expected: Option<Kind>,
    ) -> Result<*mut lvgl::lv_obj_t> {
        let (raw, pointer) = self.resolve_raw(handle)?;
        if let Some(expected) = expected
            && self.kind_of(raw, pointer)? != expected
        {
            return Err(Error::InvalidHandle);
        }
        Ok(pointer)
    }

    pub(crate) fn object_with_kind(&self, handle: Word) -> Result<(*mut lvgl::lv_obj_t, Kind)> {
        let (raw, pointer) = self.resolve_raw(handle)?;
        Ok((pointer, self.kind_of(raw, pointer)?))
    }

    pub(crate) fn is_child_of(&self, child: Word, parent: Word, kind: Kind) -> Result<bool> {
        let child_pointer = self.object(child, Some(kind))?;
        let parent_pointer = self.object(parent, None)?;
        Ok(unsafe { lvgl::lv_obj_get_parent(child_pointer) } == parent_pointer)
    }

    pub(crate) fn tile_position_in_use(&self, view: Word, column: u8, row: u8) -> Result<bool> {
        let view_pointer = self.object(view, Some(Kind::Tileview))?;
        Ok(self.side().entries.iter().any(|(&raw, entry)| {
            entry.kind == Some(Kind::Tile)
                && entry.tile_position == Some((column, row))
                && self
                    .object(raw as Word, None)
                    .is_ok_and(|p| unsafe { lvgl::lv_obj_get_parent(p) } == view_pointer)
        }))
    }

    pub(crate) fn set_tile_position(&mut self, handle: Word, position: (u8, u8)) -> Result<()> {
        let raw = self.resolve_raw(handle)?.0;
        self.side_mut().entries.or_default(raw).tile_position = Some(position);
        Ok(())
    }

    pub(crate) fn event_handle_for(&self, root: *mut lvgl::lv_obj_t, event: &Event) -> Word {
        // Native children without handles can also appear in queued events.
        // Resolve the address against the live tree under the graphics lock.
        struct Search<'a> {
            event: &'a Event,
            found: *mut lvgl::lv_obj_t,
        }
        unsafe extern "C" fn visit(
            obj: *mut lvgl::lv_obj_t,
            data: *mut core::ffi::c_void,
        ) -> lvgl::lv_obj_tree_walk_res_t {
            let search = unsafe { &mut *data.cast::<Search<'_>>() };
            if search.event.is_target(obj) {
                search.found = obj;
                lvgl::lv_obj_tree_walk_res_t_LV_OBJ_TREE_WALK_END
            } else {
                lvgl::lv_obj_tree_walk_res_t_LV_OBJ_TREE_WALK_NEXT
            }
        }
        let mut search = Search {
            event,
            found: core::ptr::null_mut(),
        };
        unsafe { lvgl::lv_obj_tree_walk(root, Some(visit), (&raw mut search).cast()) };
        self.handle_for(search.found)
    }

    pub(crate) fn handle_for(&self, pointer: *mut lvgl::lv_obj_t) -> Word {
        let Some(non_null) = NonNull::new(pointer) else {
            return 0;
        };
        let Some(side) = self.side.as_ref() else {
            return 0;
        };
        let obj = unsafe { safe::Obj::from_raw(self.lv(), non_null) };
        obj.find_handle()
            .map(safe::Handle::into_raw)
            // SAFETY: dispatcher holds the graphics lock; no borrow spans deletion.
            .filter(|raw| unsafe { &*side.get() }.handles.binary_search(raw).is_ok())
            .map_or(0, |raw| raw as Word)
    }

    pub(crate) fn insert(
        &mut self,
        pointer: *mut lvgl::lv_obj_t,
        kind: Kind,
        _parent: Option<Word>,
    ) -> Result<Word> {
        let pointer = NonNull::new(pointer).ok_or(Error::NativeFailure)?;
        // Clear pre-registration event targets at the capability birth boundary,
        // including a recycled native address. Shared screens are never tagged.
        let mut root = pointer.as_ptr();
        while !root.is_null() {
            // SAFETY: the dispatcher holds the lock and walks live ancestors.
            if let Some(window) = unsafe { Window::from_raw(root) } {
                // SAFETY: the window is live and the lock still covers the queue.
                unsafe { Window::clear_event_targets_for(window, pointer.as_ptr()) };
                break;
            }
            root = unsafe { lvgl::lv_obj_get_parent(root) };
        }
        let Some(side) = self.side.as_mut() else {
            unsafe { lvgl::lv_obj_delete(pointer.as_ptr()) };
            return Err(Error::ResourceLimit);
        };
        // SAFETY: graphics lock held; no borrow spans LVGL deletion.
        let side = unsafe { &mut *side.get() };
        if side.handles.try_reserve(1).is_err()
            || (kind == Kind::Window && side.entries.0.try_reserve(1).is_err())
        {
            // SAFETY: the new object is live and the graphics lock is held.
            unsafe { lvgl::lv_obj_delete(pointer.as_ptr()) };
            return Err(Error::ResourceLimit);
        }
        let obj = unsafe { safe::Obj::from_raw(self.lv(), pointer) };
        let handle = match obj.handle() {
            Ok(h) => h.into_raw(),
            Err(_) => {
                unsafe { lvgl::lv_obj_delete(pointer.as_ptr()) };
                return Err(Error::ResourceLimit);
            }
        };
        let side = self.side_mut();
        if let Err(pos) = side.handles.binary_search(&handle) {
            side.handles.insert(pos, handle);
        }
        if kind == Kind::Window {
            let entry = side.entries.or_default(handle);
            entry.kind = Some(kind);
            entry.set_inner_resource(Resource::Window(pointer.cast()));
        } else if matches!(
            kind,
            Kind::IndexedCanvas
                | Kind::ListText
                | Kind::ListButton
                | Kind::Tile
                | Kind::Badge
                | Kind::Card
        ) {
            side.entries.or_default(handle).kind = Some(kind);
        }
        Ok(handle as Word)
    }

    pub(crate) fn set_map(&mut self, handle: Word, map: ButtonMap) {
        let (raw, pointer) = self.resolve_raw(handle).expect("validated matrix");
        unsafe { lvgl::lv_buttonmatrix_set_map(pointer, map.as_ptr()) };
        self.side_mut()
            .entries
            .or_default(raw)
            .set_inner_resource(Resource::Map(map));
    }

    pub(crate) fn set_grid_tracks(&mut self, handle: Word, tracks: GridTracks) -> Result<()> {
        let (raw, pointer) = self.resolve_raw(handle)?;
        let previous = self
            .side()
            .entries
            .get(&raw)
            .and_then(Entry::grid)
            .map_or(0, GridTracks::track_count);
        let used = self.side().grid_tracks - previous + tracks.track_count();
        if used > MAX_GRID_TRACKS_PER_VM {
            return Err(Error::ResourceLimit);
        }
        unsafe {
            lvgl::lv_obj_set_grid_dsc_array(pointer, tracks.columns_ptr(), tracks.rows_ptr())
        };
        self.side_mut().entries.or_default(raw).set_grid(tracks);
        self.side_mut().grid_tracks = used;
        Ok(())
    }

    pub(crate) fn validate_widget_options_size(
        &self,
        handle: Word,
        new_bytes: usize,
    ) -> Result<()> {
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(Error::ResourceLimit);
        }
        let raw = self.resolve_raw(handle)?.0;
        let previous = self
            .side()
            .entries
            .get(&raw)
            .map_or(0, |entry| usize::from(entry.option_bytes));
        updated_widget_options_usage(self.side().widget_options_bytes, previous, new_bytes)?;
        Ok(())
    }

    pub(crate) fn validate_new_widget_options_size(&self, new_bytes: usize) -> Result<()> {
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(Error::ResourceLimit);
        }
        updated_widget_options_usage(self.side().widget_options_bytes, 0, new_bytes)?;
        Ok(())
    }

    pub(crate) fn commit_widget_options_size(
        &mut self,
        handle: Word,
        new_bytes: usize,
    ) -> Result<()> {
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(Error::ResourceLimit);
        }
        let raw = self.resolve_raw(handle)?.0;
        let previous = self
            .side()
            .entries
            .get(&raw)
            .map_or(0, |entry| usize::from(entry.option_bytes));
        let charge = previous.max(new_bytes);
        let used =
            updated_widget_options_usage(self.side().widget_options_bytes, previous, new_bytes)?;
        self.side_mut().entries.or_default(raw).option_bytes = charge as u16;
        self.side_mut().widget_options_bytes = used;
        Ok(())
    }

    pub(crate) fn replace_calendar_highlights(
        &mut self,
        handle: Word,
        mut dates: Vec<lvgl::lv_calendar_date_t>,
    ) -> Result<(
        *mut lvgl::lv_calendar_date_t,
        Option<Vec<lvgl::lv_calendar_date_t>>,
    )> {
        let new_bytes = dates
            .len()
            .checked_mul(core::mem::size_of::<lvgl::lv_calendar_date_t>())
            .ok_or(Error::ResourceLimit)?;
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(Error::ResourceLimit);
        }
        let (raw, pointer) = self.resolve_raw(handle)?;
        if self.kind_of(raw, pointer)? != Kind::Calendar {
            return Err(Error::InvalidHandle);
        }
        let previous = self
            .side()
            .entries
            .get(&raw)
            .map_or(0, |entry| usize::from(entry.option_bytes));
        let charge = u16::try_from(previous.max(new_bytes)).map_err(|_| Error::ResourceLimit)?;
        let used =
            updated_widget_options_usage(self.side().widget_options_bytes, previous, new_bytes)?;
        let dates_pointer = if dates.is_empty() {
            core::ptr::null_mut()
        } else {
            dates.as_mut_ptr()
        };
        let side = self.side_mut();
        let entry = side.entries.or_default(raw);
        let old = if dates.is_empty() {
            match entry.take_inner_resource() {
                Some(Resource::Calendar(old)) => Some(old),
                Some(other) => {
                    entry.set_inner_resource(other);
                    None
                }
                None => None,
            }
        } else {
            match entry.inner_resource_mut() {
                Some(Resource::Calendar(existing)) => Some(core::mem::replace(existing, dates)),
                _ => {
                    entry.set_inner_resource(Resource::Calendar(dates));
                    None
                }
            }
        };
        entry.option_bytes = charge;
        side.widget_options_bytes = used;
        Ok((dates_pointer, old))
    }

    pub(crate) fn chart_series_count(&self, handle: Word) -> Result<usize> {
        let (raw, pointer) = self.resolve_raw(handle)?;
        if self.kind_of(raw, pointer)? != Kind::Chart {
            return Err(Error::InvalidHandle);
        }
        match self
            .side()
            .entries
            .get(&raw)
            .and_then(Entry::inner_resource)
        {
            None => Ok(0),
            Some(Resource::Chart(chart)) => Ok(usize::from(chart.count)),
            Some(_) => Err(Error::InvalidHandle),
        }
    }

    pub(crate) fn add_chart_series(
        &mut self,
        handle: Word,
        series_pointer: NonNull<lvgl::lv_chart_series_t>,
    ) -> Result<u32> {
        let (raw, pointer) = self.resolve_raw(handle)?;
        if self.kind_of(raw, pointer)? != Kind::Chart {
            return Err(Error::InvalidHandle);
        }
        let entry = self.side_mut().entries.or_default(raw);
        if entry.inner_resource().is_none() {
            entry.set_inner_resource(Resource::Chart(Box::new(ChartSeries {
                series: [None; 8],
                count: 0,
            })));
        }
        let Some(Resource::Chart(chart)) = entry.inner_resource_mut() else {
            return Err(Error::InvalidHandle);
        };
        let slot = usize::from(chart.count);
        if slot >= chart.series.len() {
            return Err(Error::ResourceLimit);
        }
        chart.series[slot] = Some(series_pointer);
        chart.count += 1;
        u32::try_from(slot).map_err(|_| Error::ResourceLimit)
    }

    pub(crate) fn chart_series(
        &self,
        handle: Word,
        slot: Word,
    ) -> Result<NonNull<lvgl::lv_chart_series_t>> {
        let (raw, pointer) = self.resolve_raw(handle)?;
        if self.kind_of(raw, pointer)? != Kind::Chart {
            return Err(Error::InvalidHandle);
        }
        let index = slot;
        match self
            .side()
            .entries
            .get(&raw)
            .and_then(Entry::inner_resource)
        {
            Some(Resource::Chart(chart)) => chart
                .series
                .get(index)
                .copied()
                .flatten()
                .ok_or(Error::InvalidHandle),
            _ => Err(Error::InvalidHandle),
        }
    }

    pub(crate) fn replace_line_points(
        &mut self,
        handle: Word,
        points: Vec<lvgl::lv_point_precise_t>,
    ) -> Result<(
        *const lvgl::lv_point_precise_t,
        Option<Vec<lvgl::lv_point_precise_t>>,
    )> {
        let new_bytes = points
            .len()
            .checked_mul(core::mem::size_of::<lvgl::lv_point_precise_t>())
            .ok_or(Error::ResourceLimit)?;
        if new_bytes > MAX_WIDGET_OPTIONS_BYTES_PER_VM {
            return Err(Error::ResourceLimit);
        }
        let (raw, pointer) = self.resolve_raw(handle)?;
        if self.kind_of(raw, pointer)? != Kind::Line {
            return Err(Error::InvalidHandle);
        }
        let previous = self
            .side()
            .entries
            .get(&raw)
            .map_or(0, |entry| usize::from(entry.option_bytes));
        let charge = u16::try_from(previous.max(new_bytes)).map_err(|_| Error::ResourceLimit)?;
        let used =
            updated_widget_options_usage(self.side().widget_options_bytes, previous, new_bytes)?;
        let points_pointer = if points.is_empty() {
            core::ptr::null()
        } else {
            points.as_ptr()
        };
        let side = self.side_mut();
        let entry = side.entries.or_default(raw);
        let old = if points.is_empty() {
            match entry.take_inner_resource() {
                Some(Resource::Line(old)) => Some(old),
                Some(other) => {
                    entry.set_inner_resource(other);
                    None
                }
                None => None,
            }
        } else {
            match entry.inner_resource_mut() {
                Some(Resource::Line(existing)) => Some(core::mem::replace(existing, points)),
                _ => {
                    entry.set_inner_resource(Resource::Line(points));
                    None
                }
            }
        };
        entry.option_bytes = charge;
        side.widget_options_bytes = used;
        Ok((points_pointer, old))
    }

    pub(crate) fn validate_canvas_pixels(&self, width: usize, height: usize) -> Result<()> {
        let length = width.checked_mul(height).ok_or(Error::InvalidValue)?;
        if width == 0 || height == 0 || length > MAX_CANVAS_PIXELS {
            return Err(Error::InvalidValue);
        }
        if self.side().canvas_pixels + length > MAX_CANVAS_PIXELS {
            return Err(Error::ResourceLimit);
        }
        Ok(())
    }

    pub(crate) fn install_canvas(&mut self, handle: Word, mut canvas: IndexedCanvas) -> Result<()> {
        let length = canvas.len();
        if self.side().canvas_pixels + length > MAX_CANVAS_PIXELS {
            return Err(Error::ResourceLimit);
        }
        let (raw, pointer) = self.resolve_raw(handle)?;
        if self.kind_of(raw, pointer)? != Kind::IndexedCanvas {
            return Err(Error::InvalidHandle);
        }
        unsafe {
            canvas.attach(pointer)?;
        }
        let side = self.side_mut();
        side.entries
            .or_default(raw)
            .set_inner_resource(Resource::Canvas(canvas));
        side.canvas_pixels += length;
        Ok(())
    }

    pub(crate) fn canvas_mut(&mut self, handle: Word) -> &mut IndexedCanvas {
        let raw = self.resolve_raw(handle).expect("live canvas").0;
        let entry = self
            .side_mut()
            .entries
            .get_mut(&raw)
            .expect("canvas entry present");
        match entry.inner_resource_mut() {
            Some(Resource::Canvas(canvas)) => canvas,
            _ => panic!("canvas kind checked"),
        }
    }

    #[doc(hidden)]
    pub fn handle_count(&self) -> usize {
        let _lock = self.manager.map(Manager::synchronous_lock);
        self.side
            .as_ref()
            // SAFETY: graphics lock held when manager is present.
            .map_or(0, |side| unsafe { &*side.get() }.handles.len())
    }

    #[doc(hidden)]
    pub fn side_entry_count(&self) -> usize {
        let _lock = self.manager.map(Manager::synchronous_lock);
        self.side
            .as_ref()
            // SAFETY: graphics lock held when manager is present.
            .map_or(0, |side| {
                unsafe { &*side.get() }
                    .entries
                    .iter()
                    .filter(|(_, entry)| {
                        // A pointer alone does not count as a retained resource.
                        !matches!(entry.resource, Some(Resource::Window(_)))
                    })
                    .count()
            })
    }

    #[doc(hidden)]
    pub fn resource_counters(&self) -> (usize, usize, usize) {
        let _lock = self.manager.map(Manager::synchronous_lock);
        self.side.as_ref().map_or((0, 0, 0), |side| {
            // SAFETY: graphics lock held when manager is present.
            let side = unsafe { &*side.get() };
            (
                side.grid_tracks,
                side.widget_options_bytes,
                side.canvas_pixels,
            )
        })
    }

    #[doc(hidden)]
    pub fn registered_state_count(manager: &Manager) -> usize {
        let _lock = manager.synchronous_lock();
        // SAFETY: graphics lock held.
        unsafe { &*SIDES.0.get() }.len()
    }
}

fn unregister_side(side: Option<Box<UnsafeCell<Side>>>) {
    let Some(side) = side else {
        return;
    };
    let target = side.get();
    // SAFETY: called only while holding the graphics lock.
    let sides = unsafe { &mut *SIDES.0.get() };
    if let Some(pos) = sides.iter().position(|&ptr| ptr == target) {
        sides.swap_remove(pos);
        if sides.is_empty() {
            *sides = Vec::new();
        }
    }
    drop(side);
}

impl State {
    /// Delete every object this process still owns, inside a session.
    fn delete_objects(&mut self, lv: &mut safe::Lvgl) {
        let handles = core::mem::take(&mut self.side_mut().handles);
        for raw in handles {
            let Some(handle) = safe::Handle::<safe::Obj>::from_raw(raw) else {
                continue;
            };
            // The host owns the guest's windows: safe structural calls
            // refuse them (Core registers them as host objects), so a
            // window goes through its owner.
            let window = self
                .side()
                .entries
                .get(&raw)
                .and_then(Entry::window)
                .filter(|_| handle.get(lv).is_ok());
            if let Some(window) = window {
                // SAFETY: this VM created the window (`WindowCreate`) and
                // took it from its `OwnedWindow`; nothing else owns or
                // deletes it.
                unsafe { OwnedWindow::from_raw(window) }.delete(lv);
                continue;
            }
            if let Ok(object) = handle.get_mut(lv) {
                let _ = object.delete();
            }
        }
        // Private window roots ensure structural deletion cannot leave a
        // retained resource attached to an untracked shared object.
        unregister_side(self.side.take());
    }

    /// Delete the process's objects under the graphics lock. Afterwards the
    /// state owns nothing and dropping it is free. It is safe to call twice.
    pub fn release_locked(&mut self, guard: &mut crate::GraphicsGuard<'_>) {
        if self.side.is_some() {
            let _ = guard.with_lvgl(|lv| self.delete_objects(lv));
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let Some(manager) = self.manager else {
            return;
        };
        if self.side.is_none() {
            return;
        }
        let _ = manager.with_lvgl_sync(|lv| {
            self.delete_objects(lv);
            Ok(())
        });
        if let Some(side) = self.side.take() {
            // No session (re-entrant drop or LVGL gone): the VM's objects may
            // still be alive and LVGL may still read their maps, grid tracks or
            // canvas buffers when it deletes them. Stop observing, and leak the
            // side table rather than free memory LVGL can still reach.
            let _lock = manager.synchronous_lock();
            let target = side.get();
            // SAFETY: graphics lock held.
            let sides = unsafe { &mut *SIDES.0.get() };
            if let Some(position) = sides.iter().position(|&pointer| pointer == target) {
                sides.swap_remove(position);
            }
            Box::leak(side);
        }
    }
}

#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(core::mem::size_of::<u32>() == 4);
    assert!(core::mem::size_of::<Entry>() == 24);
    // A row of the side table: four fit in its first allocation, where a tree
    // node for the same entries would hold eleven slots, 316 bytes.
    assert!(core::mem::size_of::<(u32, Entry)>() == 28);
    assert!(core::mem::size_of::<Side>() == 36);
    assert!(core::mem::size_of::<Sides>() == 12);
    assert!(core::mem::size_of::<State>() == 16);
    assert!(core::mem::size_of::<ChartSeries>() == 36);
};

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn side_entries_stay_sorted_and_find_what_they_hold() {
        let mut entries = Entries::default();
        for handle in [40, 7, 900, 12, 7] {
            entries.or_default(handle).option_bytes += 1;
        }
        assert_eq!(entries.0.len(), 4);
        assert_eq!(
            entries
                .iter()
                .map(|(&handle, _)| handle)
                .collect::<Vec<_>>(),
            [7, 12, 40, 900]
        );
        // Asking again returns the same entry, not a new one.
        assert_eq!(entries.get(&7).unwrap().option_bytes, 2);
        assert!(entries.get(&8).is_none());
        entries.get_mut(&40).unwrap().option_bytes = 9;
        assert_eq!(entries.remove(&40).map(|entry| entry.option_bytes), Some(9));
        assert!(entries.remove(&40).is_none());
        assert_eq!(entries.0.len(), 3);
    }

    #[test]
    fn host_type_sizes_match_expected_layouts() {
        assert_eq!(size_of::<u32>(), 4);
        assert_eq!(size_of::<lvgl::lv_calendar_date_t>(), 4);
        assert_eq!(size_of::<lvgl::lv_point_precise_t>(), 8);
        assert_eq!(size_of::<task::TaskIdentifier>(), 4);
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(size_of::<Entry>(), 40);
            assert_eq!(size_of::<Side>(), 72);
            assert_eq!(size_of::<Sides>(), 24);
            assert_eq!(size_of::<State>(), 32);
        }
    }
}
