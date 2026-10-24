use crate::lvgl;

// Every generated range has unique C symbols in separate archive members.
// These aliases reference only the selected range, with no runtime registry.
macro_rules! selected_fonts {
    ($($name:ident),* $(,)?) => {
        unsafe extern "C" {
            $(
                #[link_name = concat!(stringify!($name), "_", env!("XILA_FONT_VARIANT"))]
                static $name: lvgl::lv_font_t;
            )*
        }
    };
}

selected_fonts!(
    font_montserrat_10,
    font_montserrat_14,
    font_montserrat_18,
    font_montserrat_28,
    font_unscii_8,
    font_unscii_16,
);

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
