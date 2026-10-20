#[cfg(target_arch = "wasm32")]
use alloc::{format, string::String, vec, vec::Vec};
use shared::HttpResponseParser;
#[cfg(target_arch = "wasm32")]
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
    let marker = b"\r\n\r\n";
    let index = input.windows(marker.len()).position(|w| w == marker)?;
    Some((
        &input[..index + marker.len()],
        &input[index + marker.len()..],
    ))
}

pub fn extract_http_status(headers: &[u8]) -> Option<u16> {
    HttpResponseParser::from_buffer(headers).get_status_code()
}

#[cfg(target_arch = "wasm32")]
fn extract_content_length(headers: &[u8]) -> Option<usize> {
    HttpResponseParser::from_buffer(headers)
        .get_headers()
        .find(|(name, _)| *name == "Content-Length")
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
}

#[cfg(target_arch = "wasm32")]
fn has_chunked_encoding(headers: &[u8]) -> bool {
    let parser = HttpResponseParser::from_buffer(headers);
    let headers_iter = parser.get_headers();

    for (name, value) in headers_iter {
        if name.eq_ignore_ascii_case("Transfer-Encoding") {
            let is_chunked = value.contains("chunked");

            return is_chunked;
        }
    }

    false
}

#[cfg(target_arch = "wasm32")]
fn decode_chunked_body(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::new();
    let mut pos = 0;
    let mut chunk_count = 0;

    while pos < body.len() {
        // Find the line ending for chunk size (look for \r\n)
        let mut line_end = pos;
        while line_end + 1 < body.len() {
            if body[line_end] == b'\r' && body[line_end + 1] == b'\n' {
                break;
            }
            line_end += 1;
        }

        if line_end + 1 >= body.len() {
            // No line ending found
            break;
        }

        // Parse chunk size from bytes
        let size_bytes = &body[pos..line_end];
        let size_str = core::str::from_utf8(size_bytes)
            .map_err(|_| String::from("invalid utf8 in chunk size"))?;
        let chunk_size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|_| String::from("invalid chunk size format"))?;

        if chunk_size == 0 {
            // Last chunk reached

            break;
        }

        // Move past the chunk size line and \r\n
        pos = line_end + 2;

        // Read chunk data
        if pos + chunk_size > body.len() {
            return Err(String::from("chunk data extends beyond body"));
        }

        decoded.extend_from_slice(&body[pos..pos + chunk_size]);
        pos += chunk_size;

        // Skip trailing \r\n after chunk data
        if pos + 1 < body.len() && body[pos] == b'\r' && body[pos + 1] == b'\n' {
            pos += 2;
        }

        chunk_count += 1;
    }

    Ok(decoded)
}

#[cfg(target_arch = "wasm32")]
pub fn https_get(url: &str) -> Result<Vec<u8>, String> {
    let parsed = Url::parse(url).ok_or_else(|| String::from("invalid url"))?;

    let mut request_buffer = vec![0u8; 4096];
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
            .add_body(b"")
            .ok_or_else(|| String::from("request finalization overflow"))?;
        builder.get_position()
    };

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/devices/network/https_client")
        .map_err(|_e| String::from("failed to open https device"))?;

    file.write_all(&request_buffer[..request_length])
        .map_err(|_| String::from("failed to write request"))?;

    // Read headers first (device returns headers, then transitions to body streaming)
    let mut headers_buffer = vec![0u8; 2048];
    let headers_len = read_with_retry(
        &mut file,
        &mut headers_buffer,
        "failed to read response headers",
    )?;

    if headers_len == 0 {
        return Err(String::from("empty response"));
    }

    let status = extract_http_status(&headers_buffer[..headers_len])
        .ok_or_else(|| String::from("missing status in response headers"))?;
    if status != 200 {
        return Err(format!("api error: {status}"));
    }

    let expected_body_length = extract_content_length(&headers_buffer[..headers_len]);

    // Read body from subsequent reads
    let mut body = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut retry_count = 0usize;
    loop {
        if let Some(expected_length) = expected_body_length
            && body.len() >= expected_length
        {
            break;
        }

        let count = read_with_retry(&mut file, &mut chunk, "failed to read body")?;
        if count == 0 {
            if let Some(expected_length) = expected_body_length
                && body.len() < expected_length
            {
                retry_count += 1;
                if retry_count >= HTTPS_READ_MAX_RETRIES {
                    return Err(String::from("timed out waiting for https response body"));
                }

                yield_now();
                continue;
            }

            break;
        }
        retry_count = 0;
        body.extend_from_slice(&chunk[..count]);
    }

    // Decode chunked transfer encoding if present
    let final_body = if has_chunked_encoding(&headers_buffer[..headers_len]) {
        decode_chunked_body(&body)?
    } else {
        body
    };

    Ok(final_body)
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
    #[cfg(target_arch = "wasm32")]
    fn detects_chunked_encoding() {
        use super::has_chunked_encoding;
        let headers = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n";
        assert!(has_chunked_encoding(headers));
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn detects_no_chunked_encoding() {
        use super::has_chunked_encoding;
        let headers =
            b"HTTP/1.1 200 OK\r\nContent-Length: 1234\r\nContent-Type: application/json\r\n\r\n";
        assert!(!has_chunked_encoding(headers));
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn exposes_sync_https_get_api() {
        use alloc::{string::String, vec::Vec};

        use super::https_get;

        let _: fn(&str) -> Result<Vec<u8>, String> = https_get;
    }
}
