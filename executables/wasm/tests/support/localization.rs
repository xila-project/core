//! Read the guest's catalog, not the host `wasm` crate's translation namespace.

pub fn text(application: &str, key: &str) -> String {
    let locale = option_env!("EMBEDDED_I18N_LOCALE")
        .unwrap_or("en")
        .replace('_', "-");
    let locale = match locale.to_ascii_lowercase().as_str() {
        "en" => "en",
        "de" => "de",
        "es" => "es",
        "fr" => "fr",
        "it" => "it",
        "pl" => "pl",
        "pt-br" => "pt-BR",
        "zh-cn" => "zh-CN",
        _ => panic!("unsupported guest test locale: {locale}"),
    };
    assert!(matches!(
        application,
        "clock" | "pixel_painter" | "text_editor"
    ));
    let root = std::env::var_os("XILA_APPLICATIONS_MANIFEST")
        .map(std::path::PathBuf::from)
        .and_then(|manifest| manifest.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let path = root
        .join("executables")
        .join(application)
        .join("locales")
        .join(format!("{locale}.json"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let messages: serde_json::Value = serde_json::from_str(&source).unwrap();
    let value = messages.get(key).or_else(|| {
        key.split('.')
            .try_fold(&messages, |node, part| node.get(part))
    });
    value
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("missing {application} translation for {key:?} in {locale}"))
        .to_owned()
}
