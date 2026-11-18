#![no_std]

extern crate alloc;

embedded_i18n::i18n!("locales");

mod area;
mod color;
mod display;
mod draw_buffer;
mod error;
mod event;
mod event_queue;
mod icon_text;
mod input;
mod inspection;
mod logo;
pub mod macros;
mod manager;
pub mod object_id;
pub use inspection::*;
mod point;
mod screen;
mod screenshot;
pub mod symbol;
mod window;

/// Core's own raw LVGL access: the generated bindings and a few helpers.
/// Private, so the public [`lvgl`] module can be deprecated.
mod ffi;

/// Raw LVGL bindings.
///
/// Deprecated: use [`lvgl_rust`] (borrowed views under the graphics lock).
/// The module goes away before the first release. Rust only reports a
/// deprecated module where it is imported (`use graphics::lvgl;`) and for
/// the helpers defined here, not for a full path or a named import of an item
/// it re-exports, so migrated crates set `#![deny(deprecated)]` and keep
/// the import itself out of their code.
#[deprecated(
    note = "raw LVGL access is being removed before the first release: use `graphics::lvgl_rust`"
)]
pub mod lvgl {
    pub use crate::ffi::*;
}
/// The safe LVGL layer (sessions, views, generational handles). Enter it with
/// [`Manager::with_lvgl`]; see the README.
pub use lvgl_rust;

pub use area::*;
pub use color::*;
pub use display::*;
pub use draw_buffer::*;
pub use error::*;
pub use event::*;
pub use event_queue::WINDOW_EVENT_QUEUE_CAPACITY;
pub use input::*;
pub use manager::*;
pub use point::*;
pub use screen::*;
pub use screenshot::*;
pub use window::*;
pub mod stubs;
pub use logo::*;
pub mod appearance;
pub mod fonts;
pub mod theme;

mod file_picker;
pub use file_picker::*;
