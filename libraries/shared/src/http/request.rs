use crate::{get_body, parse_header, split_lines};

#[derive(Debug)]
pub struct HttpRequestBuilder<'a> {
    buffer: &'a mut [u8],
    position: usize,
}

impl<'a> HttpRequestBuilder<'a> {
    pub const LINE_ENDING: &'static [u8] = b"\r\n";
    pub const HTTP_VERSION_1_1: &'static str = "HTTP/1.1";

    pub fn from_buffer(buffer: &'a mut [u8]) -> Self {
        buffer.fill(0);

        HttpRequestBuilder {
            buffer,
            position: 0,
        }
    }

    pub fn into_buffer(self) -> &'a mut [u8] {
        self.buffer
    }

    fn add_line_iterator<'b, I>(&mut self, iter: I) -> Option<&mut Self>
    where
        I: Iterator<Item = &'b [u8]>,
    {
        self.position = crate::add_line_iterator(self.buffer, self.position, iter)?;
        Some(self)
    }

    fn add_line(&mut self, line: &[u8]) -> Option<&mut Self> {
        self.position = crate::add_line(self.buffer, self.position, line)?;
        Some(self)
    }

    pub fn add_request(
        &mut self,
        method: &str,
        path: &str,
        http_version: &str,
    ) -> Option<&mut Self> {
        let request_line = [
            method.as_bytes(),
            b" ",
            path.as_bytes(),
            b" ",
            http_version.as_bytes(),
        ];
        self.add_line_iterator(request_line.into_iter())?;
        Some(self)
    }

    pub fn add_header(&mut self, name: &str, value: &[u8]) -> Option<&mut Self> {
        let header_line = [name.as_bytes(), b": ", value].concat();
        self.add_line(&header_line)
    }

    pub fn add_body(&mut self, body: &[u8]) -> Option<&mut Self> {
        self.add_line(b"")?;

        let destination_buffer = self
            .buffer
            .get_mut(self.position..self.position + body.len())?;

        destination_buffer.copy_from_slice(body);

        self.position += body.len();

        Some(self)
    }

    pub fn get_position(&self) -> usize {
        self.position
    }
}

pub struct HttpRequestParser<'a>(&'a [u8]);

impl<'a> HttpRequestParser<'a> {
    pub const HOST_HEADER: &'static str = "Host";
    pub const CONTENT_LENGTH_HEADER: &'static str = "Content-Length";
    pub const CONTENT_TYPE_HEADER: &'static str = "Content-Type";
    pub const USER_AGENT_HEADER: &'static str = "User-Agent";
    pub const CONNECTION_HEADER: &'static str = "Connection";
    pub const ACCEPT_HEADER: &'static str = "Accept";
    pub const ACCEPT_ENCODING_HEADER: &'static str = "Accept-Encoding";
    pub const ACCEPT_LANGUAGE_HEADER: &'static str = "Accept-Language";

    pub fn split_lines(&self) -> impl Iterator<Item = &'a [u8]> + Clone + 'a {
        split_lines(self.0)
    }

    pub fn from_buffer(buffer: &'a [u8]) -> Self {
        HttpRequestParser(buffer)
    }

    pub fn get_request(&self) -> Option<(&'a str, &'a str)> {
        let mut lines = self.split_lines();

        let request_line = lines.next()?;

        let mut parts = request_line.splitn(3, |&b| b == b' ');

        let method = str::from_utf8(parts.next()?).ok()?;
        let path = str::from_utf8(parts.next()?).ok()?;

        Some((method, path))
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
