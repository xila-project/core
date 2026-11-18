use super::{ffi, icon_text::IconText};
use crate::{Color, Error, EventKind, Key, Result, event::Event, event_queue::EventQueue};
use core::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
};
use synchronization::{once_lock::OnceLock, waitqueue::AtomicWaker};

#[repr(transparent)]
pub struct ClassWrapper(ffi::lv_obj_class_t);

unsafe impl Send for ClassWrapper {}
unsafe impl Sync for ClassWrapper {}

static WINDOW_CLASS: OnceLock<ClassWrapper> = OnceLock::new();

#[unsafe(no_mangle)]
fn lvgl_rust_external_class_policy(
    class: *const ffi::lv_obj_class_t,
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

pub fn get_window_class() -> &'static ffi::lv_obj_class_t {
    let class_ref = WINDOW_CLASS.get_or_init(|| {
        let mut cls = ffi::lv_obj_class_t {
            base_class: unsafe { &ffi::lv_obj_class },
            constructor_cb: Some(window_constructor),
            destructor_cb: Some(window_destructor),
            event_cb: Some(window_event_callback),
            width_def: unsafe { ffi::lv_pct(100) },
            height_def: unsafe { ffi::lv_pct(100) },
            name: c"window".as_ptr(),
            ..Default::default()
        };

        cls.set_instance_size(size_of::<Window>() as _);
        cls.set_group_def(ffi::lv_obj_class_group_def_t_LV_OBJ_CLASS_GROUP_DEF_INHERIT as _);
        cls.set_theme_inheritable(
            ffi::lv_obj_class_theme_inheritable_t_LV_OBJ_CLASS_THEME_INHERITABLE_TRUE as _,
        );
        cls.set_editable(ffi::lv_obj_class_editable_t_LV_OBJ_CLASS_EDITABLE_INHERIT as _);

        ClassWrapper(cls)
    });

    &class_ref.0
}

#[repr(C)]
pub struct Window {
    object: ffi::lv_obj_t,
    event_queue: EventQueue,
    icon_text: IconText,
    icon_color: Color,
    waker: AtomicWaker,
}

unsafe extern "C" fn window_constructor(
    _class_p: *const ffi::lv_obj_class_t,
    object: *mut ffi::lv_obj_t,
) {
    unsafe {
        let window = object as *mut Window;

        ptr::write(&mut (*window).event_queue, EventQueue::new());
        ptr::write(&mut (*window).icon_text, IconText::new("Wi"));
        ptr::write(&mut (*window).icon_color, Color::BLACK);
        ptr::write(&mut (*window).waker, AtomicWaker::new());

        ffi::lv_obj_add_flag(
            &mut (*window).object,
            ffi::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE,
        );
        ffi::lv_obj_set_style_border_width(&mut (*window).object, 0, ffi::LV_STATE_DEFAULT);
        ffi::lv_obj_set_style_radius(&mut (*window).object, 0, ffi::LV_STATE_DEFAULT);
        crate::theme::set_surface(&mut (*window).object, crate::theme::Surface::Background);
    }
}

unsafe extern "C" fn window_destructor(
    _class_p: *const ffi::lv_obj_class_t,
    obj: *mut ffi::lv_obj_t,
) {
    unsafe {
        let window = obj as *mut Window;
        // Appelle explicitement Drop sur l'ensemble des champs Rust de Window
        core::ptr::drop_in_place(window);
    }
}

unsafe extern "C" fn window_event_callback(
    _class_p: *const ffi::lv_obj_class_t,
    event: *mut ffi::lv_event_t,
) {
    unsafe {
        let res = ffi::lv_obj_event_base(get_window_class(), event);

        if res != ffi::lv_result_t_LV_RESULT_OK {
            return; // Not our event, ignore it
        }

        let code = ffi::lv_event_get_code(event);

        let window = ffi::lv_event_get_current_target(event) as *mut Window;
        let target = ffi::lv_event_get_target(event) as *mut ffi::lv_obj_t;

        let (code, key) = match code {
            ffi::lv_event_code_t_LV_EVENT_CHILD_CREATED => {
                let child = ffi::lv_event_get_param(event) as *mut ffi::lv_obj_t;
                ffi::lv_obj_add_flag(child, ffi::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
                (code, None)
            }
            ffi::lv_event_code_t_LV_EVENT_KEY => {
                let key = ffi::lv_indev_get_key(ffi::lv_indev_active());
                let key = Key::from(key);
                (code, Some(key))
            }
            ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN
            | ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN_BEGIN
            | ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN_END
            | ffi::lv_event_code_t_LV_EVENT_DRAW_POST
            | ffi::lv_event_code_t_LV_EVENT_DRAW_POST_BEGIN
            | ffi::lv_event_code_t_LV_EVENT_GET_SELF_SIZE
            | ffi::lv_event_code_t_LV_EVENT_COVER_CHECK
            | ffi::lv_event_code_t_LV_EVENT_LAYOUT_CHANGED => {
                return;
                // Ignore draw events
            }
            _ => (code, None),
        };

        let mut queued = Event::new(EventKind::from_lvgl_code(code), target, key);
        // The destructor of a target being deleted has not freed its identifier
        // yet (the hook runs last), so the value read here was valid.
        queued.target_id = crate::object_id::read_id(target);
        if matches!(
            code,
            ffi::lv_event_code_t_LV_EVENT_PRESSED
                | ffi::lv_event_code_t_LV_EVENT_PRESSING
                | ffi::lv_event_code_t_LV_EVENT_RELEASED
        ) {
            let input = ffi::lv_event_get_indev(event);
            if !input.is_null()
                && ffi::lv_indev_get_type(input) == ffi::lv_indev_type_t_LV_INDEV_TYPE_POINTER
            {
                let mut point = ffi::lv_point_t::default();
                let mut area = ffi::lv_area_t::default();
                ffi::lv_indev_get_point(input, &mut point);
                ffi::lv_obj_get_content_coords(target, &mut area);
                queued.pointer = Some([
                    point.x - area.x1,
                    point.y - area.y1,
                    area.x2 - area.x1 + 1,
                    area.y2 - area.y1 + 1,
                ]);
            }
        }
        (*window).event_queue.push(queued);
        (*window).waker.wake();
    }
}

impl Window {
    /// Create a new window.
    ///
    /// # Arguments
    ///
    /// * `Parent_object` - The parent object of the window.
    ///
    /// # Returns
    ///
    /// * `Result<Self>` - The result of the operation.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it may dereference raw pointers (e.g. `Parent_object`).
    ///
    pub unsafe fn new(parent: *mut ffi::lv_obj_t) -> Result<NonNull<Self>> {
        let obj = unsafe { ffi::lv_obj_class_create_obj(get_window_class(), parent) };

        unsafe {
            ffi::lv_obj_class_init_obj(obj);
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

    pub fn pop_event(&mut self) -> Option<Event> {
        self.event_queue.pop_front()
    }

    /// Number of events lost to queue overflow since the window was created.
    pub fn dropped_event_count(&self) -> u32 {
        self.event_queue.dropped()
    }

    /// Forget queued targets before their LVGL storage can be reused. This only
    /// compares addresses; it never dereferences the potentially deleted target.
    pub fn discard_events_for_target(&mut self, target: *mut ffi::lv_obj_t) {
        self.event_queue.discard_target(target);
    }

    /// Preserve event codes while preventing queued addresses from acquiring a
    /// newly registered capability when native object storage is recycled.
    /// Only compares addresses; it does not dereference event targets.
    pub fn clear_event_targets_for(&mut self, target: *mut ffi::lv_obj_t) {
        self.event_queue.clear_target(target);
    }

    pub fn as_object(&self) -> &ffi::lv_obj_t {
        &self.object
    }

    pub fn as_object_mutable(&mut self) -> &mut ffi::lv_obj_t {
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
    pub unsafe fn from_raw(window: *mut ffi::lv_obj_t) -> Option<NonNull<Self>> {
        if window.is_null() {
            return None;
        }

        let class = unsafe { ffi::lv_obj_get_class(window) };

        if class != get_window_class() {
            return None;
        }

        NonNull::new(window as *mut Self)
    }

    pub fn delete(&mut self) {
        unsafe {
            ffi::lv_obj_delete(&mut self.object);
        }
    }
}

impl AsRef<ffi::lv_obj_t> for Window {
    #[inline]
    fn as_ref(&self) -> &ffi::lv_obj_t {
        &self.object
    }
}

impl AsMut<ffi::lv_obj_t> for Window {
    #[inline]
    fn as_mut(&mut self) -> &mut ffi::lv_obj_t {
        &mut self.object
    }
}

impl Deref for Window {
    type Target = ffi::lv_obj_t;

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

/// A wrapper around Window that automatically deletes the window when dropped.
pub struct OwnedWindow(NonNull<Window>);

impl OwnedWindow {
    pub fn new(window: NonNull<Window>) -> Self {
        Self(window)
    }
}

impl OwnedWindow {
    /// A shared view of the window's object, for reading the tree under the
    /// graphics lock ([`Manager::with_lvgl`](crate::Manager::with_lvgl)).
    pub fn view<'a>(&'a self, lv: &'a lvgl_rust::Lvgl) -> &'a lvgl_rust::Obj {
        // SAFETY: this value owns a live window: nothing but `Drop` deletes
        // it, and safe code cannot delete or move a Window (it is registered
        // as an externally owned class that accepts only child edits).
        unsafe { lvgl_rust::Obj::from_raw(lv, self.0.cast()) }
    }

    /// Exclusive access to the window's object, to add and edit its children
    /// with the safe API. The borrow lasts as long as the session's.
    pub fn content_mut<'a>(
        &'a mut self,
        lv: &'a mut lvgl_rust::Lvgl,
    ) -> lvgl_rust::ObjMut<'a, lvgl_rust::Obj> {
        let window = self.0.cast();
        lv.edit(|lv| {
            // SAFETY: as for `view`; nothing ran since the session was lent.
            Some(unsafe { lvgl_rust::Obj::from_raw(lv, window) })
        })
        .expect("the selector always returns the window")
    }

    /// Exclusive access to the descendant of the window that `select` picks
    /// (for example a child created earlier), without a pointer or a handle
    /// leaving the session.
    pub fn edit<'a, W: lvgl_rust::Widget>(
        &'a mut self,
        lv: &'a mut lvgl_rust::Lvgl,
        select: impl for<'b> FnOnce(&'b lvgl_rust::Obj) -> Option<&'b W>,
    ) -> Option<lvgl_rust::ObjMut<'a, W>> {
        let window = self.0.cast();
        lv.edit(|lv| {
            // SAFETY: as for `view`; nothing ran since the session was lent.
            select(unsafe { lvgl_rust::Obj::from_raw(lv, window) })
        })
    }
}

impl From<OwnedWindow> for NonNull<Window> {
    fn from(val: OwnedWindow) -> Self {
        let this = ManuallyDrop::new(val);
        this.0
    }
}

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        crate::ffi_manager()
            .lock_function_sync(|| {
                unsafe {
                    self.0.as_mut().delete();
                }
                Ok(())
            })
            .expect("Failed to delete window");
    }
}

impl Deref for OwnedWindow {
    type Target = Window;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl DerefMut for OwnedWindow {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut() }
    }
}
