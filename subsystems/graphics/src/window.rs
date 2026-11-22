use super::{icon_text::IconText, lvgl};
use crate::{
    Color, Error, EventKind, Key, Result,
    event::Event,
    event_queue::{self, EventQueue},
};
use core::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
};
use synchronization::{once_lock::OnceLock, waitqueue::AtomicWaker};

#[repr(transparent)]
pub struct ClassWrapper(lvgl::lv_obj_class_t);

unsafe impl Send for ClassWrapper {}
unsafe impl Sync for ClassWrapper {}

static WINDOW_CLASS: OnceLock<ClassWrapper> = OnceLock::new();

#[unsafe(no_mangle)]
fn lvgl_rust_external_class_policy(
    class: *const lvgl::lv_obj_class_t,
) -> lvgl_rust::ExternalClassPolicy {
    // The descriptor is initialized under the graphics lock before use. A
    // policy lookup may only read it; initializing it calls LVGL.
    if WINDOW_CLASS
        .try_get()
        .is_some_and(|window| core::ptr::eq(&window.0, class))
    {
        lvgl_rust::ExternalClassPolicy::ChildrenOnly
    } else {
        lvgl_rust::ExternalClassPolicy::Protected
    }
}

pub fn get_window_class() -> &'static lvgl::lv_obj_class_t {
    let class_ref = WINDOW_CLASS.get_or_init(|| {
        let mut cls = lvgl::lv_obj_class_t {
            base_class: unsafe { &lvgl::lv_obj_class },
            constructor_cb: Some(window_constructor),
            destructor_cb: Some(window_destructor),
            event_cb: Some(window_event_callback),
            width_def: unsafe { lvgl::lv_pct(100) },
            height_def: unsafe { lvgl::lv_pct(100) },
            name: c"window".as_ptr(),
            ..Default::default()
        };

        cls.set_instance_size(size_of::<Window>() as _);
        cls.set_group_def(lvgl::lv_obj_class_group_def_t_LV_OBJ_CLASS_GROUP_DEF_INHERIT as _);
        cls.set_theme_inheritable(
            lvgl::lv_obj_class_theme_inheritable_t_LV_OBJ_CLASS_THEME_INHERITABLE_TRUE as _,
        );
        cls.set_editable(lvgl::lv_obj_class_editable_t_LV_OBJ_CLASS_EDITABLE_INHERIT as _);

        ClassWrapper(cls)
    });

    &class_ref.0
}

/// An LVGL object that queues the events of its widgets for one consumer.
///
/// The queue is a fixed ring of [`crate::WINDOW_EVENT_QUEUE_CAPACITY`]
/// events inside the window: it never allocates. A newer `Pressing`,
/// `LongPressedRepeat`, `ValueChanged`, `SizeChanged` or `StyleChanged`
/// replaces the queued one for the same target, whichever input device sent
/// them. When the queue is full, an arrival evicts the oldest scroll or other
/// notification, then the oldest `Pressing` or `LongPressedRepeat`, then the
/// oldest value, size or style change, but never an event more important than
/// itself. Other events (presses, releases, clicks, keys, focus changes,
/// `Ready`, `Cancel`, `Delete`, `CloseRequested`...) are never evicted or
/// reordered; one that finds no room is rejected. A newer event never
/// replaces a queued one across such an event of the same target: a value
/// change queued before a `Released` or a `Key` of its widget stays, and the
/// newer one is queued after it. A press is only queued with room for the
/// rest of it, reserved per input device, and the rest of a rejected press is
/// dropped: the queue never drops a `Pressed` event while keeping the end of
/// its press. LVGL itself may end a press on a widget that never got
/// `Pressed`: after a drag onto another widget without press lock, or when a
/// keypad is released on a newly focused widget.
/// Losses are counted ([`Window::dropped_event_count`]) and logged at most
/// once per second. The crate README ("Window events") details the policy.
#[repr(C)]
pub struct Window {
    object: lvgl::lv_obj_t,
    event_queue: EventQueue,
    icon_text: IconText,
    icon_color: Color,
    waker: AtomicWaker,
}

unsafe extern "C" fn window_constructor(
    _class_p: *const lvgl::lv_obj_class_t,
    object: *mut lvgl::lv_obj_t,
) {
    unsafe {
        let window = object as *mut Window;

        ptr::write(&mut (*window).event_queue, EventQueue::new());
        ptr::write(&mut (*window).icon_text, IconText::new("Wi"));
        ptr::write(&mut (*window).icon_color, Color::BLACK);
        ptr::write(&mut (*window).waker, AtomicWaker::new());

        lvgl::lv_obj_add_flag(
            &mut (*window).object,
            lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE,
        );
        lvgl::lv_obj_set_style_border_width(&mut (*window).object, 0, lvgl::LV_STATE_DEFAULT);
        lvgl::lv_obj_set_style_radius(&mut (*window).object, 0, lvgl::LV_STATE_DEFAULT);
        crate::theme::set_surface(&mut (*window).object, crate::theme::Surface::Background);
    }
}

unsafe extern "C" fn window_destructor(
    _class_p: *const lvgl::lv_obj_class_t,
    obj: *mut lvgl::lv_obj_t,
) {
    unsafe {
        let window = obj as *mut Window;
        // Appelle explicitement Drop sur l'ensemble des champs Rust de Window
        core::ptr::drop_in_place(window);
    }
}

/// Whether `target` is `root` or one of its descendants. Compares addresses
/// with live objects only: `target` itself is never dereferenced.
unsafe fn is_in_tree(root: *mut lvgl::lv_obj_t, target: *mut lvgl::lv_obj_t) -> bool {
    unsafe {
        root == target
            || (0..lvgl::lv_obj_get_child_count(root) as i32)
                .any(|index| is_in_tree(lvgl::lv_obj_get_child(root, index), target))
    }
}

/// Null the target of every queued event whose widget is no longer below the
/// window, and forget the presses of such widgets. Costs nothing per object
/// and allocates nothing; the walk runs only when a descendant goes away while
/// an event is queued or a press is followed.
unsafe fn forget_dead_targets(window: *mut Window) {
    unsafe {
        let queue = &mut (*window).event_queue;
        if queue.is_empty() && !queue.follows_presses() {
            return;
        }
        let root = window as *mut lvgl::lv_obj_t;
        queue.clear_dead_targets(|target| is_in_tree(root, target));
    }
}

/// Whether LVGL may still send events for the current press of `device`: it
/// is being read right now, or it still exists and is pressed. Compares
/// addresses with live devices only: `device` itself is never dereferenced,
/// since it may have been deleted.
///
/// LVGL reads one input device at a time and sends every event of a read
/// before it returns, so a device that is released and not being read has
/// sent the end of its press, if any. Its press is over.
unsafe fn may_continue_press(device: event_queue::InputDevice) -> bool {
    unsafe {
        if ptr::eq(lvgl::lv_indev_active(), device) {
            return true;
        }
        let mut input = lvgl::lv_indev_get_next(ptr::null_mut());
        while !input.is_null() {
            if ptr::eq(input, device) {
                return lvgl::lv_indev_get_state(input)
                    == lvgl::lv_indev_state_t_LV_INDEV_STATE_PRESSED;
            }
            input = lvgl::lv_indev_get_next(input);
        }
        false
    }
}

/// Forget the presses LVGL will not end, to give their reservations back: a
/// device released while the press belonged to another window (a keypad
/// whose focus moved away), or deleted mid-press. Call it only with the
/// graphics lock held.
fn forget_ended_presses(queue: &mut EventQueue) {
    if queue.follows_presses() {
        queue.forget_ended_presses(|device| unsafe { may_continue_press(device) });
    }
}

/// Log the events lost since the last report, at most once per
/// [`crate::event_queue::LOSS_REPORT_INTERVAL`].
fn report_losses(queue: &mut EventQueue) {
    let now = unsafe { lvgl::lv_tick_get() };
    if let Some(report) = queue.take_loss_report(now) {
        log::warning!(
            "Window event queue full: dropped {} events ({} since the window was created)",
            report.lost,
            report.total
        );
    }
}

unsafe extern "C" fn window_event_callback(
    _class_p: *const lvgl::lv_obj_class_t,
    event: *mut lvgl::lv_event_t,
) {
    unsafe {
        let res = lvgl::lv_obj_event_base(get_window_class(), event);

        if res != lvgl::lv_result_t_LV_RESULT_OK {
            return; // Not our event, ignore it
        }

        let code = lvgl::lv_event_get_code(event);

        let window = lvgl::lv_event_get_current_target(event) as *mut Window;
        let target = lvgl::lv_event_get_target(event) as *mut lvgl::lv_obj_t;

        let (code, key) = match code {
            // The window handles tree changes itself and never queues them:
            // consumers ignore them, and building a page emits several per
            // widget, which would fill the bounded ring and push out clicks.
            lvgl::lv_event_code_t_LV_EVENT_CHILD_CHANGED => return,
            lvgl::lv_event_code_t_LV_EVENT_CHILD_CREATED => {
                let child = lvgl::lv_event_get_param(event) as *mut lvgl::lv_obj_t;
                lvgl::lv_obj_add_flag(child, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
                return;
            }
            lvgl::lv_event_code_t_LV_EVENT_CHILD_DELETED => {
                // A descendant was deleted or moved out. LVGL sends this to
                // the direct parent after the object is freed, and it reaches
                // the window by bubbling. Queued events aimed at it would
                // otherwise match a widget created later at the same address.
                // Remaining gap: user code that runs between the free and
                // this event (a raw CHILD_CHANGED or scroll handler during a
                // clean, a parent destructor, or a handler that stops
                // bubbling) could create a widget at the freed address before
                // the sweep, and that widget would then match a stale event.
                forget_dead_targets(window);
                return;
            }
            // The shared window queue has no subscription for hover events.
            lvgl::lv_event_code_t_LV_EVENT_HOVER_OVER
            | lvgl::lv_event_code_t_LV_EVENT_HOVER_LEAVE => {
                return;
            }
            lvgl::lv_event_code_t_LV_EVENT_KEY => {
                let key = lvgl::lv_indev_get_key(lvgl::lv_indev_active());
                let key = Key::from(key);
                (code, Some(key))
            }
            lvgl::lv_event_code_t_LV_EVENT_DRAW_MAIN
            | lvgl::lv_event_code_t_LV_EVENT_DRAW_MAIN_BEGIN
            | lvgl::lv_event_code_t_LV_EVENT_DRAW_MAIN_END
            | lvgl::lv_event_code_t_LV_EVENT_DRAW_POST
            | lvgl::lv_event_code_t_LV_EVENT_DRAW_POST_BEGIN
            | lvgl::lv_event_code_t_LV_EVENT_DRAW_POST_END
            | lvgl::lv_event_code_t_LV_EVENT_DRAW_TASK_ADDED
            | lvgl::lv_event_code_t_LV_EVENT_REFR_EXT_DRAW_SIZE
            | lvgl::lv_event_code_t_LV_EVENT_GET_SELF_SIZE
            | lvgl::lv_event_code_t_LV_EVENT_COVER_CHECK
            | lvgl::lv_event_code_t_LV_EVENT_HIT_TEST
            | lvgl::lv_event_code_t_LV_EVENT_INVALIDATE_AREA
            | lvgl::lv_event_code_t_LV_EVENT_LAYOUT_CHANGED => {
                // Drawing, hit testing and layout: their parameters only live
                // during the dispatch, and no consumer handles them.
                return;
            }
            _ => (code, None),
        };

        let kind = EventKind::from_lvgl_code(code);
        // Codes without a kind (state changes, checked/unchecked) would reach
        // consumers as an opaque `Last` while taking a slot in the bounded ring.
        if kind == EventKind::Last {
            return;
        }

        let mut queued = Event::new(kind, target, key);
        if matches!(
            code,
            lvgl::lv_event_code_t_LV_EVENT_PRESSED
                | lvgl::lv_event_code_t_LV_EVENT_PRESSING
                | lvgl::lv_event_code_t_LV_EVENT_RELEASED
        ) {
            let input = lvgl::lv_event_get_indev(event);
            if !input.is_null()
                && lvgl::lv_indev_get_type(input) == lvgl::lv_indev_type_t_LV_INDEV_TYPE_POINTER
            {
                let mut point = lvgl::lv_point_t::default();
                let mut area = lvgl::lv_area_t::default();
                lvgl::lv_indev_get_point(input, &mut point);
                lvgl::lv_obj_get_content_coords(target, &mut area);
                queued.pointer = Some([
                    point.x - area.x1,
                    point.y - area.y1,
                    area.x2 - area.x1 + 1,
                    area.y2 - area.y1 + 1,
                ]);
            }
        }
        // LVGL passes the input device as the parameter of every press event
        // and of a device reset.
        let device = if event_queue::carries_input_device(kind) {
            lvgl::lv_event_get_param(event) as event_queue::InputDevice
        } else {
            ptr::null()
        };
        if kind == EventKind::Pressed {
            // A new press needs room: take back what ended presses hold.
            forget_ended_presses(&mut (*window).event_queue);
        }
        if (*window).event_queue.push(queued, device).lost_an_event() {
            report_losses(&mut (*window).event_queue);
        }
        (*window).waker.wake();
    }
}

impl Window {
    /// Create a new window.
    ///
    /// The caller owns the new window: give it to [`OwnedWindow::from_raw`],
    /// or delete it itself with `lv_obj_delete`, exactly once.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent object of the window.
    ///
    /// # Safety
    ///
    /// `parent` must be a live LVGL object, and the caller must hold the
    /// graphics lock.
    pub unsafe fn new(parent: *mut lvgl::lv_obj_t) -> Result<NonNull<Self>> {
        let obj = unsafe { lvgl::lv_obj_class_create_obj(get_window_class(), parent) };

        unsafe {
            lvgl::lv_obj_class_init_obj(obj);
        }

        match NonNull::new(obj as *mut Self) {
            Some(window) => Ok(window),
            None => Err(Error::FailedToCreateObject),
        }
    }

    pub fn get_identifier(&self) -> usize {
        self as *const Self as usize
    }

    pub fn peek_event(&self) -> Option<Event> {
        self.event_queue.front().cloned()
    }

    /// Take the oldest queued event. Events arrive in order, except that a
    /// newer "latest wins" event (`Pressing`, `LongPressedRepeat`,
    /// `ValueChanged`, `SizeChanged`, `StyleChanged`) replaces the queued one
    /// for the same target and takes its place at the back; see
    /// [`Window`] for what is dropped when the queue is full.
    ///
    /// Call it with the graphics lock held: it also forgets the presses of
    /// input devices that LVGL reports as released or deleted.
    pub fn pop_event(&mut self) -> Option<Event> {
        if self.event_queue.has_unreported_losses() {
            report_losses(&mut self.event_queue);
        }
        forget_ended_presses(&mut self.event_queue);
        self.event_queue.pop_front()
    }

    /// Number of events lost to queue overflow since the window was created.
    pub fn dropped_event_count(&self) -> u32 {
        self.event_queue.dropped()
    }

    /// Forget queued targets before their LVGL storage can be reused. This only
    /// compares addresses; it never dereferences the potentially deleted target.
    /// Press events of the target, except moves and repeats, are kept with a
    /// null target, so that the end of a press never arrives without its
    /// `Pressed` event.
    ///
    /// Not needed for widgets below the window that LVGL deletes or moves
    /// away: the window clears those targets itself when LVGL reports the
    /// deletion.
    pub fn discard_events_for_target(&mut self, target: *mut lvgl::lv_obj_t) {
        self.event_queue.discard_target(target);
    }

    /// Preserve event codes while preventing queued addresses from acquiring a
    /// newly registered capability when native object storage is recycled.
    /// Only compares addresses; it does not dereference event targets.
    pub fn clear_event_targets_for(&mut self, target: *mut lvgl::lv_obj_t) {
        self.event_queue.clear_target(target);
    }

    pub fn as_object(&self) -> &lvgl::lv_obj_t {
        &self.object
    }

    pub fn as_object_mutable(&mut self) -> &mut lvgl::lv_obj_t {
        &mut self.object
    }

    pub fn get_icon(&self) -> (&str, Color) {
        (self.icon_text.as_str(), self.icon_color)
    }

    /// Set the legacy icon using up to two Unicode scalar values.
    /// Text after a NUL is ignored; shorter strings replace the previous text.
    pub fn set_icon(&mut self, icon_string: &str, icon_color: Color) {
        self.icon_text = IconText::new(icon_string);
        self.icon_color = icon_color;
    }

    pub fn wake_up(&mut self) {
        self.waker.wake();
    }

    pub fn register_waker(&mut self, waker: &core::task::Waker) {
        self.waker.register(waker);
    }

    /// Convert a raw pointer to a window object.
    ///
    /// # Returns
    ///
    /// * `Window` - The raw pointer to the window.
    ///
    /// # Safety
    ///
    /// `window` must be null or point to a live LVGL object.
    ///
    pub unsafe fn from_raw(window: *mut lvgl::lv_obj_t) -> Option<NonNull<Self>> {
        if window.is_null() {
            return None;
        }

        let class = unsafe { lvgl::lv_obj_get_class(window) };

        if class != get_window_class() {
            return None;
        }

        NonNull::new(window as *mut Self)
    }
}

impl AsRef<lvgl::lv_obj_t> for Window {
    #[inline]
    fn as_ref(&self) -> &lvgl::lv_obj_t {
        &self.object
    }
}

impl AsMut<lvgl::lv_obj_t> for Window {
    #[inline]
    fn as_mut(&mut self) -> &mut lvgl::lv_obj_t {
        &mut self.object
    }
}

impl Deref for Window {
    type Target = lvgl::lv_obj_t;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.object
    }
}

impl DerefMut for Window {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object
    }
}

/// The single owner of a [`Window`]: it deletes the window when dropped.
///
/// Nothing else deletes an owned window. The safe layer refuses to delete,
/// clean away, own, reparent or reorder a window or an object containing one
/// (see `lvgl_rust_external_class_policy`), raw code needs `unsafe`, and
/// [`Window`] has no public delete. A window therefore lives exactly as long
/// as its owner, which deletes it once.
///
/// Safe code gets shared access through [`Deref`] and the mutating methods
/// below, never `&mut Window`: with it, [`core::mem::swap`] could exchange
/// two windows' LVGL objects behind LVGL's back.
///
/// Taking ownership of a raw window requires `unsafe`:
///
/// ```compile_fail,E0133
/// fn adopt(window: core::ptr::NonNull<graphics::Window>) -> graphics::OwnedWindow {
///     graphics::OwnedWindow::from_raw(window)
/// }
/// ```
///
/// Safe code cannot borrow the window mutably:
///
/// ```compile_fail,E0596
/// fn exchange(first: &mut graphics::OwnedWindow, second: &mut graphics::OwnedWindow) {
///     core::mem::swap::<graphics::Window>(&mut *first, &mut *second);
/// }
/// ```
///
/// Nor delete it behind its owner's back:
///
/// ```compile_fail,E0599
/// fn delete(window: &graphics::Window) {
///     window.delete();
/// }
/// ```
pub struct OwnedWindow(NonNull<Window>);

impl OwnedWindow {
    /// Take ownership of a raw window.
    ///
    /// # Safety
    ///
    /// `window` must point to a live window (see [`Window::new`]) that
    /// nothing else owns or deletes: the result deletes it when dropped. Every
    /// window created inside it must be deleted first.
    pub unsafe fn from_raw(window: NonNull<Window>) -> Self {
        Self(window)
    }

    /// Give up ownership without deleting the window. The caller becomes its
    /// owner: it deletes the window once, under the graphics lock, or hands it
    /// back to [`OwnedWindow::from_raw`].
    pub fn into_raw(self) -> NonNull<Window> {
        ManuallyDrop::new(self).0
    }

    /// Delete the window now, from a session of the safe API, where dropping
    /// it would deadlock on the graphics lock the session already holds.
    pub fn delete(self, _lvgl: &mut lvgl_rust::Lvgl) {
        let window = self.into_raw();
        // SAFETY: `&mut Lvgl` proves exclusive access to LVGL (the graphics
        // lock), and this value owned the window.
        unsafe { delete(window) };
    }

    /// The window's LVGL object, for the raw bindings.
    pub fn as_object_mutable(&mut self) -> *mut lvgl::lv_obj_t {
        self.0.as_ptr().cast()
    }

    /// See [`Window::pop_event`].
    pub fn pop_event(&mut self) -> Option<Event> {
        self.window_mut().pop_event()
    }

    /// See [`Window::discard_events_for_target`].
    pub fn discard_events_for_target(&mut self, target: *mut lvgl::lv_obj_t) {
        self.window_mut().discard_events_for_target(target);
    }

    /// See [`Window::clear_event_targets_for`].
    pub fn clear_event_targets_for(&mut self, target: *mut lvgl::lv_obj_t) {
        self.window_mut().clear_event_targets_for(target);
    }

    /// See [`Window::set_icon`].
    pub fn set_icon(&mut self, icon_string: &str, icon_color: Color) {
        self.window_mut().set_icon(icon_string, icon_color);
    }

    pub fn wake_up(&mut self) {
        self.window_mut().wake_up();
    }

    pub fn register_waker(&mut self, waker: &core::task::Waker) {
        self.window_mut().register_waker(waker);
    }

    /// Private: callers outside this module could swap the window away.
    fn window_mut(&mut self) -> &mut Window {
        // SAFETY: this value owns the live window, and `&mut self` makes the
        // borrow unique.
        unsafe { self.0.as_mut() }
    }
}

impl From<OwnedWindow> for NonNull<Window> {
    fn from(window: OwnedWindow) -> Self {
        window.into_raw()
    }
}

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        let window = self.0;
        crate::ffi_manager()
            .lock_function_sync(|| {
                // SAFETY: the graphics lock is held, and this value owns the
                // window.
                unsafe { delete(window) };
                Ok(())
            })
            .expect("Failed to delete window");
    }
}

impl Deref for OwnedWindow {
    type Target = Window;

    #[inline]
    fn deref(&self) -> &Self::Target {
        // SAFETY: this value owns the live window.
        unsafe { self.0.as_ref() }
    }
}

/// Delete `window` and everything it contains.
///
/// # Safety
///
/// The caller holds the graphics lock and owns `window`, which it does not
/// use afterwards.
unsafe fn delete(window: NonNull<Window>) {
    unsafe { lvgl::lv_obj_delete(window.as_ptr().cast()) };
}
