use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const PAGE_BYTES: usize = 4096;
pub const DEFAULT_PATH: &str = "/configuration/shared/text_editor/untitled.txt";
const IO_BYTES: usize = 4096;

pub struct WorkingCopy {
    file: Option<File>,
    path: PathBuf,
    length: u64,
    allocated_length: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub length: usize,
}

impl WorkingCopy {
    pub fn open(source: impl AsRef<Path>) -> io::Result<Self> {
        let source = source.as_ref();
        let parent = source
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("/"));
        let (path, mut copy) = Self::create_file(parent)?;
        let result = (|| {
            let mut input = File::open(source)?;
            let mut buffer = [0; IO_BYTES];
            let mut carry_len = 0;
            let mut length = 0u64;
            loop {
                let read = input.read(&mut buffer[carry_len..])?;
                let available = carry_len + read;
                let final_chunk = read == 0;
                let valid_len = match std::str::from_utf8(&buffer[..available]) {
                    Ok(text) => {
                        if text.as_bytes().contains(&0) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "NUL bytes are not supported",
                            ));
                        }
                        available
                    }
                    Err(error) if error.error_len().is_none() && !final_chunk => {
                        let valid = error.valid_up_to();
                        if buffer[..valid].contains(&0) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "NUL bytes are not supported",
                            ));
                        }
                        valid
                    }
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "file is not valid UTF-8",
                        ));
                    }
                };
                copy.write_all(&buffer[..valid_len])?;
                length = length
                    .checked_add(valid_len as u64)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "file too large"))?;
                if final_chunk {
                    break;
                }
                carry_len = available - valid_len;
                buffer.copy_within(valid_len..available, 0);
            }
            copy.flush()?;
            Ok(length)
        })();
        match result {
            Ok(length) => Ok(Self {
                file: Some(copy),
                path,
                length,
                allocated_length: length,
            }),
            Err(error) => {
                drop(copy);
                let _ = fs::remove_file(path);
                Err(error)
            }
        }
    }

    pub fn create(parent: impl AsRef<Path>) -> io::Result<Self> {
        let (path, file) = Self::create_file(parent.as_ref())?;
        Ok(Self {
            file: Some(file),
            path,
            length: 0,
            allocated_length: 0,
        })
    }

    fn create_file(parent: &Path) -> io::Result<(PathBuf, File)> {
        for sequence in 0..32 {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = parent.join(format!(".xila-text-editor-{timestamp}-{sequence}"));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique working copy",
        ))
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("working copy file is open")
    }

    pub fn read_page(&mut self, offset: u64, output: &mut [u8; PAGE_BYTES]) -> io::Result<Page> {
        if offset > self.length || !self.is_boundary(offset)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "page offset is not a UTF-8 boundary",
            ));
        }
        self.file().seek(SeekFrom::Start(offset))?;
        let wanted = usize::try_from((self.length - offset).min(PAGE_BYTES as u64)).unwrap();
        let mut total = 0;
        while total < wanted {
            let count = self.file().read(&mut output[total..wanted])?;
            if count == 0 {
                break;
            }
            total += count;
        }
        let valid = match std::str::from_utf8(&output[..total]) {
            Ok(_) => total,
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "working copy is not valid UTF-8",
                ));
            }
        };
        Ok(Page { length: valid })
    }

    pub fn previous_page_start(&mut self, offset: u64) -> io::Result<u64> {
        if offset == 0 || offset > self.length {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "page offset is out of range",
            ));
        }
        let earliest = offset.saturating_sub(PAGE_BYTES as u64);
        let latest = offset.min(earliest.saturating_add(3));
        let mut page = [0; PAGE_BYTES];
        for start in earliest..=latest {
            if self.is_boundary(start)? {
                let current = self.read_page(start, &mut page)?;
                if start + current.length as u64 == offset {
                    return Ok(start);
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offset is not a page boundary",
        ))
    }

    pub fn replace_page(
        &mut self,
        offset: u64,
        old_length: usize,
        replacement: &str,
    ) -> io::Result<u64> {
        let old_end = offset.checked_add(old_length as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "page end is out of range")
        })?;
        if replacement.len() > PAGE_BYTES
            || replacement.contains('\0')
            || !self.is_boundary(offset)?
            || old_end > self.length
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid page replacement",
            ));
        }
        let new_total = self
            .length
            .checked_sub(old_length as u64)
            .and_then(|length| length.checked_add(replacement.len() as u64))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "document is too large"))?;
        if new_total > self.allocated_length {
            self.file().seek(SeekFrom::Start(new_total - 1))?;
            self.file().write_all(&[0])?;
            self.allocated_length = new_total;
        }
        if !self.is_boundary(old_end)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "page end is not a UTF-8 boundary",
            ));
        }
        let new_length = replacement.len() as u64;
        let tail_len = self.length - old_end;
        let mut buffer = [0; IO_BYTES];
        if new_length > old_length as u64 {
            let mut remaining = tail_len;
            while remaining > 0 {
                let count = remaining.min(IO_BYTES as u64) as usize;
                let source = old_end + remaining - count as u64;
                self.file().seek(SeekFrom::Start(source))?;
                self.file().read_exact(&mut buffer[..count])?;
                self.file()
                    .seek(SeekFrom::Start(source + new_length - old_length as u64))?;
                self.file().write_all(&buffer[..count])?;
                remaining -= count as u64;
            }
        } else if new_length < old_length as u64 {
            let mut moved = 0;
            while moved < tail_len {
                let count = (tail_len - moved).min(IO_BYTES as u64) as usize;
                self.file().seek(SeekFrom::Start(old_end + moved))?;
                self.file().read_exact(&mut buffer[..count])?;
                self.file()
                    .seek(SeekFrom::Start(offset + new_length + moved))?;
                self.file().write_all(&buffer[..count])?;
                moved += count as u64;
            }
        }
        self.file().seek(SeekFrom::Start(offset))?;
        self.file().write_all(replacement.as_bytes())?;
        self.length = new_total;
        self.file().flush()?;
        Ok(self.length)
    }

    pub fn save_to(&mut self, target: impl AsRef<Path>) -> io::Result<()> {
        self.file().flush()?;
        self.file().seek(SeekFrom::Start(0))?;
        let mut output = File::create(target)?;
        let mut remaining = self.length;
        let mut buffer = [0; IO_BYTES];
        while remaining > 0 {
            let count = remaining.min(IO_BYTES as u64) as usize;
            self.file().read_exact(&mut buffer[..count])?;
            output.write_all(&buffer[..count])?;
            remaining -= count as u64;
        }
        output.flush()
    }

    fn is_boundary(&mut self, offset: u64) -> io::Result<bool> {
        if offset == 0 || offset == self.length {
            return Ok(true);
        }
        self.file().seek(SeekFrom::Start(offset))?;
        let mut byte = [0];
        self.file().read_exact(&mut byte)?;
        Ok(byte[0] & 0b1100_0000 != 0b1000_0000)
    }
}

impl Drop for WorkingCopy {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for sequence in 0..32 {
            let path = std::env::temp_dir().join(format!(
                "xila-text-editor-{}-{timestamp}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create test directory: {error}"),
            }
        }
        panic!("could not create a unique test directory")
    }

    #[test]
    fn working_copy_pages_and_saves_without_loading_the_document() {
        let dir = directory();
        let source = dir.join("source.txt");
        let contents = format!(
            "{}é{}",
            "a".repeat(PAGE_BYTES - 1),
            "b".repeat(PAGE_BYTES * 3)
        );
        fs::write(&source, &contents).unwrap();
        let mut copy = WorkingCopy::open(&source).unwrap();
        let mut page = [0; PAGE_BYTES];
        let first = copy.read_page(0, &mut page).unwrap();
        assert_eq!(first.length, PAGE_BYTES - 1);
        assert!(copy.length() > first.length as u64);
        assert_eq!(copy.previous_page_start(first.length as u64).unwrap(), 0);
        copy.replace_page(0, first.length, "short").unwrap();
        let second = copy.read_page(0, &mut page).unwrap();
        assert!(
            std::str::from_utf8(&page[..second.length])
                .unwrap()
                .starts_with("shorté")
        );
        let saved = dir.join("saved.txt");
        copy.save_to(&saved).unwrap();
        assert_eq!(
            fs::read(saved).unwrap(),
            format!("shorté{}", "b".repeat(PAGE_BYTES * 3)).as_bytes()
        );
        drop(copy);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn page_replacement_moves_large_tail_in_both_directions() {
        let dir = directory();
        let source = dir.join("source.txt");
        let tail = "z".repeat(IO_BYTES * 3 + 17);
        fs::write(&source, format!("head\n{tail}")).unwrap();
        let mut copy = WorkingCopy::open(&source).unwrap();
        copy.replace_page(0, 5, &"a".repeat(PAGE_BYTES)).unwrap();
        let expanded = dir.join("expanded.txt");
        copy.save_to(&expanded).unwrap();
        assert_eq!(
            fs::read(&expanded).unwrap(),
            format!("{}{tail}", "a".repeat(PAGE_BYTES)).as_bytes()
        );
        copy.replace_page(0, PAGE_BYTES, "x").unwrap();
        let contracted = dir.join("contracted.txt");
        copy.save_to(&contracted).unwrap();
        assert_eq!(
            fs::read(&contracted).unwrap(),
            format!("x{tail}").as_bytes()
        );
        copy.replace_page(0, 1, &"y".repeat(PAGE_BYTES)).unwrap();
        let regrown = dir.join("regrown.txt");
        copy.save_to(&regrown).unwrap();
        assert_eq!(
            fs::read(&regrown).unwrap(),
            format!("{}{tail}", "y".repeat(PAGE_BYTES)).as_bytes()
        );
        drop(copy);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_utf8_and_nul_are_rejected_and_temporary_copy_is_removed() {
        let dir = directory();
        for data in [&b"\xff"[..], &b"a\0b"[..]] {
            let source = dir.join("invalid.txt");
            fs::write(&source, data).unwrap();
            assert!(WorkingCopy::open(&source).is_err());
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
