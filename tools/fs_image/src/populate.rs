use super::source::{Directory, Entry};
use file_system::{
    AccessFlags, AttributeFlags, Attributes, BaseOperations, Context, FileSystemOperations, Kind,
    Permissions,
};
use std::{error::Error, fs, io::Read};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const BUFFER_SIZE: usize = 4096;

pub fn validate(root: &Directory) -> Result<()> {
    for name in root.names()? {
        if name.contains(['\0', '\\']) || name.len() > 255 {
            return Err("invalid file name".into());
        }
        if let Entry::Directory(directory) = root.open_child(&name)? {
            validate(&directory)?;
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
    let mut result = Attributes::new()
        .set_mask(AttributeFlags::All)
        .set_kind(kind)
        .set_permissions(Permissions::from_octal(permissions).unwrap());
    if matches!(
        path,
        "/configuration/shared/clock"
            | "/configuration/shared/pixel_painter"
            | "/configuration/shared/text_editor"
    ) {
        result = result
            .set_user(users::UserIdentifier::new(1000))
            .set_group(users::GroupIdentifier::new(1000));
    }
    if matches!(path, "/system/users" | "/system/groups") {
        result = result.set_permissions(Permissions::from_octal(0o700).unwrap());
    }
    result
}

pub fn populate(fs: &little_fs::FileSystem, root: Option<&Directory>) -> Result<()> {
    fs.set_attributes("/".as_ref(), &attributes("/", Kind::Directory))?;
    if let Some(root) = root {
        visit(fs, root, "", false)?;
    }
    Ok(())
}

pub fn verify(fs: &little_fs::FileSystem, root: Option<&Directory>) -> Result<()> {
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

fn visit(fs: &little_fs::FileSystem, root: &Directory, parent: &str, verify: bool) -> Result<()> {
    for name in root.names()? {
        let path = format!("{parent}/{name}");
        let entry = root.open_child(&name)?;
        let kind = match &entry {
            Entry::Directory(_) => Kind::Directory,
            Entry::File(_) => Kind::File,
        };
        if verify {
            verify_attributes(fs, &path, kind)?;
        } else {
            match &entry {
                Entry::Directory(_) => fs.create_directory(path.as_str().as_ref())?,
                Entry::File(_) => fs.create_file(path.as_str().as_ref())?,
            }
            fs.set_attributes(path.as_str().as_ref(), &attributes(&path, kind))?;
        }
        match entry {
            Entry::Directory(directory) => visit(fs, &directory, &path, verify)?,
            Entry::File(file) => copy_or_verify(fs, file, &path, verify)?,
        }
    }
    Ok(())
}

fn copy_or_verify(
    fs: &little_fs::FileSystem,
    mut file: fs::File,
    path: &str,
    verify: bool,
) -> Result<()> {
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

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    #[test]
    fn rejects_a_file_replaced_by_a_symlink_after_validation() {
        let root = std::env::temp_dir().join(format!("xila-source-race-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("entry"), b"approved").unwrap();
        let source = Directory::open(&root).unwrap();
        validate(&source).unwrap();
        fs::remove_file(root.join("entry")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("entry")).unwrap();
        let filesystem = little_fs::FileSystem::new_format(
            file_system::MemoryDevice::<512>::new_static(512 * 1024),
            256,
        )
        .unwrap();
        let result = populate(&filesystem, Some(&source));
        fs::remove_dir_all(root).unwrap();
        assert!(
            result.is_err(),
            "population must not follow a link introduced after validation"
        );
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod mutation_tests {
    use super::*;
    fn temporary(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("xila-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn directory_substitution_cannot_escape_the_opened_root() {
        let root = temporary("directory-substitution");
        fs::create_dir(root.join("child")).unwrap();
        let source = Directory::open(&root).unwrap();
        validate(&source).unwrap();
        fs::remove_dir(root.join("child")).unwrap();
        std::os::unix::fs::symlink("/etc", root.join("child")).unwrap();
        let filesystem = little_fs::FileSystem::new_format(
            file_system::MemoryDevice::<512>::new_static(512 * 1024),
            256,
        )
        .unwrap();
        assert!(populate(&filesystem, Some(&source)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn root_rename_keeps_import_anchored_to_its_original_descriptor() {
        let parent = temporary("root-substitution");
        let root = parent.join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("approved"), b"original").unwrap();
        let source = Directory::open(&root).unwrap();
        validate(&source).unwrap();
        fs::rename(&root, parent.join("original")).unwrap();
        std::os::unix::fs::symlink("/etc", &root).unwrap();
        let filesystem = little_fs::FileSystem::new_format(
            file_system::MemoryDevice::<512>::new_static(512 * 1024),
            256,
        )
        .unwrap();
        populate(&filesystem, Some(&source)).unwrap();
        let mut attributes = Attributes::new().set_mask(AttributeFlags::All);
        filesystem
            .get_attributes("/approved".as_ref(), &mut attributes)
            .unwrap();
        assert_eq!(attributes.get_size(), Some(&8));
        assert!(
            filesystem
                .get_attributes("/passwd".as_ref(), &mut attributes)
                .is_err()
        );
        verify(&filesystem, Some(&source)).unwrap();
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn substituted_fifo_is_rejected_without_blocking() {
        let root = temporary("fifo-substitution");
        fs::write(root.join("entry"), b"approved").unwrap();
        let source = Directory::open(&root).unwrap();
        validate(&source).unwrap();
        fs::remove_file(root.join("entry")).unwrap();
        let name =
            std::ffi::CString::new(root.join("entry").as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(source.open_child("entry").is_err());
        });
        assert!(
            receive
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
