use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fixture {
    pub path: String,
    pub data: Vec<u8>,
}

/// Build a reproducible package corpus from a deterministic seed.
pub fn fixtures(seed: u64) -> io::Result<Vec<Fixture>> {
    let mut rng = SeededBytes(seed);
    let mut result = Vec::with_capacity(108);

    let mut wasm = vec![0; 1_048_576];
    wasm[..8].copy_from_slice(b"\0asm\x01\0\0\0");
    rng.fill(&mut wasm[8..]);
    result.push(Fixture {
        path: "bin/application.wasm".into(),
        data: wasm,
    });

    result.push(Fixture {
        path: "etc/application.toml".into(),
        data: b"name = \"evaluation\"\nversion = \"1.0.0\"\n".to_vec(),
    });

    let mut opaque_image = vec![0; 32_768];
    opaque_image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    rng.fill(&mut opaque_image[8..]);
    result.push(Fixture {
        path: "share/icon.png".into(),
        data: opaque_image,
    });

    result.push(Fixture {
        path: "share/nested/deeper/readme.txt".into(),
        data: b"Nested package resource.\n".to_vec(),
    });

    for index in 0..100 {
        let mut data = vec![0; 32];
        rng.fill(&mut data);
        result.push(Fixture {
            path: format!("share/tiny/file-{index:03}.dat"),
            data,
        });
    }

    result.push(Fixture {
        path: format!("share/boundary/{}.boundary.txt", "a".repeat(180)),
        data: b"boundary path fixture\n".to_vec(),
    });

    Ok(result)
}

/// Write fixture files only below `output_dir`.
pub fn write_fixtures(output_dir: &Path, seed: u64) -> io::Result<PathBuf> {
    if output_dir.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output directory is empty",
        ));
    }

    fs::create_dir_all(output_dir)?;
    let output_dir = output_dir.canonicalize()?;
    if output_dir == std::env::current_dir()?.canonicalize()? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to write corpus into the current directory",
        ));
    }

    for fixture in fixtures(seed)? {
        let relative = Path::new(&fixture.path);
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixture path is not relative",
            ));
        }
        let destination = output_dir.join(relative);
        if !destination.starts_with(&output_dir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixture escapes output directory",
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
            if !parent.canonicalize()?.starts_with(&output_dir) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "fixture parent escapes output directory",
                ));
            }
        }
        if fs::symlink_metadata(&destination)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to overwrite a symlink",
            ));
        }
        fs::write(destination, fixture.data)?;
    }
    Ok(output_dir)
}

struct SeededBytes(u64);

impl SeededBytes {
    fn fill(&mut self, target: &mut [u8]) {
        for byte in target {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            *byte = (self.0 >> 32) as u8;
        }
    }
}
