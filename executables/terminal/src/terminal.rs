use crate::error::{Error, Result};
use alloc::{collections::vec_deque::VecDeque, string::String};
use core::ffi::CStr;
use xila::graphics::OwnedWindow;
use xila::internationalization::translate;
use xila::graphics::fonts::get_font_monospace_medium;
use xila::graphics::{
    self, Color, EventKind, Key,
    lvgl::{self, lv_obj_t},
};
use xila::synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};

pub(crate) struct Inner {
    window: OwnedWindow,
    buffer: String,
    display: *mut lvgl::lv_obj_t,
    input: *mut lvgl::lv_obj_t,
    validated_input: VecDeque<u8>,
}

pub struct Terminal {
    inner: RwLock<CriticalSectionRawMutex, Inner>,
    graphics_manager: &'static graphics::Manager,
}

unsafe impl Send for Terminal {}

unsafe impl Sync for Terminal {}

impl Terminal {
    const CLEAR: &'static str = "\x1B[2J";
    const HOME: &'static str = "\x1B[H";

    pub async fn new(graphics_manager: &'static graphics::Manager) -> Result<Self> {
        let inner = graphics::lock!(graphics_manager, {
            let mut window = graphics_manager.create_window().await?;
            unsafe {
                window.set_icon(">_", Color::BLACK);

                lvgl::lv_obj_set_flex_flow(
                    window.as_object_mutable(),
                    lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN,
                );
                // Edge-to-edge window chrome: the shared window class already
                // flattens border and radius, only the container padding remains.
                lvgl::lv_obj_set_style_pad_all(
                    window.as_object_mutable(),
                    0,
                    lvgl::LV_STATE_DEFAULT,
                );
            }

            let container = unsafe {
                let container = lvgl::lv_obj_create(window.as_object_mutable());

                lvgl::lv_obj_set_width(container, lvgl::lv_pct(100));
                lvgl::lv_obj_set_flex_grow(container, 1);
                lvgl::lv_obj_set_flex_flow(container, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
                // Flat container chrome: keep the theme gap (space_md) between
                // the display and the input, but drop the card padding, border
                // and radius around the output.
                lvgl::lv_obj_set_style_pad_all(container, 0, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_border_width(container, 0, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_radius(container, 0, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_scroll_snap_y(
                    container,
                    lvgl::lv_scroll_snap_t_LV_SCROLL_SNAP_END,
                );

                container
            };

            let mut buffer = String::with_capacity(80 * 24);
            buffer.push('\0'); // Initialize with null terminator for LVGL

            let display = unsafe {
                let label = lvgl::lv_label_create(container);

                if label.is_null() {
                    return Err(crate::error::Error::FailedToCreateObject);
                }

                lvgl::lv_obj_set_width(label, lvgl::lv_pct(100));
                lvgl::lv_label_set_text_static(label, buffer.as_ptr() as *const core::ffi::c_char);
                lvgl::lv_obj_set_style_text_font(
                    label,
                    get_font_monospace_medium(),
                    lvgl::LV_STATE_DEFAULT,
                );

                label
            };

            let input = unsafe {
                let input = lvgl::lv_textarea_create(window.as_object_mutable());

                if input.is_null() {
                    return Err(crate::error::Error::FailedToCreateObject);
                }

                lvgl::lv_textarea_set_placeholder_text(
                    input,
                    translate!(c"Enter your command ...").as_ptr(),
                );
                lvgl::lv_obj_set_width(input, lvgl::lv_pct(100));
                lvgl::lv_textarea_set_one_line(input, true);

                input
            };

            Inner {
                window,
                buffer,
                display,
                input,
                validated_input: VecDeque::with_capacity(128),
            }
        });

        Ok(Self {
            inner: RwLock::new(inner),
            graphics_manager,
        })
    }

    pub fn print(&self, text: &str) -> Result<()> {
        let mut inner = self.inner.try_write().map_err(|_| Error::RessourceBusy)?;
        let _lock = self
            .graphics_manager
            .try_lock()
            .ok_or(Error::RessourceBusy)?;

        Self::print_internal(&mut inner, text)?;

        Ok(())
    }

    fn get_start_index(buffer: &mut String, text: &str) {
        let start_index = if let Some(last_clear) = text.rfind(Self::CLEAR) {
            buffer.clear();
            last_clear + Self::CLEAR.len()
        } else {
            0
        };

        let start_index = if let Some(last_home) = text.rfind(Self::HOME) {
            start_index.max(last_home + Self::HOME.len())
        } else {
            start_index
        };

        let text = &text[start_index..];

        buffer.push_str(text);
    }

    fn print_internal(inner: &mut Inner, text: &str) -> Result<()> {
        inner.buffer.pop(); // Remove the trailing null character

        Self::get_start_index(&mut inner.buffer, text);
        inner.buffer.push('\0');

        unsafe {
            lvgl::lv_label_set_text_static(
                inner.display,
                inner.buffer.as_ptr() as *const core::ffi::c_char,
            );
            lvgl::lv_obj_scroll_to_view(inner.display, true);
        }

        Ok(())
    }

    async fn print_line_internal(inner: &mut Inner, text: &str) -> Result<()> {
        inner.buffer.pop(); // Remove the trailing null character

        Self::get_start_index(&mut inner.buffer, text);
        inner.buffer.push_str("\n\0");

        unsafe {
            lvgl::lv_label_set_text_static(
                inner.display,
                inner.buffer.as_ptr() as *const core::ffi::c_char,
            );
            lvgl::lv_obj_scroll_to_view(inner.display, true);
        }

        Ok(())
    }

    pub fn read_input(&self, buffer: &mut [u8]) -> Result<usize> {
        let mut inner = self.inner.try_write().map_err(|_| Error::RessourceBusy)?;

        let mut read = 0;

        buffer
            .iter_mut()
            .zip(inner.validated_input.iter())
            .for_each(|(byte, &char)| {
                *byte = char;
                read += 1;
            });

        inner.validated_input.drain(0..read);

        Ok(read)
    }

    fn get_input(text_area: *mut lv_obj_t) -> &'static str {
        unsafe {
            let text = lvgl::lv_textarea_get_text(text_area);

            CStr::from_ptr(text).to_str().unwrap_or("")
        }
    }

    pub async fn handle_events(&self) -> Result<bool> {
        let mut running = true;

        graphics::lock!(self.graphics_manager, {
            let mut inner = self.inner.write().await;

            while let Some(event) = inner.window.pop_event() {
                match event.code {
                    EventKind::Delete | EventKind::CloseRequested
                        if event.target == inner.window.as_object_mutable() =>
                    {
                        running = false;
                    }
                    EventKind::Key => {
                        if let Some(Key::Enter) = event.key {
                            let text = Self::get_input(inner.input);

                            Self::print_line_internal(&mut inner, text).await?;

                            let text = Self::get_input(inner.input);

                            inner.validated_input.extend(text.as_bytes());
                            inner.validated_input.push_back(b'\n');

                            unsafe {
                                lvgl::lv_textarea_set_text(inner.input, c"".as_ptr());
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(running)
    }
}
