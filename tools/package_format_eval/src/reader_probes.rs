use crate::{ArchiveFormat, CandidateArchive};
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ProbeReport {
    pub entry_count: usize,
    pub first_path: String,
    pub max_source_read_bytes: usize,
    pub configured_chunk_bytes: usize,
}

#[derive(Debug)]
pub struct ExtractionReport {
    pub files_written: usize,
    pub bytes_written: u64,
    pub max_source_read_bytes: usize,
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

struct FileChunkReader {
    inner: File,
    limit: usize,
    max_read: usize,
}

impl Read for FileChunkReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let limit = output.len().min(self.limit);
        let read = self.inner.read(&mut output[..limit])?;
        self.max_read = self.max_read.max(read);
        Ok(read)
    }
}

pub fn extract_candidate_file_backed(
    format: ArchiveFormat,
    source_path: &Path,
    output_dir: &Path,
    chunk: usize,
    selected_path: Option<&str>,
) -> io::Result<ExtractionReport> {
    if chunk == 0 || chunk > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "chunk must be in 1..=65536",
        ));
    }
    fs::create_dir_all(output_dir)?;
    let file = File::open(source_path)?;
    let mut reader = FileChunkReader {
        inner: file,
        limit: chunk,
        max_read: 0,
    };
    let mut scratch = vec![0; chunk];
    let mut report = match format {
        ArchiveFormat::CustomXpk => {
            extract_custom(&mut reader, output_dir, selected_path, &mut scratch)?
        }
        ArchiveFormat::CpioNewc => {
            extract_cpio(&mut reader, output_dir, selected_path, &mut scratch)?
        }
        ArchiveFormat::Ustar => extract_tar(&mut reader, output_dir, selected_path, &mut scratch)?,
    };
    report.max_source_read_bytes = reader.max_read;
    Ok(report)
}

fn extract_custom(
    reader: &mut impl Read,
    output_dir: &Path,
    selected_path: Option<&str>,
    scratch: &mut [u8],
) -> io::Result<ExtractionReport> {
    let mut header = [0; 12];
    reader.read_exact(&mut header)?;
    if &header[..4] != b"XPK1" {
        return Err(invalid("bad XPK magic"));
    }
    let manifest_len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64;
    let count = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if manifest_len > 64 * 1024 {
        return Err(invalid("manifest exceeds limit"));
    }
    transfer(reader, &mut io::sink(), manifest_len, scratch)?;
    let mut report = ExtractionReport {
        files_written: 0,
        bytes_written: 0,
        max_source_read_bytes: 0,
    };
    for _ in 0..count {
        let mut record = [0; 10];
        reader.read_exact(&mut record)?;
        let name_len = u16::from_le_bytes(record[..2].try_into().unwrap()) as usize;
        let data_len = u64::from_le_bytes(record[2..].try_into().unwrap());
        if name_len == 0 || name_len > 255 {
            return Err(invalid("invalid path length"));
        }
        let mut name = vec![0; name_len];
        reader.read_exact(&mut name)?;
        let path = std::str::from_utf8(&name).map_err(|_| invalid("non-UTF8 path"))?;
        validate_path(path)?;
        if selected_path.is_some_and(|selected| selected != path) {
            transfer(reader, &mut io::sink(), data_len, scratch)?;
            continue;
        }
        let mut output = create_output(output_dir, path)?;
        transfer(reader, &mut output, data_len, scratch)?;
        report.files_written += 1;
        report.bytes_written = report
            .bytes_written
            .checked_add(data_len)
            .ok_or_else(|| invalid("written byte count overflow"))?;
        if selected_path.is_some() {
            break;
        }
    }
    Ok(report)
}

fn extract_cpio(
    reader: &mut impl Read,
    output_dir: &Path,
    selected_path: Option<&str>,
    scratch: &mut [u8],
) -> io::Result<ExtractionReport> {
    let mut report = ExtractionReport {
        files_written: 0,
        bytes_written: 0,
        max_source_read_bytes: 0,
    };
    loop {
        let mut header = [0; 110];
        reader.read_exact(&mut header)?;
        if &header[..6] != b"070701" {
            return Err(invalid("unsupported CPIO magic"));
        }
        let file_size = hex(&header[54..62])? as u64;
        let name_size = hex(&header[94..102])? as usize;
        if name_size == 0 || name_size > 256 {
            return Err(invalid("invalid CPIO name length"));
        }
        let mut name = vec![0; name_size];
        reader.read_exact(&mut name)?;
        if name.pop() != Some(0) {
            return Err(invalid("unterminated CPIO name"));
        }
        let path = std::str::from_utf8(&name).map_err(|_| invalid("non-UTF8 CPIO path"))?;
        let name_pad = (4 - ((110 + name_size) % 4)) % 4;
        transfer(reader, &mut io::sink(), name_pad as u64, scratch)?;
        if path == "TRAILER!!!" {
            break;
        }
        validate_path(path)?;
        if path == ".xila/manifest" || selected_path.is_some_and(|selected| selected != path) {
            transfer(reader, &mut io::sink(), file_size, scratch)?;
        } else {
            let mut output = create_output(output_dir, path)?;
            transfer(reader, &mut output, file_size, scratch)?;
            report.files_written += 1;
            report.bytes_written = report
                .bytes_written
                .checked_add(file_size)
                .ok_or_else(|| invalid("written byte count overflow"))?;
        }
        let payload_pad = (4 - (file_size as usize % 4)) % 4;
        transfer(reader, &mut io::sink(), payload_pad as u64, scratch)?;
        if selected_path == Some(path) {
            break;
        }
    }
    Ok(report)
}

fn extract_tar(
    reader: &mut impl Read,
    output_dir: &Path,
    selected_path: Option<&str>,
    scratch: &mut [u8],
) -> io::Result<ExtractionReport> {
    let mut archive = tar::Archive::new(reader);
    let mut report = ExtractionReport {
        files_written: 0,
        bytes_written: 0,
        max_source_read_bytes: 0,
    };
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        validate_path(&path)?;
        let is_manifest = path == ".xila/manifest";
        if is_manifest || selected_path.is_some_and(|selected| selected != path) {
            let size = entry.size();
            transfer(&mut entry, &mut io::sink(), size, scratch)?;
            continue;
        }
        let mut output = create_output(output_dir, &path)?;
        let size = entry.size();
        transfer(&mut entry, &mut output, size, scratch)?;
        report.files_written += 1;
        report.bytes_written = report
            .bytes_written
            .checked_add(size)
            .ok_or_else(|| invalid("written byte count overflow"))?;
        if selected_path.is_some() {
            break;
        }
    }
    Ok(report)
}

fn transfer(
    reader: &mut impl Read,
    writer: &mut impl Write,
    mut remaining: u64,
    scratch: &mut [u8],
) -> io::Result<()> {
    while remaining > 0 {
        let count = usize::try_from(remaining.min(scratch.len() as u64)).unwrap();
        reader.read_exact(&mut scratch[..count])?;
        writer.write_all(&scratch[..count])?;
        remaining -= count as u64;
    }
    Ok(())
}

fn create_output(root: &Path, relative: &str) -> io::Result<File> {
    validate_path(relative)?;
    let relative_path = PathBuf::from(relative);
    let destination = root.join(relative_path);
    if !destination.starts_with(root) {
        return Err(invalid("output path escapes destination"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    File::create(destination)
}

pub fn probe_candidate(archive: &CandidateArchive, chunk: usize) -> io::Result<ProbeReport> {
    probe_candidate_for_path(archive, chunk, None)
}

pub fn probe_candidate_for_path(
    archive: &CandidateArchive,
    chunk: usize,
    selected_path: Option<&str>,
) -> io::Result<ProbeReport> {
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
        ArchiveFormat::CustomXpk => parse_custom(&mut reader, chunk, selected_path)?,
        ArchiveFormat::CpioNewc => parse_cpio(&mut reader, chunk, selected_path)?,
        ArchiveFormat::Ustar => parse_tar(&mut reader, chunk, selected_path)?,
    };
    Ok(ProbeReport {
        entry_count,
        first_path,
        max_source_read_bytes: reader.max_read,
        configured_chunk_bytes: chunk,
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

fn parse_custom(
    reader: &mut impl Read,
    chunk: usize,
    selected_path: Option<&str>,
) -> io::Result<(usize, String)> {
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
    for index in 0..count {
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
        if selected_path == Some(path) {
            return Ok((index + 2, first));
        }
    }
    Ok((count + 1, first))
}

fn parse_cpio(
    reader: &mut impl Read,
    chunk: usize,
    selected_path: Option<&str>,
) -> io::Result<(usize, String)> {
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
        if selected_path == Some(path) {
            return Ok((count, first));
        }
    }
    Ok((count, first))
}

fn parse_tar(
    reader: &mut impl Read,
    chunk: usize,
    selected_path: Option<&str>,
) -> io::Result<(usize, String)> {
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
        if selected_path == Some(path.as_str()) {
            break;
        }
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
