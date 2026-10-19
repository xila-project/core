use std::io::{BufRead, Read, Write};

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

        let mut response = Vec::new();
        std::io::BufReader::new(stream)
            .take(64 * 1024)
            .read_until(b'\n', &mut response)
            .map_err(|error| format!("Failed to read response: {error}"))?;
        if response.len() >= 64 * 1024 {
            return Err("Response exceeds the 64 KiB limit".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&response)
            .map_err(|error| format!("Invalid response: {error}"))?;
        println!("{}", String::from_utf8_lossy(&response).trim_end());
        Ok(value["ok"] == true)
    }
    #[cfg(not(unix))]
    {
        let _ = endpoint;
        Err("The native control client is not available on this platform yet.".into())
    }
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
