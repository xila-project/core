use file_system::{
    AccessFlags, AttributeFlags, Attributes, BaseOperations, Context, FileSystemOperations, Kind,
    Permissions,
};
use std::{error::Error, fs, io::Read, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const BUFFER_SIZE: usize = 4096;

fn entries(root: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries = fs::read_dir(root)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

pub fn validate(root: &Path) -> Result<()> {
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err("root must be a real directory".into());
    }
    for entry in entries(root)? {
        let name = entry.file_name();
        let name = name.to_str().ok_or("file names must be UTF-8")?;
        if name.contains(['\0', '\\']) || name.len() > 255 {
            return Err("invalid file name".into());
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            validate(&entry.path())?;
        } else if !kind.is_file() {
            return Err("only regular files and directories can be imported".into());
        }
    }
    Ok(())
}

fn attributes(path: &str, kind: Kind) -> Attributes {
    let permissions = if kind == Kind::Directory || path.starts_with("/binaries/") {
        0o755
    } else {
        0o644
    };
    Attributes::new()
        .set_mask(AttributeFlags::All)
        .set_kind(kind)
        .set_permissions(Permissions::from_octal(permissions).unwrap())
}

pub fn populate(fs: &little_fs::FileSystem, root: Option<&Path>) -> Result<()> {
    fs.set_attributes("/".as_ref(), &attributes("/", Kind::Directory))?;
    if let Some(root) = root {
        visit(fs, root, "", false)?;
    }
    Ok(())
}

pub fn verify(fs: &little_fs::FileSystem, root: Option<&Path>) -> Result<()> {
    verify_attributes(fs, "/", Kind::Directory)?;
    if let Some(root) = root {
        visit(fs, root, "", true)?;
    }
    Ok(())
}

fn verify_attributes(fs: &little_fs::FileSystem, path: &str, kind: Kind) -> Result<()> {
    // Size/links are computed by the filesystem rather than stored attributes.
    let mut actual = Attributes::new().set_mask(AttributeFlags::All);
    fs.get_attributes(path.as_ref(), &mut actual)?;
    let expected = attributes(path, kind);
    if actual.get_kind() != expected.get_kind()
        || actual.get_permissions() != expected.get_permissions()
        || actual.get_user() != expected.get_user()
        || actual.get_group() != expected.get_group()
        || actual.get_inode() != expected.get_inode()
        || actual.get_creation() != expected.get_creation()
        || actual.get_modification() != expected.get_modification()
        || actual.get_access() != expected.get_access()
        || actual.get_status() != expected.get_status()
    {
        return Err(format!("metadata mismatch: {path}").into());
    }
    Ok(())
}

fn visit(fs: &little_fs::FileSystem, root: &Path, parent: &str, verify: bool) -> Result<()> {
    for entry in entries(root)? {
        let name = entry.file_name();
        let path = format!(
            "{parent}/{}",
            name.to_str().ok_or("file names must be UTF-8")?
        );
        let directory = entry.file_type()?.is_dir();
        let kind = if directory {
            Kind::Directory
        } else {
            Kind::File
        };
        if verify {
            verify_attributes(fs, &path, kind)?;
        } else {
            if directory {
                fs.create_directory(path.as_str().as_ref())?;
            } else {
                fs.create_file(path.as_str().as_ref())?;
            }
            fs.set_attributes(path.as_str().as_ref(), &attributes(&path, kind))?;
        }
        if directory {
            visit(fs, &entry.path(), &path, verify)?;
        } else {
            copy_or_verify(fs, &entry.path(), &path, verify)?;
        }
    }
    Ok(())
}

fn copy_or_verify(
    fs: &little_fs::FileSystem,
    source: &Path,
    path: &str,
    verify: bool,
) -> Result<()> {
    let mut file = fs::File::open(source)?;
    let mut context = Context::new_empty();
    fs.lookup_file(
        &mut context,
        path.as_ref(),
        if verify {
            AccessFlags::Read
        } else {
            AccessFlags::Write
        }
        .into(),
    )?;
    let result = (|| -> Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut comparison = [0; BUFFER_SIZE];
        let mut position = 0;
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let mut done = 0;
            while done < count {
                let transferred = if verify {
                    let read = fs.read(
                        &mut context,
                        &mut comparison[done..count],
                        position + done as u64,
                    )?;
                    if comparison[done..done + read] != buffer[done..done + read] {
                        return Err(format!("content mismatch: {path}").into());
                    }
                    read
                } else {
                    fs.write(&mut context, &buffer[done..count], position + done as u64)?
                };
                if transferred == 0 {
                    return Err("short filesystem I/O".into());
                }
                done += transferred;
            }
            position += count as u64;
        }
        if verify && fs.read(&mut context, &mut comparison[..1], position)? != 0 {
            return Err("file has unexpected trailing content".into());
        }
        Ok(())
    })();
    let close = fs.close(&mut context);
    result?;
    close?;
    Ok(())
}
