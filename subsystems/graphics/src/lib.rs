#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

mod area;
mod color;
mod display;
mod draw_buffer;
mod error;
mod event;
mod icon_spec;
mod icon_text;
mod input;
mod logo;
pub mod macros;
mod manager;
mod point;
mod screen;
mod screenshot;
pub mod symbol;
mod window;

pub mod lvgl;

pub use area::*;
pub use color::*;
pub use display::*;
pub use draw_buffer::*;
pub use error::*;
pub use event::*;
pub use icon_spec::*;
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
