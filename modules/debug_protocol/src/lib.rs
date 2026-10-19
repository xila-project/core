#![no_std]

extern crate alloc;

use alloc::string::String;
use serde::{Deserialize, Serialize};

pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
pub const MAX_TEXT_BYTES: usize = 4 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
pub const MAX_SCREENSHOT_PIXELS: usize = 2_073_600;
pub const MAX_SCREENSHOT_BYTES: usize = MAX_SCREENSHOT_PIXELS * 4;
pub const DEFAULT_UI_TREE_ELEMENTS: usize = 64;
pub const DEFAULT_UI_TREE_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotPixelFormat {
    Rgba8888,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ScreenshotMetadata {
    pub width: u16,
    pub height: u16,
    pub format: ScreenshotPixelFormat,
    pub payload_bytes: usize,
}

pub fn validate_screenshot_metadata(metadata: ScreenshotMetadata) -> Result<(), ProtocolError> {
    let expected = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .and_then(|pixels| pixels.checked_mul(4));
    if metadata.width == 0
        || metadata.height == 0
        || expected != Some(metadata.payload_bytes)
        || metadata.payload_bytes > MAX_SCREENSHOT_BYTES
    {
        return Err(ProtocolError::InvalidScreenshotFrame);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_metadata_checks_dimensions_and_exact_payload_length() {
        let valid = ScreenshotMetadata {
            width: 2,
            height: 3,
            format: ScreenshotPixelFormat::Rgba8888,
            payload_bytes: 24,
        };
        assert_eq!(validate_screenshot_metadata(valid), Ok(()));
        assert_eq!(
            validate_screenshot_metadata(ScreenshotMetadata {
                payload_bytes: 23,
                ..valid
            }),
            Err(ProtocolError::InvalidScreenshotFrame)
        );
        assert_eq!(
            validate_screenshot_metadata(ScreenshotMetadata {
                width: 0,
                height: 3,
                payload_bytes: 0,
                ..valid
            }),
            Err(ProtocolError::InvalidScreenshotFrame)
        );
    }
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
    InvalidScreenshotFrame,
}

impl ProtocolError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::RequestTooLarge => "request_too_large",
            Self::InvalidJson => "invalid_json",
            Self::TextTooLarge => "text_too_large",
            Self::EmptyScreenshotPath => "empty_screenshot_path",
            Self::InvalidScreenshotFrame => "invalid_screenshot_frame",
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
