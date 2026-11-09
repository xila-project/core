//! Read the same locale defaults and fallback chain as the translation macros.
use std::path::Path;

pub fn read(manifest: &Path, locale_override: Option<String>) -> (String, String) {
    let source = std::fs::read_to_string(manifest).expect("graphics manifest");
    from_manifest(&source, locale_override)
}

fn from_manifest(source: &str, locale_override: Option<String>) -> (String, String) {
    let manifest: toml::Value = toml::from_str(source).expect("graphics manifest TOML");
    let config = &manifest["package"]["metadata"]["i18n"];
    let locale = locale_override.unwrap_or_else(|| {
        config["default-locale"]
            .as_str()
            .expect("default locale")
            .to_owned()
    });
    let fallback = config["fallback"]
        .as_array()
        .expect("fallback locales")
        .iter()
        .map(|locale| locale.as_str().expect("fallback locale"))
        .collect::<Vec<_>>()
        .join(",");
    (locale, fallback)
}

#[cfg(test)]
mod tests {
    use super::from_manifest;
    #[test]
    fn font_selection_uses_translation_metadata_and_the_locale_override() {
        let source = "[package.metadata.i18n]\ndefault-locale = 'fr'\nfallback = ['en', 'de']\n";
        assert_eq!(from_manifest(source, None), ("fr".into(), "en,de".into()));
        assert_eq!(
            from_manifest(source, Some("zh-CN".into())),
            ("zh-CN".into(), "en,de".into())
        );
    }
}
