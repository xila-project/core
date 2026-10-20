use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
mod reader_probes;
pub use reader_probes::{ProbeReport, probe_candidate};

const BLOCK_SIZE: usize = 4096;
const MANIFEST_PATH: &str = ".xila/manifest";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fixture {
    pub path: String,
    pub data: Vec<u8>,
}

pub fn cpio_entry_bytes(path_bytes: usize, stored_bytes: usize) -> Option<usize> {
    let name_end = align(110usize.checked_add(path_bytes.checked_add(1)?)?, 4)?;
    let name = name_end.checked_sub(110)?;
    let payload = align(stored_bytes, 4)?;
    110usize.checked_add(name)?.checked_add(payload)
}

pub fn tar_entry_bytes(stored_bytes: usize) -> Option<usize> {
    512usize.checked_add(align(stored_bytes, 512)?)
}

pub fn tar_archive_bytes<const N: usize>(entries: [usize; N]) -> Option<usize> {
    entries.into_iter().try_fold(1024usize, |total, stored| {
        total.checked_add(tar_entry_bytes(stored)?)
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveFormat {
    CpioNewc,
    Ustar,
    CustomXpk,
}

impl ArchiveFormat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::CpioNewc => "cpio-newc",
            Self::Ustar => "ustar",
            Self::CustomXpk => "custom-xpk",
        }
    }

    pub const fn filename(self) -> &'static str {
        match self {
            Self::CpioNewc => "package.cpio",
            Self::Ustar => "package.tar",
            Self::CustomXpk => "package.xpk",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateArchive {
    pub format: ArchiveFormat,
    pub bytes: Vec<u8>,
    pub manifest_bytes: usize,
    pub name_bytes: usize,
    pub header_bytes: usize,
    pub padding_bytes: usize,
    pub compression_frame_bytes: usize,
    pub stored_payload_bytes: usize,
    pub compressed_files: usize,
    pub uncompressed_files: usize,
}

#[derive(Serialize)]
struct Manifest<'a> {
    version: u8,
    files: Vec<ManifestFile<'a>>,
}

#[derive(Serialize)]
struct ManifestFile<'a> {
    path: &'a str,
    stored_size: u64,
    expanded_size: u64,
    compression: Compression,
}

#[derive(Clone, Copy, Serialize)]
enum Compression {
    None,
    Lz4Blocks,
}

struct EncodedFile<'a> {
    fixture: &'a Fixture,
    data: Vec<u8>,
    compression: Compression,
    frame_bytes: usize,
}

pub fn build_candidate_archives(fixtures: &[Fixture]) -> io::Result<Vec<CandidateArchive>> {
    let encoded_files = encode_files(fixtures)?;
    let manifest = postcard::to_allocvec(&Manifest {
        version: 1,
        files: encoded_files
            .iter()
            .map(|file| ManifestFile {
                path: &file.fixture.path,
                stored_size: file.data.len() as u64,
                expanded_size: file.fixture.data.len() as u64,
                compression: file.compression,
            })
            .collect(),
    })
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    Ok(vec![
        build_cpio(&manifest, &encoded_files)?,
        build_tar(&manifest, &encoded_files)?,
        build_custom(&manifest, &encoded_files)?,
    ])
}

pub fn encode_file_for_test(data: &[u8]) -> io::Result<(Vec<u8>, bool, usize)> {
    let fixture = Fixture {
        path: "probe.bin".into(),
        data: data.to_vec(),
    };
    let encoded = encode_files(std::slice::from_ref(&fixture))?.remove(0);
    Ok((
        encoded.data,
        matches!(encoded.compression, Compression::Lz4Blocks),
        encoded.frame_bytes,
    ))
}

fn encode_files(fixtures: &[Fixture]) -> io::Result<Vec<EncodedFile<'_>>> {
    fixtures
        .iter()
        .map(|fixture| {
            let mut blocks = Vec::new();
            for block in fixture.data.chunks(BLOCK_SIZE) {
                let compressed = lz4_flex::block::compress(block);
                let compressed_len = u32::try_from(compressed.len()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "LZ4 block exceeds u32")
                })?;
                let expanded_len = u32::try_from(block.len()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "expanded LZ4 block exceeds u32")
                })?;
                blocks.extend_from_slice(&compressed_len.to_le_bytes());
                blocks.extend_from_slice(&expanded_len.to_le_bytes());
                blocks.extend_from_slice(&compressed);
            }
            if blocks.len() < fixture.data.len() {
                Ok(EncodedFile {
                    fixture,
                    frame_bytes: fixture.data.chunks(BLOCK_SIZE).count() * 8,
                    data: blocks,
                    compression: Compression::Lz4Blocks,
                })
            } else {
                Ok(EncodedFile {
                    fixture,
                    data: fixture.data.clone(),
                    compression: Compression::None,
                    frame_bytes: 0,
                })
            }
        })
        .collect()
}

fn build_cpio(manifest: &[u8], files: &[EncodedFile<'_>]) -> io::Result<CandidateArchive> {
    use hadris_cpio::sync::{CpioArchiveWriter, CpioWriteOptions, FileNode, FileTree};

    let mut tree = FileTree::new();
    tree.add(FileNode::file(MANIFEST_PATH, manifest.to_vec(), 0o644));
    for file in files {
        tree.add(FileNode::file(&file.fixture.path, file.data.clone(), 0o644));
    }
    let bytes = CpioArchiveWriter::create(Vec::new(), &tree, CpioWriteOptions::default())
        .map_err(|error| io::Error::other(format!("hadris-cpio writer: {error}")))?;
    let name_bytes = cpio_name_field_bytes(MANIFEST_PATH.len())
        + files
            .iter()
            .map(|file| cpio_name_field_bytes(file.fixture.path.len()))
            .sum::<usize>()
        + cpio_name_field_bytes(b"TRAILER!!!".len());
    let padding_bytes = (align(manifest.len(), 4).unwrap() - manifest.len())
        + files
            .iter()
            .map(|file| align(file.data.len(), 4).unwrap() - file.data.len())
            .sum::<usize>();
    let header_bytes = (files.len() + 2) * 110;
    let compression_frame_bytes = files.iter().map(|file| file.frame_bytes).sum();
    let stored_payload_bytes = files
        .iter()
        .map(|file| file.data.len() - file.frame_bytes)
        .sum();
    let (compressed_files, uncompressed_files) = file_counts(files);
    Ok(CandidateArchive {
        format: ArchiveFormat::CpioNewc,
        bytes,
        manifest_bytes: manifest.len(),
        name_bytes,
        header_bytes,
        padding_bytes,
        compression_frame_bytes,
        stored_payload_bytes,
        compressed_files,
        uncompressed_files,
    })
}

fn build_tar(manifest: &[u8], files: &[EncodedFile<'_>]) -> io::Result<CandidateArchive> {
    let mut builder = tar::Builder::new(Vec::new());
    append_tar_entry(&mut builder, MANIFEST_PATH, manifest)?;
    for file in files {
        append_tar_entry(&mut builder, &file.fixture.path, &file.data)?;
    }
    builder.finish()?;
    let bytes = builder.into_inner()?;
    let all_entries = std::iter::once((MANIFEST_PATH, manifest.len())).chain(
        files
            .iter()
            .map(|file| (file.fixture.path.as_str(), file.data.len())),
    );
    let mut name_bytes = 0;
    let mut header_bytes = 1024;
    let mut padding_bytes = 0;
    for (path, size) in all_entries {
        name_bytes += path.len();
        header_bytes += 512 - path.len();
        padding_bytes += align(size, 512).unwrap() - size;
    }
    let compression_frame_bytes = files.iter().map(|file| file.frame_bytes).sum();
    let stored_payload_bytes = files
        .iter()
        .map(|file| file.data.len() - file.frame_bytes)
        .sum();
    let (compressed_files, uncompressed_files) = file_counts(files);
    Ok(CandidateArchive {
        format: ArchiveFormat::Ustar,
        bytes,
        manifest_bytes: manifest.len(),
        name_bytes,
        header_bytes,
        padding_bytes,
        compression_frame_bytes,
        stored_payload_bytes,
        compressed_files,
        uncompressed_files,
    })
}

fn append_tar_entry<W: io::Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    data: &[u8],
) -> io::Result<()> {
    let mut header = tar::Header::new_ustar();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
        .set_path(path)
        .map_err(|error| io::Error::other(format!("TAR path {path}: {error}")))?;
    header.set_cksum();
    builder.append(&header, data)
}

fn build_custom(manifest: &[u8], files: &[EncodedFile<'_>]) -> io::Result<CandidateArchive> {
    let manifest_len = u32::try_from(manifest.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "manifest exceeds u32"))?;
    let file_count = u32::try_from(files.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file count exceeds u32"))?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"XPK1");
    bytes.extend_from_slice(&manifest_len.to_le_bytes());
    bytes.extend_from_slice(&file_count.to_le_bytes());
    bytes.extend_from_slice(manifest);
    let mut name_bytes = 0;
    let mut header_bytes = 12;
    let mut compression_frame_bytes = 0;
    let mut stored_payload_bytes = 0;
    for file in files {
        let path = file.fixture.path.as_bytes();
        let name_len = u16::try_from(path.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path exceeds u16"))?;
        let data_len = u64::try_from(file.data.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "payload exceeds u64"))?;
        bytes.extend_from_slice(&name_len.to_le_bytes());
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.extend_from_slice(path);
        bytes.extend_from_slice(&file.data);
        name_bytes += path.len();
        header_bytes += 10;
        compression_frame_bytes += file.frame_bytes;
        stored_payload_bytes += file.data.len() - file.frame_bytes;
    }
    let (compressed_files, uncompressed_files) = file_counts(files);
    Ok(CandidateArchive {
        format: ArchiveFormat::CustomXpk,
        bytes,
        manifest_bytes: manifest.len(),
        name_bytes,
        header_bytes,
        padding_bytes: 0,
        compression_frame_bytes,
        stored_payload_bytes,
        compressed_files,
        uncompressed_files,
    })
}

fn file_counts(files: &[EncodedFile<'_>]) -> (usize, usize) {
    let compressed = files
        .iter()
        .filter(|file| matches!(file.compression, Compression::Lz4Blocks))
        .count();
    (compressed, files.len() - compressed)
}

fn cpio_name_field_bytes(path_bytes: usize) -> usize {
    align(110 + path_bytes + 1, 4).unwrap() - 110
}

pub fn write_evaluation(output_dir: &Path, seed: u64) -> io::Result<Vec<CandidateArchive>> {
    fs::create_dir_all(output_dir)?;
    let output_dir = output_dir.canonicalize()?;
    if output_dir == std::env::current_dir()?.canonicalize()? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to write evaluation into current directory",
        ));
    }
    let archives = build_candidate_archives(&fixtures(seed)?)?;
    for archive in &archives {
        fs::write(output_dir.join(archive.format.filename()), &archive.bytes)?;
    }
    let mut csv = String::from(
        "format,archive_bytes,manifest_bytes,name_bytes,header_bytes,padding_bytes,compression_frame_bytes,stored_payload_bytes,compressed_files,uncompressed_files\n",
    );
    for archive in &archives {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            archive.format.name(),
            archive.bytes.len(),
            archive.manifest_bytes,
            archive.name_bytes,
            archive.header_bytes,
            archive.padding_bytes,
            archive.compression_frame_bytes,
            archive.stored_payload_bytes,
            archive.compressed_files,
            archive.uncompressed_files
        ));
    }
    fs::write(output_dir.join("summary.csv"), csv)?;
    let mut json = String::from("[\n");
    for (index, archive) in archives.iter().enumerate() {
        if index != 0 {
            json.push_str(",\n");
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        archive.bytes.hash(&mut hasher);
        let probe = probe_candidate(archive, 127)?;
        json.push_str(&format!(
            "  {{\"format\":\"{}\",\"archive_bytes\":{},\"manifest_bytes\":{},\"name_bytes\":{},\"header_bytes\":{},\"padding_bytes\":{},\"compression_frame_bytes\":{},\"stored_payload_bytes\":{},\"read_buffer_ceiling\":{},\"entries_read\":{},\"archive_hash\":\"{:016x}\",\"tool_versions\":{{\"package_format_eval\":\"{}\",\"tar\":\"0.4.46\",\"hadris_cpio\":\"2.5.0\",\"lz4_flex\":\"0.12.2\"}},\"hardware_measurements\":null}}",
            archive.format.name(), archive.bytes.len(), archive.manifest_bytes, archive.name_bytes,
            archive.header_bytes, archive.padding_bytes, archive.compression_frame_bytes,
            archive.stored_payload_bytes, probe.peak_user_buffer, probe.entry_count, hasher.finish(),
            env!("CARGO_PKG_VERSION")
        ));
    }
    json.push_str("\n]\n");
    fs::write(output_dir.join("summary.json"), json)?;
    Ok(archives)
}

fn align(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment - 1)
        .map(|rounded| rounded & !(alignment - 1))
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
    result.push(Fixture {
        path: "share/text/large.txt".into(),
        data: "Xila package compression probe: repeated text.\n"
            .repeat(384)
            .into_bytes(),
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
        path: format!(
            "share/boundary/{}/{}.boundary.txt",
            "a".repeat(110),
            "b".repeat(80)
        ),
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
