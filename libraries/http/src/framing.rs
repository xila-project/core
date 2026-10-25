//! Sans-I/O response framing. The caller owns the head and body buffers.
//! Head parsing uses `N * size_of::<httparse::Header>()` bytes of stack.
//! Body decoding has constant storage and compacts payload in place.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidHead,
    HeadersTooLarge,
    InvalidFraming,
    Truncated,
    UnsupportedUpgrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Empty,
    Length(u64),
    Chunked,
    UntilClose,
}

#[derive(Debug, Clone, Copy)]
pub struct ResponseHead {
    pub length: usize,
    pub status: u16,
    pub body: BodyKind,
}

/// Parse a contiguous head without allocating. `None` requests more bytes.
/// The caller must bound the input buffer and handle informational responses.
pub fn parse_response_head<const N: usize>(
    input: &[u8],
    head_request: bool,
) -> Result<Option<ResponseHead>, Error> {
    let mut headers = [httparse::EMPTY_HEADER; N];
    let mut response = httparse::Response::new(&mut headers);
    let length = match response.parse(input) {
        Ok(httparse::Status::Partial) => return Ok(None),
        Ok(httparse::Status::Complete(n)) => n,
        Err(httparse::Error::TooManyHeaders) => return Err(Error::HeadersTooLarge),
        Err(_) => return Err(Error::InvalidHead),
    };
    let status = response.code.ok_or(Error::InvalidHead)?;
    if status == 101 {
        return Err(Error::UnsupportedUpgrade);
    }
    if head_request || (100..200).contains(&status) || status == 204 || status == 304 {
        return Ok(Some(ResponseHead {
            length,
            status,
            body: BodyKind::Empty,
        }));
    }
    let mut content_length = None;
    let mut transfer = false;
    let mut chunked = false;
    for header in response.headers.iter() {
        if header.name.eq_ignore_ascii_case("Transfer-Encoding") {
            transfer = true;
            let value = core::str::from_utf8(header.value).map_err(|_| Error::InvalidFraming)?;
            for coding in value.split(',') {
                if chunked || coding.trim().is_empty() {
                    return Err(Error::InvalidFraming);
                }
                chunked = coding.trim().eq_ignore_ascii_case("chunked");
            }
        }
    }
    if !transfer {
        for header in response.headers.iter() {
            if header.name.eq_ignore_ascii_case("Content-Length") {
                for value in header.value.split(|b| *b == b',') {
                    let value = core::str::from_utf8(value)
                        .map_err(|_| Error::InvalidFraming)?
                        .trim();
                    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(Error::InvalidFraming);
                    }
                    let value = value.parse::<u64>().map_err(|_| Error::InvalidFraming)?;
                    if content_length.is_some_and(|previous| previous != value) {
                        return Err(Error::InvalidFraming);
                    }
                    content_length = Some(value);
                }
            }
        }
    }
    let body = if chunked {
        BodyKind::Chunked
    } else if transfer {
        BodyKind::UntilClose
    } else if let Some(n) = content_length {
        BodyKind::Length(n)
    } else {
        BodyKind::UntilClose
    };
    Ok(Some(ResponseHead {
        length,
        status,
        body,
    }))
}

#[derive(Debug, Clone, Copy)]
enum ChunkState {
    Size,
    SizeWhitespace,
    Extension,
    SizeLf,
    Data(u64),
    DataCr,
    DataLf,
    Trailer,
    TrailerLf,
    Done,
}

pub struct BodyDecoder {
    kind: BodyKind,
    state: ChunkState,
    // At most 16 hexadecimal digits plus CRLF for httparse.
    digits: [u8; 18],
    digits_len: u8,
    line_len: u16,
}

impl BodyDecoder {
    pub fn new(kind: BodyKind) -> Self {
        Self {
            kind,
            state: ChunkState::Size,
            digits: [0; 18],
            digits_len: 0,
            line_len: 0,
        }
    }
    pub fn is_complete(&self) -> bool {
        matches!(self.kind, BodyKind::Empty | BodyKind::Length(0))
            || matches!(self.state, ChunkState::Done)
    }
    pub fn finish(&self) -> Result<(), Error> {
        if self.is_complete() || self.kind == BodyKind::UntilClose {
            Ok(())
        } else {
            Err(Error::Truncated)
        }
    }
    /// Consume all input, replacing it with decoded payload. Bytes beyond the
    /// framed message are ignored (this client does not pipeline responses).
    pub fn decode(&mut self, input: &mut [u8]) -> Result<usize, Error> {
        match &mut self.kind {
            BodyKind::Empty => return Ok(0),
            BodyKind::UntilClose => return Ok(input.len()),
            BodyKind::Length(remaining) => {
                let n = (*remaining).min(input.len() as u64) as usize;
                *remaining -= n as u64;
                return Ok(n);
            }
            BodyKind::Chunked => {}
        }
        let mut output = 0;
        for i in 0..input.len() {
            let byte = input[i];
            match self.state {
                ChunkState::Size => {
                    if byte.is_ascii_hexdigit() {
                        if self.digits_len == 16 {
                            return Err(Error::InvalidFraming);
                        }
                        self.digits[self.digits_len as usize] = byte;
                        self.digits_len += 1;
                    } else if matches!(byte, b' ' | b'\t') && self.digits_len > 0 {
                        self.state = ChunkState::SizeWhitespace;
                    } else if byte == b';' && self.digits_len > 0 {
                        self.state = ChunkState::Extension;
                    } else if byte == b'\r' && self.digits_len > 0 {
                        self.state = ChunkState::SizeLf;
                    } else {
                        return Err(Error::InvalidFraming);
                    }
                }
                ChunkState::SizeWhitespace => {
                    if byte == b';' {
                        self.state = ChunkState::Extension;
                    } else if !matches!(byte, b' ' | b'\t') {
                        return Err(Error::InvalidFraming);
                    }
                }
                ChunkState::Extension => {
                    self.line_len = self.line_len.checked_add(1).ok_or(Error::InvalidFraming)?;
                    if byte == b'\r' {
                        self.state = ChunkState::SizeLf;
                    } else if byte == b'\n' || byte < 0x20 && byte != b'\t' {
                        return Err(Error::InvalidFraming);
                    }
                }
                ChunkState::SizeLf => {
                    if byte != b'\n' {
                        return Err(Error::InvalidFraming);
                    }
                    let n = self.digits_len as usize;
                    self.digits[n..n + 2].copy_from_slice(b"\r\n");
                    let size = match httparse::parse_chunk_size(&self.digits[..n + 2]) {
                        Ok(httparse::Status::Complete((_, size))) => size,
                        _ => return Err(Error::InvalidFraming),
                    };
                    self.digits_len = 0;
                    self.line_len = 0;
                    self.state = if size == 0 {
                        ChunkState::Trailer
                    } else {
                        ChunkState::Data(size)
                    };
                }
                ChunkState::Data(remaining) => {
                    input[output] = byte;
                    output += 1;
                    self.state = if remaining == 1 {
                        ChunkState::DataCr
                    } else {
                        ChunkState::Data(remaining - 1)
                    };
                }
                ChunkState::DataCr => {
                    if byte != b'\r' {
                        return Err(Error::InvalidFraming);
                    }
                    self.state = ChunkState::DataLf;
                }
                ChunkState::DataLf => {
                    if byte != b'\n' {
                        return Err(Error::InvalidFraming);
                    }
                    self.state = ChunkState::Size;
                }
                ChunkState::Trailer => {
                    if byte == b'\r' {
                        self.state = ChunkState::TrailerLf;
                    } else if byte == b'\n' {
                        return Err(Error::InvalidFraming);
                    } else {
                        self.line_len =
                            self.line_len.checked_add(1).ok_or(Error::InvalidFraming)?;
                    }
                }
                ChunkState::TrailerLf => {
                    if byte != b'\n' {
                        return Err(Error::InvalidFraming);
                    }
                    self.state = if self.line_len == 0 {
                        ChunkState::Done
                    } else {
                        ChunkState::Trailer
                    };
                    self.line_len = 0;
                }
                ChunkState::Done => break,
            }
        }
        Ok(output)
    }
}
