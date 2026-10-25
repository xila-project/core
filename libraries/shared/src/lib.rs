#![no_std]

extern crate alloc;

mod any;
mod bijective_map;
mod error;
pub mod shadow_type;
mod size;
mod slice;
pub mod task;
mod time;
mod unit;
mod utf8;

pub use any::*;
pub use bijective_map::*;
pub use error::*;
pub use size::*;
pub use slice::*;
pub use time::*;
pub use unit::*;
pub use utf8::*;
pub use xila_http::{
    HEADER_SEPARATOR, HttpRequestBuilder, HttpRequestParser, HttpResponseBuilder,
    HttpResponseParser, LINE_ENDING, Url, add_line, add_line_iterator, format_url, get_body,
    parse_header, split_lines,
};
