use crate::{get_body, parse_header, split_lines};
use alloc::string::ToString;

pub struct HttpResponseParser<'a>(&'a [u8]);

impl<'a> HttpResponseParser<'a> {
    pub fn from_buffer(buffer: &'a [u8]) -> Self {
        HttpResponseParser(buffer)
    }

    pub fn split_lines(&self) -> impl Iterator<Item = &'a [u8]> + Clone + 'a {
        split_lines(self.0)
    }

    pub fn get_status_code(&self) -> Option<u16> {
        let mut lines = self.split_lines();

        let status_line = lines.next()?;

        let mut parts = status_line.splitn(3, |&b| b == b' ');

        parts.next()?; // Skip HTTP version
        let status_code_str = parts.next()?;

        let status_code_str = core::str::from_utf8(status_code_str).ok()?;
        let status_code = status_code_str.parse::<u16>().ok()?;

        Some(status_code)
    }

    pub fn get_headers(&self) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        let mut lines = self.split_lines();

        // Skip request line
        lines.next();

        lines
            .take_while(|line| !line.is_empty())
            .filter_map(parse_header)
    }

    pub fn get_body(&self) -> Option<&'a [u8]> {
        get_body(self.0)
    }
}

pub struct HttpResponseBuilder<'a> {
    buffer: &'a mut [u8],
    position: usize,
}

impl<'a> HttpResponseBuilder<'a> {
    pub const LINE_ENDING: &'static [u8] = b"\r\n";

    pub fn from_buffer(buffer: &'a mut [u8]) -> Self {
        buffer.fill(0);

        HttpResponseBuilder {
            buffer,
            position: 0,
        }
    }

    pub fn add_line(&mut self, buffer: &[u8]) -> Option<()> {
        let total_length = buffer.len() + Self::LINE_ENDING.len();

        let destination_buffer = self
            .buffer
            .get_mut(self.position..self.position + total_length)?;

        destination_buffer[..buffer.len()].copy_from_slice(buffer);
        destination_buffer[buffer.len()..buffer.len() + Self::LINE_ENDING.len()]
            .copy_from_slice(Self::LINE_ENDING);

        self.position += total_length;

        Some(())
    }

    pub fn add_line_iterator<'b, I>(&mut self, iter: I) -> Option<()>
    where
        I: Iterator<Item = &'b [u8]>,
    {
        // add in the same line all the parts from the iterator
        let mut position = self.position; // temporary position, will be commited if all parts fit

        for part in iter {
            let part_length = part.len();

            let destination_buffer = self.buffer.get_mut(position..position + part_length)?;

            destination_buffer.copy_from_slice(part);
            position += part_length;
        }

        let destination_buffer = self
            .buffer
            .get_mut(position..position + Self::LINE_ENDING.len())?;
        destination_buffer.copy_from_slice(Self::LINE_ENDING);
        position += Self::LINE_ENDING.len();

        self.position = position;

        Some(())
    }

    pub fn add_status_code(&mut self, status_code: u16) -> Option<()> {
        let status_line = &["HTTP/1.1 ", &status_code.to_string(), " "].concat();
        self.add_line(status_line.as_bytes())
    }

    pub fn add_header(&mut self, name: &str, value: &[u8]) -> Option<()> {
        let header_line = &[name.as_bytes(), b": ", value].concat();
        self.add_line(header_line)
    }

    pub fn add_body(&mut self, body: &[u8]) -> Option<()> {
        let destination_buffer = self
            .buffer
            .get_mut(self.position..self.position + body.len())?;

        destination_buffer.copy_from_slice(body);
        self.position += body.len();

        Some(())
    }

    pub fn add_chunk(&mut self, chunk: &[u8]) -> Option<()> {
        self.add_line(chunk.len().to_string().as_bytes())?;
        self.add_line(chunk)?;
        Some(())
    }

    pub fn get_position(&self) -> usize {
        self.position
    }
}
