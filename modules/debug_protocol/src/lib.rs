#![no_std]

extern crate alloc;

use alloc::string::String;
use serde::{Deserialize, Serialize};

pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
pub const MAX_TEXT_BYTES: usize = 4 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
pub const DEFAULT_UI_TREE_ELEMENTS: usize = 64;
pub const DEFAULT_UI_TREE_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputState {
    Pressed,
    Released,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Diagnostics,
    Resolution,
    Screenshot {
        path: String,
    },
    UiTree {
        #[serde(default = "default_max_elements")]
        max_elements: usize,
        #[serde(default = "default_max_depth")]
        max_depth: usize,
    },
    PointerMove {
        x: u32,
        y: u32,
    },
    PointerButton {
        state: InputState,
    },
    Key {
        key: String,
        state: InputState,
    },
    Text {
        text: String,
    },
}

fn default_max_elements() -> usize {
    DEFAULT_UI_TREE_ELEMENTS
}

fn default_max_depth() -> usize {
    DEFAULT_UI_TREE_DEPTH
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    RequestTooLarge,
    InvalidJson,
    TextTooLarge,
    EmptyScreenshotPath,
}

impl ProtocolError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::RequestTooLarge => "request_too_large",
            Self::InvalidJson => "invalid_json",
            Self::TextTooLarge => "text_too_large",
            Self::EmptyScreenshotPath => "empty_screenshot_path",
        }
    }
}

pub fn validate_request(request: &Request) -> Result<(), ProtocolError> {
    if let Request::Text { text } = request
        && text.len() > MAX_TEXT_BYTES
    {
        return Err(ProtocolError::TextTooLarge);
    }
    if let Request::Screenshot { path } = request
        && path.is_empty()
    {
        return Err(ProtocolError::EmptyScreenshotPath);
    }
    Ok(())
}
