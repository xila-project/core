use std::os::raw::c_void;

use graphics::{
    Color, Logo,
    lvgl::{
        _lv_obj_t, LV_PART_MAIN, LV_STATE_DEFAULT, lv_align_t_LV_ALIGN_CENTER, lv_anim_delete,
        lv_anim_init, lv_anim_path_ease_in_out, lv_anim_set_duration, lv_anim_set_exec_cb,
        lv_anim_set_path_cb, lv_anim_set_repeat_count, lv_anim_set_repeat_delay,
        lv_anim_set_reverse_delay, lv_anim_set_reverse_time, lv_anim_set_values, lv_anim_set_var,
        lv_anim_start, lv_anim_t, lv_obj_get_child, lv_obj_get_child_count, lv_obj_get_size,
        lv_obj_get_style_text_color, lv_obj_set_align, lv_obj_set_style_opa,
    },
};

mod error;

pub use error::{Error, Result};

pub struct Bootsplash {
    animation: Box<lv_anim_t>,
    _logo: Logo,
}

unsafe extern "C" fn load_animation_callback(object: *mut c_void, value: i32) {
    static mut ANIMATED_PART: u8 = 2;
    let object = object as *mut _lv_obj_t;

    unsafe {
        if (value == 255) || (value == 64) {
            if ANIMATED_PART == 4 {
                if value == 64 {
                    ANIMATED_PART = 1;
                }
            } else {
                ANIMATED_PART += 1;
            }
        }

        let next_part = lv_obj_get_child(object, (ANIMATED_PART - 1) as i32);

        if ANIMATED_PART.is_multiple_of(2) {
            // lv_obj_set_style_shadow_width(next_part, 255 + 64 - value, LV_STATE_DEFAULT);

            lv_obj_set_style_opa(next_part, (255 + 64 - value) as u8, LV_STATE_DEFAULT);

            let previous_part = lv_obj_get_child(object, (ANIMATED_PART - 2) as i32);

            lv_obj_set_style_opa(previous_part, value as u8, LV_STATE_DEFAULT);
        } else {
            // lv_obj_set_style_shadow_width(next_part, value, LV_STATE_DEFAULT);

            lv_obj_set_style_opa(next_part, value as u8, LV_STATE_DEFAULT);

            if ANIMATED_PART == 1 {
                let previous_part = lv_obj_get_child(object, 3);

                lv_obj_set_style_opa(previous_part, (255 + 64 - value) as u8, LV_STATE_DEFAULT);
            } else {
                let previous_part = lv_obj_get_child(object, (ANIMATED_PART - 2) as i32);

                lv_obj_set_style_opa(previous_part, (255 + 64 - value) as u8, LV_STATE_DEFAULT);
            }
        }
    }
}

impl Bootsplash {
    pub async fn new(graphics_manager: &'static graphics::Manager) -> Result<Self> {
        let _lock = graphics_manager.lock().await;

        unsafe {
            let current_screen = graphics_manager.get_current_screen()?;

            let screen_size = lv_obj_get_size(current_screen);
            let factor = Logo::get_factor(screen_size.scale(0.3));

            let foreground =
                Color::from_lvgl_color(lv_obj_get_style_text_color(current_screen, LV_PART_MAIN));
            let logo = Logo::new(current_screen, factor, foreground)?;

            let logo_inner_object = logo.get_inner_object();

            lv_obj_set_align(logo_inner_object, lv_align_t_LV_ALIGN_CENTER);
            let child_count = lv_obj_get_child_count(logo_inner_object);
            for i in 0..child_count {
                let part = lv_obj_get_child(logo_inner_object, i as i32);

                lv_obj_set_style_opa(part, 0, LV_STATE_DEFAULT);
            }

            let mut s = Self {
                animation: Box::new(lv_anim_t::default()),
                _logo: logo,
            };

            lv_anim_init(&mut *s.animation);
            lv_anim_set_var(&mut *s.animation, logo_inner_object as *mut c_void);
            lv_anim_set_values(&mut *s.animation, 64, 255);
            lv_anim_set_duration(&mut *s.animation, 500);
            lv_anim_set_reverse_delay(&mut *s.animation, 0);
            lv_anim_set_reverse_time(&mut *s.animation, 500);
            lv_anim_set_repeat_delay(&mut *s.animation, 0);
            lv_anim_set_repeat_count(&mut *s.animation, u32::MAX);
            lv_anim_set_path_cb(&mut *s.animation, Some(lv_anim_path_ease_in_out));
            lv_anim_set_exec_cb(&mut *s.animation, Some(load_animation_callback));
            lv_anim_start(&*s.animation);

            Ok(s)
        }
    }

    pub async fn stop(self, graphics_manager: &'static graphics::Manager) -> Result<()> {
        let _lock = graphics_manager.lock().await;

        unsafe {
            lv_anim_delete(self.animation.var, Some(load_animation_callback));
        }

        Ok(())
    }
}
