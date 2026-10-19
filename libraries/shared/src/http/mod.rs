mod request;
mod response;
mod url;

use alloc::string::String;
pub use request::*;
pub use response::*;
pub use url::*;

pub const LINE_ENDING: &str = "\r\n";
pub const HEADER_SEPARATOR: &str = ": ";

pub fn add_line(buffer: &mut [u8], position: usize, line: &[u8]) -> Option<usize> {
    let destination_buffer = buffer.get_mut(position..position + line.len())?;
    destination_buffer.copy_from_slice(line);
    let mut new_position = position + line.len();

    // Add line ending
    let line_ending = LINE_ENDING.as_bytes();
    let destination_buffer = buffer.get_mut(new_position..new_position + line_ending.len())?;
    destination_buffer.copy_from_slice(line_ending);
    new_position += line_ending.len();

    Some(new_position)
}

pub fn add_line_iterator<'a, I>(buffer: &mut [u8], mut position: usize, iter: I) -> Option<usize>
where
    I: Iterator<Item = &'a [u8]>,
{
    for part in iter {
        let part_length = part.len();

        let destination_buffer = buffer.get_mut(position..position + part_length)?;
        destination_buffer.copy_from_slice(part);
        position += part_length;
    }

    // Add line ending
    let line_ending = LINE_ENDING.as_bytes();
    let destination_buffer = buffer.get_mut(position..position + line_ending.len())?;
    destination_buffer.copy_from_slice(line_ending);
    position += line_ending.len();

    Some(position)
}

pub fn parse_header(buffer: &[u8]) -> Option<(&str, &str)> {
    let mut parts = buffer.splitn(2, |&b| b == b':');

    let name = parts.next()?;
    let value = parts.next()?;

    let name = core::str::from_utf8(name).ok()?.trim();
    let value = core::str::from_utf8(value).ok()?.trim();

    Some((name, value))
}

pub fn split_lines(buffer: &[u8]) -> impl Iterator<Item = &[u8]> + '_ + Clone {
    buffer.split(|&b| b == b'\n').map(|line| {
        if line.ends_with(b"\r") {
            &line[..line.len() - 1]
        } else {
            line
        }
    })
}

pub fn get_body(buffer: &[u8]) -> Option<&[u8]> {
    // Find the end of headers where 2 consecutive line endings occur, DO NOT SPLIT LINES
    let headers_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| pos + 4)?; // Move past the header ending

    buffer.get(headers_end..)
}

pub fn format_url<'a>(
    scheme: &str,
    host: &str,
    query_parameters: impl Iterator<Item = (&'a str, &'a str)> + Clone,
) -> String {
    let total_query_length: usize = query_parameters
        .clone()
        .map(|(key, value)| key.len() + value.len() + 2) // +2 for '=' and '&'
        .sum();

    let mut url = String::with_capacity(
        scheme.len() + 3 + host.len() + 1 + total_query_length.saturating_sub(1),
    ); // 3 for "://", 1 for "?",

    url.push_str(scheme);
    url.push_str("://");
    url.push_str(host);
    if total_query_length > 0 {
        url.push('?');

        for (i, (key, value)) in query_parameters.enumerate() {
            url.push_str(key);
            url.push('=');
            url.push_str(value);

            if i < total_query_length - 1 {
                url.push('&');
            }
        }
    }

    url
}
