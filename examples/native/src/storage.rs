//! Exclusive first-use reference-disk import. Never format an existing disk.
use drivers_std::drive_file::FileDriveDevice;
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn prepare_disk(disk: &Path, seed: &Path, size: u64) -> io::Result<bool> {
    match fs::symlink_metadata(disk) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.len() != size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "existing disk is not a regular image of the requested size; it was not modified",
                ));
            }
            return Ok(false);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut source = fs::File::open(seed).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                error.kind(),
                format!(
                    "reference disk image '{}' was not found; create it with `cargo make prepare-native-image` or pass an existing image with --seed-image",
                    seed.display()
                ),
            )
        } else {
            error
        }
    })?;
    if !source.metadata()?.is_file() || source.metadata()?.len() != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "seed image has the wrong size",
        ));
    }
    let parent = disk
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (temporary, mut output) = loop {
        let path = parent.join(format!(
            ".xila-import-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => break (path, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        if io::copy(&mut source, &mut output)? != size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "seed changed size during import",
            ));
        }
        output.sync_all()?;
        let drive = Box::leak(Box::new(FileDriveDevice::open_existing(&temporary)?));
        let fs = xila::little_fs::mount_disk(drive, 256).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid seed filesystem: {e:?}"),
            )
        })?;
        drop(fs);
        // A hard link publishes complete bytes atomically and refuses replacement,
        // including if another process created the destination during validation.
        fs::hard_link(&temporary, disk)?;
        Ok(true)
    })();
    drop(output);
    let cleanup = fs::remove_file(&temporary);
    match result {
        Ok(imported) => {
            cleanup?;
            Ok(imported)
        }
        Err(error) => {
            let _ = cleanup;
            Err(error)
        }
    }
}
