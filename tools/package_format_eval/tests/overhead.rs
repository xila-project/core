use package_format_eval::{
    build_candidate_archives, cpio_entry_bytes, encode_file_for_test, fixtures, tar_archive_bytes,
    tar_entry_bytes,
};
use std::process::Command;

#[test]
fn archive_overhead_uses_format_alignment_and_terminators() {
    assert_eq!(cpio_entry_bytes(20, 64), Some(196));
    assert_eq!(tar_entry_bytes(64), Some(1024));
    assert_eq!(tar_archive_bytes([64]), Some(2048));
}

#[test]
fn independent_lz4_blocks_roundtrip_and_raw_mode_wins_when_compression_expands() {
    let repeated = vec![b'Z'; 9_000];
    let (stored, compressed, frame_bytes) = encode_file_for_test(&repeated).unwrap();
    assert!(compressed);
    assert_eq!(frame_bytes, 24);
    let mut decoded = Vec::new();
    let mut offset = 0;
    while offset < stored.len() {
        let compressed_len =
            u32::from_le_bytes(stored[offset..offset + 4].try_into().unwrap()) as usize;
        let expanded_len =
            u32::from_le_bytes(stored[offset + 4..offset + 8].try_into().unwrap()) as usize;
        offset += 8;
        let end = offset + compressed_len;
        decoded.extend_from_slice(
            &lz4_flex::block::decompress(&stored[offset..end], expanded_len).unwrap(),
        );
        offset = end;
    }
    assert_eq!(decoded, repeated);

    let opaque = fixtures(812).unwrap()[0].data[8..4096 + 8].to_vec();
    let (raw, compressed, frame_bytes) = encode_file_for_test(&opaque).unwrap();
    assert!(!compressed);
    assert_eq!(frame_bytes, 0);
    assert_eq!(raw, opaque);
}

#[test]
fn writers_are_deterministic_and_compress_each_file_independently() {
    let corpus = fixtures(402).unwrap();
    let first = build_candidate_archives(&corpus).unwrap();
    let second = build_candidate_archives(&corpus).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
    for archive in &first {
        assert_eq!(
            archive.bytes.len(),
            archive.manifest_bytes
                + archive.name_bytes
                + archive.header_bytes
                + archive.padding_bytes
                + archive.compression_frame_bytes
                + archive.stored_payload_bytes,
            "{} breakdown mismatch",
            archive.format.name()
        );
        assert!(archive.compressed_files > 0);
        assert!(archive.uncompressed_files > 0);
    }
}

#[test]
fn evaluate_command_writes_candidate_archives_and_csv_beneath_output() {
    let output = std::env::temp_dir().join(format!("xila-archive-eval-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&output);
    let result = Command::new(env!("CARGO_BIN_EXE_package_format_eval"))
        .args(["evaluate", "--output"])
        .arg(&output)
        .args(["--seed", "402"])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for filename in [
        "package.cpio",
        "package.tar",
        "package.xpk",
        "summary.csv",
        "summary.json",
    ] {
        assert!(output.join(filename).is_file(), "missing {filename}");
    }
    let summary = std::fs::read_to_string(output.join("summary.json")).unwrap();
    assert!(summary.contains("\"max_source_read_bytes\""));
    assert!(summary.contains("\"parser_peak_memory_bytes\":null"));
    assert!(summary.contains("\"archive_hash\""));
    assert!(summary.contains("\"tool_versions\""));
    assert!(
        std::fs::read(output.join("package.xpk"))
            .unwrap()
            .starts_with(b"XPK1")
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[cfg(unix)]
#[test]
fn evaluate_refuses_symlink_outputs_that_escape_the_output_directory() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!("xila-archive-symlink-{}", std::process::id()));
    let output = root.join("out");
    let outside = root.join("outside.txt");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(&outside, b"leave unchanged").unwrap();
    symlink(&outside, output.join("summary.csv")).unwrap();

    let result = package_format_eval::write_evaluation(&output, 402);

    assert!(result.is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"leave unchanged");
    std::fs::remove_dir_all(root).unwrap();
}
