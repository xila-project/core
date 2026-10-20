use package_format_eval::{fixtures, write_fixtures};
use std::collections::BTreeSet;
use std::fs;

#[test]
fn fixture_corpus_is_deterministic_for_a_fixed_seed() {
    assert_eq!(fixtures(402).unwrap(), fixtures(402).unwrap());
    assert_ne!(fixtures(402).unwrap(), fixtures(403).unwrap());
}

#[test]
fn corpus_writer_keeps_files_beneath_the_requested_output_directory() {
    let output = std::env::temp_dir().join(format!("xila-package-eval-{}", std::process::id()));
    let _ = fs::remove_dir_all(&output);

    let written_to = write_fixtures(&output, 402).unwrap();

    assert_eq!(written_to, output.canonicalize().unwrap());
    assert_eq!(
        fs::read(output.join("etc/application.toml")).unwrap(),
        b"name = \"evaluation\"\nversion = \"1.0.0\"\n"
    );
    assert_eq!(
        fs::read_dir(output.join("share/tiny")).unwrap().count(),
        100
    );
    fs::remove_dir_all(output).unwrap();
}

#[test]
fn fixture_paths_are_unique_relative_and_cover_required_categories() {
    let corpus = fixtures(402).unwrap();
    let paths: BTreeSet<_> = corpus.iter().map(|fixture| fixture.path.as_str()).collect();

    assert_eq!(paths.len(), corpus.len());
    assert!(corpus.iter().all(|fixture| {
        !fixture.path.is_empty()
            && !fixture.path.starts_with('/')
            && !fixture.path.split('/').any(|component| component == "..")
    }));
    assert!(
        corpus
            .iter()
            .any(|fixture| fixture.path.ends_with(".wasm") && fixture.data.len() >= 1_000_000)
    );
    assert!(
        corpus
            .iter()
            .any(|fixture| fixture.path.ends_with(".toml") && fixture.data.is_ascii())
    );
    assert!(
        corpus
            .iter()
            .any(|fixture| fixture.path.ends_with(".png") && fixture.data.len() >= 32_000)
    );
    assert!(
        corpus
            .iter()
            .filter(|fixture| fixture.path.starts_with("share/tiny/"))
            .count()
            >= 100
    );
    assert!(
        corpus
            .iter()
            .any(|fixture| fixture.path.starts_with("share/nested/"))
    );
    assert!(
        corpus
            .iter()
            .any(|fixture| fixture.path.ends_with("boundary.txt") && fixture.path.len() >= 200)
    );
}
