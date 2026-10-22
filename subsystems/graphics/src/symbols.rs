use core::ffi::CStr;

use crate::lvgl;

// LVGL symbols
pub const BULLET: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BULLET) };
pub const AUDIO: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_AUDIO) };
pub const VIDEO: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_VIDEO) };
pub const LIST: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_LIST) };
pub const OK: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_OK) };
pub const CLOSE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_CLOSE) };
pub const POWER: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_POWER) };
pub const SETTINGS: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_SETTINGS) };
pub const HOME: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_HOME) };
pub const DOWNLOAD: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_DOWNLOAD) };
pub const DRIVE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_DRIVE) };
pub const REFRESH: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_REFRESH) };
pub const MUTE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_MUTE) };
pub const VOLUME_MID: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_VOLUME_MID) };
pub const VOLUME_MAX: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_VOLUME_MAX) };
pub const IMAGE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_IMAGE) };
pub const TINT: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_TINT) };
pub const PREV: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_PREV) };
pub const PLAY: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_PLAY) };
pub const PAUSE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_PAUSE) };
pub const STOP: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_STOP) };
pub const NEXT: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_NEXT) };
pub const EJECT: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_EJECT) };
pub const LEFT: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_LEFT) };
pub const RIGHT: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_RIGHT) };
pub const PLUS: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_PLUS) };
pub const MINUS: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_MINUS) };
pub const EYE_OPEN: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_EYE_OPEN) };
pub const EYE_CLOSE: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_EYE_CLOSE) };
pub const WARNING: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_WARNING) };
pub const SHUFFLE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_SHUFFLE) };
pub const UP: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_UP) };
pub const DOWN: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_DOWN) };
pub const LOOP: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_LOOP) };
pub const DIRECTORY: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_DIRECTORY) };
pub const UPLOAD: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_UPLOAD) };
pub const CALL: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_CALL) };
pub const CUT: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_CUT) };
pub const COPY: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_COPY) };
pub const SAVE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_SAVE) };
pub const BARS: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BARS) };
pub const ENVELOPE: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_ENVELOPE) };
pub const CHARGE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_CHARGE) };
pub const PASTE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_PASTE) };
pub const BELL: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BELL) };
pub const KEYBOARD: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_KEYBOARD) };
pub const GPS: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_GPS) };
pub const FILE: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_FILE) };
pub const WIFI: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_WIFI) };
pub const BATTERY_FULL: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BATTERY_FULL) };
pub const BATTERY_3: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BATTERY_3) };
pub const BATTERY_2: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BATTERY_2) };
pub const BATTERY_1: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BATTERY_1) };
pub const BATTERY_EMPTY: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BATTERY_EMPTY) };
pub const USB: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_USB) };
pub const BLUETOOTH: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BLUETOOTH) };
pub const TRASH: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_TRASH) };
pub const EDIT: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_EDIT) };
pub const BACKSPACE: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_BACKSPACE) };
pub const SD_CARD: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_SD_CARD) };
pub const NEW_LINE: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_NEW_LINE) };
pub const DUMMY: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(lvgl::LV_SYMBOL_DUMMY) };
// Additional symbols
pub const NETWORK_WIRED: &CStr = c"\xEF\x9B\xBF";
