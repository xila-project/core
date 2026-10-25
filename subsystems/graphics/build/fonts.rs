//! Build-only locale coverage. Shared with the isolated font pipeline tests.

use std::ops::Range;

use embedded_i18n::{get_locale_ranges, merge_contiguous_ranges};

const SHIPPED_LOCALES: &[&str] = &["en", "fr", "es", "de", "pt-BR", "it", "pl"];
const EXTRA_RANGES: &[Range<u32>] = &[0xb0..0xb1, 0x2022..0x2023];

pub fn catalog_font(locale: &str, fallback: &str) -> Option<&'static str> {
    std::iter::once(locale)
        .chain(fallback.split(','))
        .find_map(|locale| match normalize_locale(locale).as_str() {
            "ja" | "ja-JP" => Some("SourceHanSansJP-Regular.otf"),
            "zh" | "zh-CN" => Some("SourceHanSansSC-Regular.otf"),
            _ => None,
        })
}

pub fn uses_catalog_subset(locale: &str, fallback: &str) -> bool {
    catalog_font(locale, fallback).is_some()
}

pub fn uses_chinese(locale: &str, fallback: &str) -> bool {
    std::iter::once(locale)
        .chain(fallback.split(','))
        .any(|locale| matches!(normalize_locale(locale).as_str(), "zh" | "zh-CN"))
}

pub fn normalize_locale(locale: &str) -> String {
    locale
        .trim()
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .enumerate()
        .map(|(index, part)| {
            if index > 0 && part.len() == 2 {
                part.to_ascii_uppercase()
            } else {
                part.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn locale_ranges(locale: &str) -> Result<&'static [Range<u32>], String> {
    let mut normalized = normalize_locale(locale);
    loop {
        if let Some(ranges) = get_locale_ranges(&normalized) {
            return Ok(ranges);
        }
        match normalized.rfind('-') {
            Some(index) => normalized.truncate(index),
            None => return Err(format!("Unsupported font locale: {locale}")),
        }
    }
}

pub fn effective_ranges(locale: &str, fallback: &str) -> Result<Vec<Range<u32>>, String> {
    let mut ranges = locale_ranges(locale)?.to_vec();
    for fallback in fallback.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        ranges.extend_from_slice(locale_ranges(fallback)?);
    }
    ranges.extend_from_slice(EXTRA_RANGES);
    Ok(merge_contiguous_ranges(ranges))
}

pub fn supported_variants() -> Vec<Vec<Range<u32>>> {
    let mut variants = Vec::new();
    for locale in SHIPPED_LOCALES {
        for fallback in SHIPPED_LOCALES {
            let ranges = effective_ranges(locale, fallback).expect("shipped font locale");
            if !variants.contains(&ranges) {
                variants.push(ranges);
            }
        }
    }
    variants
}
