use super::{icon_text::IconText, lvgl};
use crate::{Color, Error, EventKind, Key, Result, event::Event};
use alloc::collections::VecDeque;
use core::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
};
use synchronization::{once_lock::OnceLock, waitqueue::AtomicWaker};

const WINDOW_QUEUE_DEFAULT_CAPACITY: usize = 10;

#[repr(transparent)]
pub struct ClassWrapper(lvgl::lv_obj_class_t);

unsafe impl Send for ClassWrapper {}
unsafe impl Sync for ClassWrapper {}

static WINDOW_CLASS: OnceLock<ClassWrapper> = OnceLock::new();

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

#[repr(C)]
pub struct Window {
    object: lvgl::lv_obj_t,
    event_queue: VecDeque<Event>,
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

        ptr::write(
            &mut (*window).event_queue,
            VecDeque::with_capacity(WINDOW_QUEUE_DEFAULT_CAPACITY),
        );
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
            lvgl::lv_event_code_t_LV_EVENT_CHILD_CREATED => {
                let child = lvgl::lv_event_get_param(event) as *mut lvgl::lv_obj_t;
                lvgl::lv_obj_add_flag(child, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
                (code, None)
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
            | lvgl::lv_event_code_t_LV_EVENT_GET_SELF_SIZE
            | lvgl::lv_event_code_t_LV_EVENT_COVER_CHECK
            | lvgl::lv_event_code_t_LV_EVENT_LAYOUT_CHANGED => {
                return;
                // Ignore draw events
            }
            _ => (code, None),
        };

        let mut queued = Event::new(EventKind::from_lvgl_code(code), target, key);
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
        (*window).event_queue.push_back(queued);
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

    pub fn pop_event(&mut self) -> Option<Event> {
        self.event_queue.pop_front()
    }

    /// Forget queued targets before their LVGL storage can be reused. This only
    /// compares addresses; it never dereferences the potentially deleted target.
    pub fn discard_events_for_target(&mut self, target: *mut lvgl::lv_obj_t) {
        self.event_queue.retain(|event| event.target != target);
    }

    /// Preserve event codes while preventing queued addresses from acquiring a
    /// newly registered capability when native object storage is recycled.
    /// Only compares addresses; it does not dereference event targets.
    pub fn clear_event_targets_for(&mut self, target: *mut lvgl::lv_obj_t) {
        for event in &mut self.event_queue {
            if event.target == target {
                event.target = ptr::null_mut();
            }
        }
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

    pub fn delete(&mut self) {
        unsafe {
            lvgl::lv_obj_delete(&mut self.object);
        }
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

/// A wrapper around Window that automatically deletes the window when dropped.
pub struct OwnedWindow(NonNull<Window>);

impl OwnedWindow {
    pub fn new(window: NonNull<Window>) -> Self {
        Self(window)
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
