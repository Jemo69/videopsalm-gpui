//! i18n via JSON locale files (v1: en + fr, SPEC 2.5).
//!
//! Locale files live in `gpui-app/locales/<lang>.json` as flat
//! `{ "key": "text" }` maps. Missing keys fall back to the key itself so
//! the UI never renders blanks.

use std::collections::HashMap;

/// A loaded UI language.
#[derive(Clone, Debug, Default)]
pub struct Locale {
    pub lang: String,
    strings: HashMap<String, String>,
}

impl Locale {
    /// Load from a JSON object string.
    pub fn load(lang: &str, json: &str) -> Result<Self, String> {
        let strings: HashMap<String, String> =
            serde_json::from_str(json).map_err(|e| format!("bad locale {lang}: {e}"))?;
        Ok(Locale {
            lang: lang.to_string(),
            strings,
        })
    }

    /// Translate `key`; falls back to the key when missing.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).map(|s| s.as_str()).unwrap_or(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.strings.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_and_fallback() {
        let en = Locale::load("en", r#"{"agenda": "Agenda", "search": "Search"}"#).unwrap();
        assert_eq!(en.t("agenda"), "Agenda");
        assert_eq!(en.t("missing_key"), "missing_key");
        assert!(Locale::load("xx", "not json").is_err());
    }
}
