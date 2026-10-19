use crate::lvgl;

unsafe extern "C" {
    static font_montserrat_10: lvgl::lv_font_t;
    static font_montserrat_14: lvgl::lv_font_t;
    static font_montserrat_18: lvgl::lv_font_t;
    static font_montserrat_28: lvgl::lv_font_t;
    static font_unscii_8: lvgl::lv_font_t;
    static font_unscii_16: lvgl::lv_font_t;
}

pub const fn get_font_small() -> &'static lvgl::lv_font_t {
    unsafe { &font_montserrat_10 }
}

pub const fn get_font_medium() -> &'static lvgl::lv_font_t {
    unsafe { &font_montserrat_14 }
}

pub const fn get_font_large() -> &'static lvgl::lv_font_t {
    unsafe { &font_montserrat_18 }
}

pub const fn get_font_extra_large() -> &'static lvgl::lv_font_t {
    unsafe { &font_montserrat_28 }
}

pub const fn get_font_monospace_small() -> &'static lvgl::lv_font_t {
    unsafe { &font_unscii_8 }
}

pub const fn get_font_monospace_medium() -> &'static lvgl::lv_font_t {
    unsafe { &font_unscii_16 }
}
