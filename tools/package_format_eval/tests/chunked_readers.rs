use package_format_eval::{build_candidate_archives, fixtures, probe_candidate};

#[test]
fn archive_parsers_consume_arbitrary_short_reads_with_bounded_buffers() {
    let archives = build_candidate_archives(&fixtures(402).unwrap()).unwrap();
    for archive in archives {
        let report = probe_candidate(&archive, 127)
            .unwrap_or_else(|error| panic!("{}: {error}", archive.format.name()));
        assert_eq!(report.entry_count, 107);
        assert_eq!(report.first_path, ".xila/manifest");
        assert!(report.max_source_read_bytes <= 127);
        assert_eq!(report.configured_chunk_bytes, 127);
    }
}

#[test]
fn custom_parser_rejects_truncation_and_traversal_before_payload_processing() {
    let archive = build_candidate_archives(&fixtures(402).unwrap())
        .unwrap()
        .into_iter()
        .find(|archive| archive.format.name() == "custom-xpk")
        .unwrap();

    let mut truncated = archive.clone();
    truncated.bytes.truncate(8);
    assert!(probe_candidate(&truncated, 64).is_err());

    let mut traversal = archive;
    let manifest_len = u32::from_le_bytes(traversal.bytes[4..8].try_into().unwrap()) as usize;
    let record_name = 12 + manifest_len + 10;
    traversal.bytes[record_name..record_name + 20].copy_from_slice(b"../application.wasmX");
    assert!(probe_candidate(&traversal, 64).is_err());
}
