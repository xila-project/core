//! Every locale translates exactly the English message set.
use std::{collections::BTreeSet, fs, path::Path};

use io_error::Kind;

fn keys(locale: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("locales")
        .join(format!("{locale}.json"));
    fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (key, rest) = line.strip_prefix('"')?.split_once("\": ")?;
            rest.starts_with('"').then(|| key.to_owned())
        })
        .collect()
}

#[test]
fn all_nine_locales_cover_every_kind_exactly() {
    let english = keys("en");
    assert_eq!(english.len(), Kind::ALL.len());
    for locale in ["de", "es", "fr", "it", "ja", "pl", "pt-BR", "zh-CN"] {
        assert_eq!(keys(locale), english, "{locale}");
    }
}

#[test]
fn english_messages_are_distinct_and_never_leak_internals() {
    let mut seen = BTreeSet::new();
    for kind in Kind::ALL {
        let message = kind.to_string();
        assert!(!message.is_empty());
        assert!(
            seen.insert(message.clone()),
            "duplicate message {message:?}"
        );
        // No initialization steps or internal component names.
        for internal in ["seed", "spawn", "mount_device", "manager", "subsystem"] {
            assert!(
                !message.to_lowercase().contains(internal),
                "{kind:?}: {message:?}"
            );
        }
    }
}
