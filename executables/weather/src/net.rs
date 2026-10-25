#[cfg(any(target_arch = "wasm32", test))]
use alloc::{format, string::String, vec, vec::Vec};
use shared::HttpResponseParser;
#[cfg(any(target_arch = "wasm32", test))]
use shared::{HttpRequestBuilder, Url};
#[cfg(target_arch = "wasm32")]
use std::fs::OpenOptions;
#[cfg(target_arch = "wasm32")]
use std::io::{ErrorKind, Read, Write};
#[cfg(target_arch = "wasm32")]
use std::thread::{sleep, yield_now};

#[cfg(target_arch = "wasm32")]
const RESOURCE_BUSY_OS_ERROR: i32 = 277;

#[cfg(target_arch = "wasm32")]
const HTTPS_READ_MAX_RETRIES: usize = 16_384;

#[cfg(target_arch = "wasm32")]
fn is_transient_read_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
        || error.raw_os_error() == Some(RESOURCE_BUSY_OS_ERROR)
}

#[cfg(target_arch = "wasm32")]
fn read_with_retry(
    file: &mut std::fs::File,
    buffer: &mut [u8],
    error_label: &str,
) -> Result<usize, String> {
    for attempt in 0..HTTPS_READ_MAX_RETRIES {
        match file.read(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if is_transient_read_error(&error) => {
                yield_now();

                if attempt % 64 == 0 {
                    sleep(std::time::Duration::from_millis(1));
                }
            }
            Err(_error) => {
                return Err(String::from(error_label));
            }
        }
    }

    Err(String::from("timed out waiting for https response"))
}

pub fn split_headers_body(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let body = shared::get_body(input)?;
    Some((&input[..input.len() - body.len()], body))
}

pub fn extract_http_status(headers: &[u8]) -> Option<u16> {
    HttpResponseParser::from_buffer(headers).get_status_code()
}

#[cfg(target_arch = "wasm32")]
pub fn https_get(url: &str) -> Result<Vec<u8>, String> {
    let request_buffer = build_weather_request(url)?;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/devices/network/https_client")
        .map_err(|_e| String::from("failed to open https device"))?;

    file.write_all(&request_buffer)
        .map_err(|_| String::from("failed to write request"))?;

    drop(request_buffer);
    read_device_response(|buffer| read_with_retry(&mut file, buffer, "failed to read response"))
}

#[cfg(any(target_arch = "wasm32", test))]
fn build_weather_request(url: &str) -> Result<Vec<u8>, String> {
    let parsed = Url::parse(url).ok_or_else(|| String::from("invalid url"))?;

    const FIXED_REQUEST: &[u8] = b"GET  HTTP/1.1\r\nHost: \r\nConnection: close\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nUser-Agent: Xila-Weather\r\n\r\n";
    let mut request_buffer = vec![0; FIXED_REQUEST.len() + parsed.path.len() + parsed.host.len()];
    let request_length = {
        let mut builder = HttpRequestBuilder::from_buffer(&mut request_buffer);
        builder
            .add_request("GET", parsed.path, HttpRequestBuilder::HTTP_VERSION_1_1)
            .ok_or_else(|| String::from("request line overflow"))?
            .add_header("Host", parsed.host.as_bytes())
            .ok_or_else(|| String::from("host header overflow"))?
            .add_header("Connection", b"close")
            .ok_or_else(|| String::from("connection header overflow"))?
            .add_header("Accept", b"application/json")
            .ok_or_else(|| String::from("accept header overflow"))?
            .add_header("Accept-Encoding", b"identity")
            .ok_or_else(|| String::from("accept encoding header overflow"))?
            .add_header("User-Agent", b"Xila-Weather")
            .ok_or_else(|| String::from("user agent header overflow"))?
            .add_body(b"")
            .ok_or_else(|| String::from("request finalization overflow"))?;
        builder.get_position()
    };

    request_buffer.truncate(request_length);
    Ok(request_buffer)
}

/// VFS bodies are decoded by the device. Keep any body prefix received with
/// the head, then reuse this allocation for JSON instead of copying through a
/// separate stack buffer. The JSON parser still needs the complete body.
#[cfg(any(target_arch = "wasm32", test))]
fn read_device_response(
    mut read: impl FnMut(&mut [u8]) -> Result<usize, String>,
) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0; 2048];
    let mut filled = 0;
    let head_length = loop {
        if let Some(body) = shared::get_body(&buffer[..filled]) {
            break filled - body.len();
        }
        if filled == buffer.len() {
            return Err(String::from("response headers too large"));
        }
        let n = read(&mut buffer[filled..])?;
        if n == 0 {
            return Err(String::from("incomplete response headers"));
        }
        filled += n;
    };
    let status = extract_http_status(&buffer[..head_length])
        .ok_or_else(|| String::from("missing status in response headers"))?;
    if status != 200 {
        return Err(format!("api error: {status}"));
    }
    buffer.copy_within(head_length..filled, 0);
    buffer.truncate(filled - head_length);
    loop {
        if buffer.len() == buffer.capacity() {
            // Probe EOF before growing: an exactly full response must not
            // double its allocation just to discover that it has finished.
            let mut byte = [0];
            if read(&mut byte)? == 0 {
                break;
            }
            buffer.push(byte[0]);
        }
        let start = buffer.len();
        let end = start + (buffer.capacity() - start).min(512);
        buffer.resize(end, 0);
        let n = read(&mut buffer[start..])?;
        buffer.truncate(start + n);
        if n == 0 {
            break;
        }
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::{extract_http_status, split_headers_body};

    #[test]
    fn split_headers_and_body_works() {
        let bytes = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}";
        let (headers, body) = split_headers_body(bytes).unwrap();
        assert!(core::str::from_utf8(headers).unwrap().contains("200"));
        assert_eq!(body, b"{\"ok\":true}");
    }

    #[test]
    fn extracts_status_code() {
        let headers = b"HTTP/1.1 404 Not Found\r\nX-Test: 1\r\n\r\n";
        assert_eq!(extract_http_status(headers), Some(404));
    }

    #[test]
    fn response_is_independent_of_read_boundaries() {
        use super::read_device_response;
        let head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        // Device bodies are already decoded, even with the original header.
        let wire = [head.as_slice(), b"{\"ok\":true}"].concat();
        for fragment in 1..=wire.len() {
            let mut remaining = wire.as_slice();
            let body = read_device_response(|out| {
                let n = remaining.len().min(out.len()).min(fragment);
                out[..n].copy_from_slice(&remaining[..n]);
                remaining = &remaining[n..];
                Ok(n)
            })
            .unwrap();
            assert_eq!(body, b"{\"ok\":true}");
        }
    }

    #[test]
    fn response_errors_and_lf_headers() {
        use super::read_device_response;
        for (wire, expected) in [
            (
                b"HTTP/1.1 503 Service Unavailable\r\n\r\n".as_slice(),
                "api error: 503",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: incomplete",
                "incomplete response headers",
            ),
        ] {
            let mut remaining = wire;
            let error = read_device_response(|out| {
                let n = out.len().min(remaining.len());
                out[..n].copy_from_slice(&remaining[..n]);
                remaining = &remaining[n..];
                Ok(n)
            })
            .unwrap_err();
            assert_eq!(error, expected);
        }
        let mut remaining = b"HTTP/1.0 200 OK\nX: y\n\nbody".as_slice();
        assert_eq!(
            read_device_response(|out| {
                let n = out.len().min(remaining.len());
                out[..n].copy_from_slice(&remaining[..n]);
                remaining = &remaining[n..];
                Ok(n)
            })
            .unwrap(),
            b"body"
        );
        let mut remaining = b"HTTP/1.1 200 OK\r\n\r\npartial".as_slice();
        assert_eq!(
            read_device_response(|out| {
                if remaining.is_empty() {
                    return Err(alloc::string::String::from("truncated device body"));
                }
                let n = out.len().min(remaining.len());
                out[..n].copy_from_slice(&remaining[..n]);
                remaining = &remaining[n..];
                Ok(n)
            })
            .unwrap_err(),
            "truncated device body"
        );
    }

    #[test]
    fn request_has_exact_capacity_and_explicit_encoding() {
        let request = super::build_weather_request("https://example.com/weather?q=Paris").unwrap();
        assert_eq!(request.len(), request.capacity());
        assert_eq!(request, b"GET /weather?q=Paris HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nUser-Agent: Xila-Weather\r\n\r\n");
    }

    #[test]
    fn large_response_uses_existing_capacity_until_it_is_full() {
        use super::read_device_response;
        let mut remaining = 8192;
        let mut head = b"HTTP/1.1 200 OK\r\n\r\n".as_slice();
        let body = read_device_response(|out| {
            if !head.is_empty() {
                let n = head.len().min(out.len());
                out[..n].copy_from_slice(&head[..n]);
                head = &head[n..];
                return Ok(n);
            }
            let n = out.len().min(remaining);
            out[..n].fill(b'x');
            remaining -= n;
            Ok(n)
        })
        .unwrap();
        assert_eq!(body.len(), 8192);
        assert_eq!(body.capacity(), 8192, "EOF must not double a full buffer");
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn exposes_sync_https_get_api() {
        use alloc::{string::String, vec::Vec};

        use super::https_get;

        let _: fn(&str) -> Result<Vec<u8>, String> = https_get;
    }
}
