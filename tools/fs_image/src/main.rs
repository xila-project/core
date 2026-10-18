mod bundle;
mod image;
mod populate;
use std::{env, error::Error, path::PathBuf, process};

const DEFAULT_SIZE: u64 = 16 * 1024 * 1024;

fn main() {
    drivers_std::memory::instantiate_global_allocator!();
    // Pull the supported ABI memory exports into the host executable. LittleFS
    // calls these C ABI symbols from its allocator hooks.
    std::hint::black_box(abi_definitions::xila_memory_allocate as *const ());
    std::hint::black_box(abi_definitions::xila_memory_deallocate as *const ());
    std::hint::black_box(&abi_definitions::XILA_MEMORY_CAPABILITIES_NONE as *const _ as usize);
    if let Err(error) = run() {
        eprintln!("fs_image: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1).peekable();
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: fs_image <output-image> [size-bytes] (size defaults to 16777216)")?;
    let size = if arguments
        .peek()
        .is_some_and(|value| !value.to_string_lossy().starts_with("--"))
    {
        arguments.next().unwrap().to_string_lossy().parse::<u64>()?
    } else {
        DEFAULT_SIZE
    };
    let mut root = None;
    let mut bundle = None;
    let mut applications = None;
    while let Some(flag) = arguments.next() {
        match flag.to_str() {
            Some("--root") => {
                root = Some(PathBuf::from(
                    arguments.next().ok_or("--root requires a directory")?,
                ))
            }
            Some("--bundle") => {
                bundle = Some(arguments.next().ok_or("--bundle requires native or wasm")?)
            }
            Some("--applications-dir") => {
                applications = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or("--applications-dir requires a directory")?,
                ))
            }
            _ => return Err("unknown argument".into()),
        }
    }
    let staging = match (bundle, applications) {
        (Some(profile), Some(directory)) if root.is_none() => Some(bundle::prepare(
            profile.to_str().ok_or("invalid bundle")?,
            &directory,
        )?),
        (None, None) => None,
        _ => {
            return Err("use --root or --bundle native|wasm --applications-dir <directory>".into());
        }
    };
    let root = staging.as_ref().map(|s| s.0.as_path()).or(root.as_deref());
    let output = output.canonicalize_parent()?;
    image::build(&output, size, root)?;

    println!(
        "Verified Xila MBR, LittleFS contents and metadata: {} ({} bytes)",
        output.display(),
        size
    );
    Ok(())
}

fn create_output_file(path: &std::path::Path, size: u64) -> std::io::Result<()> {
    if path.to_str().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "output image path must be valid UTF-8",
        ));
    }
    // Exclusive creation prevents accidental replacement of an existing image.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .set_len(size)
}

trait CanonicalizeParent {
    fn canonicalize_parent(&self) -> Result<PathBuf, Box<dyn Error>>;
}

impl CanonicalizeParent for PathBuf {
    fn canonicalize_parent(&self) -> Result<PathBuf, Box<dyn Error>> {
        let parent = self
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let parent = parent.canonicalize()?;
        Ok(parent.join(self.file_name().ok_or("output path must name a file")?))
    }
}

#[cfg(test)]
mod tests {
    use super::create_output_file;
    use std::{fs, io::Write};

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_output_before_creating_file() {
        use std::os::unix::ffi::OsStringExt;

        let mut name = format!(
            "fs-image-invalid-{}-{}.img",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
        .into_bytes();
        name.push(0xff);
        let path = std::env::temp_dir().join(std::ffi::OsString::from_vec(name));

        let result = create_output_file(&path, 16 * 1024 * 1024);

        let error = result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("valid UTF-8"));
        assert!(!path.exists());
    }

    #[test]
    fn refuses_existing_output_without_modifying_it() {
        let path = std::env::temp_dir().join(format!(
            "fs-image-existing-{}-{}.img",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut existing = fs::File::create(&path).unwrap();
        existing.write_all(b"keep this installation").unwrap();
        drop(existing);

        let result = create_output_file(&path, 16 * 1024 * 1024);

        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"keep this installation");
        fs::remove_file(path).unwrap();
    }
}
