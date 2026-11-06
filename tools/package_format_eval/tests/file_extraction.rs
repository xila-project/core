use package_format_eval::{
    build_candidate_archives_uncompressed, extract_candidate_file_backed, fixtures,
};
use std::fs;

#[test]
fn file_backed_extractors_write_identical_files_with_a_512_byte_buffer() {
    let corpus = fixtures(402).unwrap();
    let candidates = build_candidate_archives_uncompressed(&corpus).unwrap();
    let root = std::env::temp_dir().join(format!("xila-file-extract-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let expected_bytes: usize = corpus.iter().map(|fixture| fixture.data.len()).sum();

    for candidate in candidates {
        let source = root.join(format!("{}.pkg", candidate.format.name()));
        fs::write(&source, &candidate.bytes).unwrap();
        let output = root.join(format!("{}-all", candidate.format.name()));
        let result =
            extract_candidate_file_backed(candidate.format, &source, &output, 512, None).unwrap();
        assert_eq!(result.files_written, corpus.len());
        assert_eq!(result.bytes_written, expected_bytes as u64);
        assert!(result.max_source_read_bytes <= 512);
        for fixture in &corpus {
            assert_eq!(fs::read(output.join(&fixture.path)).unwrap(), fixture.data);
        }

        let selected = root.join(format!("{}-selected", candidate.format.name()));
        let result = extract_candidate_file_backed(
            candidate.format,
            &source,
            &selected,
            512,
            Some("bin/application.wasm"),
        )
        .unwrap();
        assert_eq!(result.files_written, 1);
        assert_eq!(result.bytes_written, 1_048_576);
        assert!(result.max_source_read_bytes <= 512);
        assert_eq!(
            fs::read(selected.join("bin/application.wasm")).unwrap(),
            corpus[0].data
        );
    }

    fs::remove_dir_all(root).unwrap();
}
