//! Runtime catalogs. All eight locales were exported verbatim from the old
//! TS bundles (`src/i18n/locales/*.ts` at commit c4e8721).

use std::collections::HashMap;
use std::sync::OnceLock;

type Dict = HashMap<&'static str, &'static str>;

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
        let en_src = CATALOG_SOURCES[0].1;
        let en_json: serde_json::Value = serde_json::from_str(en_src).expect("en catalog");
        let en: Dict = en_json
            .as_object()
            .expect("en object")
            .iter()
            .map(|(k, v)| (leak(k.clone()), leak(v.as_str().unwrap_or("").to_string())))
            .collect();

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
        .unwrap_or_else(|| leak(key.to_string()))
}

/// Interpolating variant: replaces `{name}` placeholders from `vars`.
pub fn tv(locale: &str, key: &str, vars: &[(&str, &str)]) -> String {
    let mut out = t(locale, key).to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

pub fn language_name(locale: &str, code: &str) -> &'static str {
    t(locale, &format!("languages.{code}"))
}
