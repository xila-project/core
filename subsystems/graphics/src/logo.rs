use crate::{Color, Error, Point, Result, lvgl};

pub struct Logo {
    pub object: *mut lvgl::lv_obj_t,
}

impl Logo {
    pub const BASE_SIZE: Point = Point::new(32, 32);
    pub const BASE_PART_SIZE: Point = Point::new(10, 21);

    pub fn get_factor(size: Point) -> u8 {
        let factor_x = size.get_x() / Self::BASE_SIZE.get_x();
        let factor_y = size.get_y() / Self::BASE_SIZE.get_y();

        factor_x.min(factor_y) as u8
    }

    /// Create a new Logo object
    ///
    /// # Arguments
    /// * `Parent` - The parent object to create the logo on.
    /// * `Factor` - The scaling factor for the logo size.
    /// * `Color` - The color of the logo parts.
    ///
    /// # Safety
    /// This function is unsafe because it may dereference raw pointers (e.g.
    pub unsafe fn new(parent: *mut lvgl::lv_obj_t, factor: u8, color: Color) -> Result<Self> {
        unsafe {
            let object = lvgl::lv_button_create(parent);

            if object.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            let size = Self::BASE_SIZE.scale(factor as f64);

            lvgl::lv_obj_set_size(object, size.x.into(), size.y.into());
            // Logo bars use explicit corner geometry rather than the control's
            // flex layout and minimum touch-control height.
            lvgl::lv_obj_set_style_layout(
                object,
                lvgl::lv_layout_t_LV_LAYOUT_NONE as u16,
                lvgl::LV_STATE_DEFAULT,
            );
            lvgl::lv_obj_set_style_min_height(object, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_bg_opa(object, lvgl::LV_OPA_0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_pad_all(object, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_radius(object, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_border_width(object, 0, lvgl::LV_STATE_DEFAULT);

            Self::new_part(object, lvgl::lv_align_t_LV_ALIGN_TOP_LEFT, factor, color)?;
            Self::new_part(object, lvgl::lv_align_t_LV_ALIGN_BOTTOM_LEFT, factor, color)?;
            Self::new_part(
                object,
                lvgl::lv_align_t_LV_ALIGN_BOTTOM_RIGHT,
                factor,
                color,
            )?;
            Self::new_part(object, lvgl::lv_align_t_LV_ALIGN_TOP_RIGHT, factor, color)?;

            Ok(Self { object })
        }
    }

    pub fn get_inner_object(&self) -> *mut lvgl::lv_obj_t {
        self.object
    }

    fn new_part(
        parent: *mut lvgl::lv_obj_t,
        alignment: lvgl::lv_align_t,
        factor: u8,
        color: Color,
    ) -> Result<*mut lvgl::lv_obj_t> {
        let size = Self::BASE_PART_SIZE.scale(factor as f64);

        unsafe {
            let part = lvgl::lv_button_create(parent);

            if part.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            lvgl::lv_obj_set_style_bg_color(part, color.into_lvgl_color(), lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_bg_color(part, lvgl::lv_color_white(), lvgl::LV_STATE_PRESSED);

            lvgl::lv_obj_set_align(part, alignment);

            match alignment {
                lvgl::lv_align_t_LV_ALIGN_TOP_LEFT | lvgl::lv_align_t_LV_ALIGN_BOTTOM_RIGHT => {
                    lvgl::lv_obj_set_size(part, size.x.into(), size.y.into());
                }
                lvgl::lv_align_t_LV_ALIGN_BOTTOM_LEFT | lvgl::lv_align_t_LV_ALIGN_TOP_RIGHT => {
                    lvgl::lv_obj_set_size(part, size.y.into(), size.x.into());
                }
                _ => {}
            }

            lvgl::lv_obj_set_style_pad_all(part, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_min_height(part, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_radius(part, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_border_width(part, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_add_flag(part, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);

            Ok(part)
        }
    }
}

impl Drop for Logo {
    fn drop(&mut self) {
        unsafe {
            lvgl::lv_obj_delete(self.object);
        }
    }
}
