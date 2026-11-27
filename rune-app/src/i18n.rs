//! Runtime catalogs. All eight locales were exported verbatim from the old
//! TS bundles (`src/i18n/locales/*.ts` at commit c4e8721).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type Dict = HashMap<&'static str, &'static str>;
type Missing = HashMap<String, &'static str>;

const CATALOG_SOURCES: &[(&str, &str)] = &[
    ("en", include_str!("i18n/catalogs/en.json")),
    ("fr", include_str!("i18n/catalogs/fr.json")),
    ("pt-BR", include_str!("i18n/catalogs/pt_BR.json")),
    ("zh-CN", include_str!("i18n/catalogs/zh_CN.json")),
    ("zh-TW", include_str!("i18n/catalogs/zh_TW.json")),
    ("ru", include_str!("i18n/catalogs/ru.json")),
    ("id", include_str!("i18n/catalogs/id.json")),
    ("ar", include_str!("i18n/catalogs/ar.json")),
];

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn catalogs() -> &'static HashMap<&'static str, Dict> {
    static CATS: OnceLock<HashMap<&'static str, Dict>> = OnceLock::new();
    CATS.get_or_init(|| {
        // `en` is the embedded source of truth and is validated by CI
        // (`check_i18n.py`); degrade to an empty English map rather than panic
        // a launch if it ever failed to parse.
        let en_src = CATALOG_SOURCES[0].1;
        let en: Dict = match serde_json::from_str::<serde_json::Value>(en_src) {
            Ok(value) => value
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .map(|(k, v)| (leak(k.clone()), leak(v.as_str().unwrap_or("").to_string())))
                        .collect()
                })
                .unwrap_or_default(),
            Err(err) => {
                eprintln!("[i18n] English catalog failed to parse: {err}");
                Dict::new()
            }
        };

        let mut all = HashMap::new();
        all.insert("en", en.clone());
        for (locale, src) in CATALOG_SOURCES.iter().skip(1) {
            let value: serde_json::Value = serde_json::from_str(src).unwrap_or_default();
            let mut dict = en.clone();
            if let Some(obj) = value.as_object() {
                for (k, v) in obj {
                    dict.insert(leak(k.clone()), leak(v.as_str().unwrap_or("").to_string()));
                }
            }
            all.insert(*locale, dict);
        }
        all
    })
}

/// Missing keys still have to be leaked so callers keep `&'static str`, but the
/// catalog can be hit thousands of times per frame while typing. Cache each
/// fallback so every unknown key leaks at most once for the process lifetime.
fn missing_fallback(key: &str) -> &'static str {
    static MISSING: OnceLock<Mutex<Missing>> = OnceLock::new();
    let cache = MISSING.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().unwrap();
    if let Some(value) = cache.get(key) {
        return value;
    }
    let value: &'static str = leak(key.to_string());
    cache.insert(key.to_string(), value);
    value
}

/// Lookup with English fallback. Returns a leaked static string so callers can
/// keep using `&'static str` in Rune element builders.
pub fn t(locale: &str, key: &str) -> &'static str {
    let cats = catalogs();
    let en = cats.get("en").expect("en catalog");
    let dict = cats.get(locale).unwrap_or(en);
    dict.get(key)
        .filter(|v| !v.is_empty())
        .or_else(|| en.get(key))
        .copied()
        .unwrap_or_else(|| missing_fallback(key))
}

/// Interpolating variant: replaces `{name}` placeholders from `vars`.
pub fn tv(locale: &str, key: &str, vars: &[(&str, &str)]) -> String {
    let mut out = t(locale, key).to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// The display name for a language code in `locale`. An unknown code falls
/// back to the code itself rather than the raw `languages.xx` key, so a stale
/// pref never shows a raw catalog key on the trigger button.
pub fn language_name(locale: &str, code: &str) -> &'static str {
    let key = format!("languages.{code}");
    let value = t(locale, &key);
    if value == key {
        missing_fallback(code)
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_is_cached_not_leaked_per_lookup() {
        let first = t("zz", "tests.missing.key");
        let second = t("zz", "tests.missing.key");
        assert_eq!(first, "tests.missing.key");
        assert!(
            std::ptr::eq(first, second),
            "the same missing key must reuse one leaked fallback"
        );
    }

    #[test]
    fn a_missing_locale_falls_back_to_english() {
        let english = t("en", "header.scrcpyReady");
        assert!(!english.is_empty());
        assert_eq!(t("zz", "header.scrcpyReady"), english);
        assert_eq!(
            t("zz", "tests.another.missing.key"),
            "tests.another.missing.key"
        );
    }
}
