use crate::{ArchiveFormat, CandidateArchive};
use std::io::{self, Cursor, Read};

#[derive(Debug)]
pub struct ProbeReport {
    pub entry_count: usize,
    pub first_path: String,
    pub max_read_request: usize,
    pub peak_user_buffer: usize,
}

struct ShortReader<'a> {
    inner: Cursor<&'a [u8]>,
    limit: usize,
    pattern: [usize; 5],
    next: usize,
    max_read: usize,
}

impl Read for ShortReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let cap = self.pattern[self.next % self.pattern.len()]
            .min(self.limit)
            .min(output.len());
        self.next += 1;
        let read = self.inner.read(&mut output[..cap])?;
        self.max_read = self.max_read.max(read);
        Ok(read)
    }
}

pub fn probe_candidate(archive: &CandidateArchive, chunk: usize) -> io::Result<ProbeReport> {
    if chunk == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "chunk size must be nonzero",
        ));
    }
    let mut reader = ShortReader {
        inner: Cursor::new(archive.bytes.as_slice()),
        limit: chunk,
        pattern: [1, 7, 31, 2, 127],
        next: 0,
        max_read: 0,
    };
    let (entry_count, first_path) = match archive.format {
        ArchiveFormat::CustomXpk => parse_custom(&mut reader, chunk)?,
        ArchiveFormat::CpioNewc => parse_cpio(&mut reader, chunk)?,
        ArchiveFormat::Ustar => parse_tar(&mut reader, chunk)?,
    };
    Ok(ProbeReport {
        entry_count,
        first_path,
        max_read_request: reader.max_read,
        peak_user_buffer: chunk,
    })
}

fn read_exact_short(reader: &mut impl Read, output: &mut [u8]) -> io::Result<()> {
    reader.read_exact(output)
}

fn skip_payload(reader: &mut impl Read, mut length: u64, chunk: usize) -> io::Result<()> {
    let mut buffer = vec![0; chunk.min(4096)];
    while length != 0 {
        let take = usize::try_from(length.min(buffer.len() as u64)).unwrap();
        reader.read_exact(&mut buffer[..take])?;
        length -= take as u64;
    }
    Ok(())
}

fn parse_custom(reader: &mut impl Read, chunk: usize) -> io::Result<(usize, String)> {
    let mut header = [0; 12];
    read_exact_short(reader, &mut header)?;
    if &header[..4] != b"XPK1" {
        return Err(invalid("bad XPK magic"));
    }
    let manifest_len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64;
    let count = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if manifest_len > 64 * 1024 {
        return Err(invalid("manifest exceeds limit"));
    }
    skip_payload(reader, manifest_len, chunk)?;
    let first = ".xila/manifest".to_owned();
    for _ in 0..count {
        let mut record = [0; 10];
        read_exact_short(reader, &mut record)?;
        let name_len = u16::from_le_bytes(record[..2].try_into().unwrap()) as usize;
        let data_len = u64::from_le_bytes(record[2..].try_into().unwrap());
        if name_len == 0 || name_len > 255 {
            return Err(invalid("invalid path length"));
        }
        let mut name = vec![0; name_len];
        read_exact_short(reader, &mut name)?;
        let path = std::str::from_utf8(&name).map_err(|_| invalid("non-UTF8 path"))?;
        validate_path(path)?;
        skip_payload(reader, data_len, chunk)?;
    }
    Ok((count + 1, first))
}

fn parse_cpio(reader: &mut impl Read, chunk: usize) -> io::Result<(usize, String)> {
    let mut count = 0;
    let mut first = String::new();
    loop {
        let mut header = [0; 110];
        read_exact_short(reader, &mut header)?;
        if &header[..6] != b"070701" {
            return Err(invalid("unsupported CPIO magic"));
        }
        let file_size = hex(&header[54..62])? as u64;
        let name_size = hex(&header[94..102])? as usize;
        if name_size == 0 || name_size > 256 {
            return Err(invalid("invalid CPIO name length"));
        }
        let mut name = vec![0; name_size];
        read_exact_short(reader, &mut name)?;
        if name.pop() != Some(0) {
            return Err(invalid("unterminated CPIO name"));
        }
        let path = std::str::from_utf8(&name).map_err(|_| invalid("non-UTF8 CPIO path"))?;
        let name_pad = (4 - ((110 + name_size) % 4)) % 4;
        skip_payload(reader, name_pad as u64, chunk)?;
        if path == "TRAILER!!!" {
            break;
        }
        validate_path(path)?;
        if count == 0 {
            first = path.to_owned();
            if path != ".xila/manifest" {
                return Err(invalid("manifest is not first"));
            }
        }
        if count == 0 && file_size > 64 * 1024 {
            return Err(invalid("manifest exceeds limit"));
        }
        skip_payload(reader, file_size, chunk)?;
        let payload_pad = (4 - (file_size as usize % 4)) % 4;
        skip_payload(reader, payload_pad as u64, chunk)?;
        count += 1;
    }
    Ok((count, first))
}

fn parse_tar(reader: &mut impl Read, chunk: usize) -> io::Result<(usize, String)> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries()?;
    let mut count = 0;
    let mut first = String::new();
    let mut buffer = vec![0; chunk.min(4096)];
    for entry in entries {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        validate_path(&path)?;
        if count == 0 {
            first = path.clone();
            if path != ".xila/manifest" {
                return Err(invalid("manifest is not first"));
            }
        }
        if count == 0 && entry.size() > 64 * 1024 {
            return Err(invalid("manifest exceeds limit"));
        }
        while entry.read(&mut buffer)? != 0 {}
        count += 1;
    }
    Ok((count, first))
}

fn validate_path(path: &str) -> io::Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
    {
        return Err(invalid("unsafe path"));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> io::Result<u32> {
    u32::from_str_radix(
        std::str::from_utf8(bytes).map_err(|_| invalid("invalid CPIO number"))?,
        16,
    )
    .map_err(|_| invalid("invalid CPIO number"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
