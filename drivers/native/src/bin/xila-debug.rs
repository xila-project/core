use std::io::{BufReader, Read, Write};

use drivers_native::window_screen::control::protocol::{InputState, Request};

fn main() {
    match run() {
        Ok(success) => std::process::exit(if success { 0 } else { 1 }),
        Err(error) => {
            eprintln!("xila-debug: {error}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<bool, String> {
    let mut arguments = std::env::args().skip(1).peekable();
    let mut endpoint = None;
    if arguments
        .peek()
        .is_some_and(|argument| argument == "--endpoint")
    {
        arguments.next();
        endpoint = Some(arguments.next().ok_or("--endpoint requires a path")?.into());
    }

    let operation = arguments.next().ok_or_else(usage)?;
    let request = parse_operation(&operation, &mut arguments)?;
    let screenshot_path = match &request {
        Request::Screenshot { path } => Some(path.clone()),
        _ => None,
    };
    if arguments.next().is_some() {
        return Err(usage());
    }

    #[cfg(unix)]
    {
        let endpoint = endpoint.unwrap_or_else(|| {
            drivers_native::window_screen::control::transport::endpoint_path().unwrap_or_else(
                |error| {
                    eprintln!("xila-debug: {error}");
                    std::process::exit(2);
                },
            )
        });
        let mut stream = std::os::unix::net::UnixStream::connect(&endpoint)
            .map_err(|error| format!("Cannot connect to {}: {error}", endpoint.display()))?;
        let mut bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        stream
            .write_all(&bytes)
            .map_err(|error| format!("Failed to send request: {error}"))?;

        let mut reader = BufReader::new(stream);
        let mut header = Vec::new();
        loop {
            let mut byte = [0];
            reader
                .read_exact(&mut byte)
                .map_err(|error| format!("Failed to read response header: {error}"))?;
            if byte[0] == b'\n' {
                break;
            }
            if header.len() + 1 >= debug_protocol::MAX_RESPONSE_BYTES {
                return Err("Response header exceeds the 64 KiB limit".into());
            }
            header.push(byte[0]);
        }
        let value: serde_json::Value = serde_json::from_slice(&header)
            .map_err(|error| format!("Invalid response: {error}"))?;
        let payload_bytes = value
            .pointer("/result/payload_bytes")
            .and_then(serde_json::Value::as_u64)
            .map(usize::try_from)
            .transpose()
            .map_err(|_| "Invalid screenshot payload length")?
            .unwrap_or(0);
        if payload_bytes > debug_protocol::MAX_SCREENSHOT_BYTES {
            return Err("Screenshot payload exceeds the 8 MiB limit".into());
        }
        if value["ok"] != true {
            if payload_bytes != 0 {
                return Err("Unexpected payload in an error response".into());
            }
            println!("{}", String::from_utf8_lossy(&header));
            return Ok(false);
        }
        if let Some(path) = screenshot_path {
            let metadata: debug_protocol::ScreenshotMetadata = serde_json::from_value(
                value
                    .get("result")
                    .cloned()
                    .ok_or("Screenshot response has no metadata")?,
            )
            .map_err(|error| format!("Invalid screenshot metadata: {error}"))?;
            debug_protocol::validate_screenshot_metadata(metadata)
                .map_err(|_| "Invalid screenshot dimensions or payload length")?;
            if metadata.payload_bytes != payload_bytes
                || metadata.format != debug_protocol::ScreenshotPixelFormat::Rgba8888
            {
                return Err("Unsupported screenshot pixel format or payload length".into());
            }
            let mut pixels = vec![0; payload_bytes];
            reader
                .read_exact(&mut pixels)
                .map_err(|error| format!("Truncated screenshot payload: {error}"))?;
            write_png(&path, metadata.width, metadata.height, &pixels)?;
            println!("{}", String::from_utf8_lossy(&header));
            println!("Screenshot written to {path}");
        } else {
            if payload_bytes != 0 {
                return Err("Unexpected binary payload in non-screenshot response".into());
            }
            println!("{}", String::from_utf8_lossy(&header));
        }
        Ok(value["ok"] == true)
    }
    #[cfg(not(unix))]
    {
        let _ = endpoint;
        Err("The native control client is not available on this platform yet.".into())
    }
}

fn write_png(path: &str, width: u16, height: u16, pixels: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path)
        .map_err(|error| format!("Failed to create screenshot {path}: {error}"))?;
    let mut encoder = png::Encoder::new(file, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("Failed to encode screenshot PNG: {error}"))?;
    writer
        .write_image_data(pixels)
        .map_err(|error| format!("Failed to write screenshot PNG: {error}"))
}

fn parse_operation(
    operation: &str,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<Request, String> {
    match operation {
        "diagnostics" => Ok(Request::Diagnostics),
        "resolution" => Ok(Request::Resolution),
        "screenshot" => Ok(Request::Screenshot {
            path: arguments.next().ok_or("screenshot requires a path")?,
        }),
        "ui-tree" => Ok(Request::UiTree {
            max_elements: arguments
                .next()
                .map(|value| value.parse().map_err(|_| "invalid max_elements"))
                .transpose()?
                .unwrap_or(256),
            max_depth: arguments
                .next()
                .map(|value| value.parse().map_err(|_| "invalid max_depth"))
                .transpose()?
                .unwrap_or(24),
        }),
        "pointer" => match arguments.next().as_deref() {
            Some("move") => Ok(Request::PointerMove {
                x: arguments
                    .next()
                    .ok_or("pointer move requires x")?
                    .parse()
                    .map_err(|_| "invalid x coordinate")?,
                y: arguments
                    .next()
                    .ok_or("pointer move requires y")?
                    .parse()
                    .map_err(|_| "invalid y coordinate")?,
            }),
            Some("button") => Ok(Request::PointerButton {
                state: parse_state(arguments.next().as_deref())?,
            }),
            _ => Err("pointer expects `move X Y` or `button press|release`".into()),
        },
        "key" => Ok(Request::Key {
            key: arguments.next().ok_or("key requires a name")?,
            state: parse_state(arguments.next().as_deref())?,
        }),
        "text" => {
            let text = arguments.collect::<Vec<_>>().join(" ");
            Ok(Request::Text { text })
        }
        _ => Err(usage()),
    }
}

fn parse_state(value: Option<&str>) -> Result<InputState, String> {
    match value {
        Some("press" | "pressed") => Ok(InputState::Pressed),
        Some("release" | "released") => Ok(InputState::Released),
        _ => Err("state must be press or release".into()),
    }
}

fn usage() -> String {
    "usage: xila-debug [--endpoint PATH] diagnostics|resolution|screenshot PATH|ui-tree [MAX_ELEMENTS MAX_DEPTH]|pointer move X Y|pointer button press|release|key KEY press|release|text TEXT".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pointer_key_text_and_screenshot_requests() {
        assert_eq!(
            parse_operation(
                "pointer",
                &mut ["move".into(), "12".into(), "34".into()].into_iter()
            ),
            Ok(Request::PointerMove { x: 12, y: 34 })
        );
        assert_eq!(
            parse_operation("key", &mut ["enter".into(), "press".into()].into_iter()),
            Ok(Request::Key {
                key: "enter".into(),
                state: InputState::Pressed,
            })
        );
        assert_eq!(
            parse_operation("screenshot", &mut ["capture.png".into()].into_iter()),
            Ok(Request::Screenshot {
                path: "capture.png".into(),
            })
        );
    }
}
