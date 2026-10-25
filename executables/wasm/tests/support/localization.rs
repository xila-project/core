//! Read the guest's catalog, not the host `wasm` crate's translation namespace.

pub fn text(application: &str, key: &str) -> String {
    let locale = option_env!("EMBEDDED_I18N_LOCALE")
        .unwrap_or("en")
        .replace('_', "-")
        .to_ascii_lowercase();
    macro_rules! catalog {
        ($application:literal) => {
            match locale.as_str() {
                "en" => include_str!(concat!("../../../", $application, "/locales/en.json")),
                "de" => include_str!(concat!("../../../", $application, "/locales/de.json")),
                "es" => include_str!(concat!("../../../", $application, "/locales/es.json")),
                "fr" => include_str!(concat!("../../../", $application, "/locales/fr.json")),
                "it" => include_str!(concat!("../../../", $application, "/locales/it.json")),
                "pl" => include_str!(concat!("../../../", $application, "/locales/pl.json")),
                "pt-br" => include_str!(concat!("../../../", $application, "/locales/pt-BR.json")),
                "zh-cn" => include_str!(concat!("../../../", $application, "/locales/zh-CN.json")),
                _ => panic!("unsupported guest test locale: {locale}"),
            }
        };
    }
    let catalog = match application {
        "clock" => catalog!("clock"),
        "pixel_painter" => catalog!("pixel_painter"),
        "text_editor" => catalog!("text_editor"),
        _ => panic!("unknown guest test application: {application}"),
    };
    let messages: serde_json::Value = serde_json::from_str(catalog).unwrap();
    messages[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing {application} translation for {key:?} in {locale}"))
        .to_owned()
}
