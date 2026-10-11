//! Minimal i18n (TIOSP-style): JSON maps in locales/, EN fallback.
use std::collections::HashMap;

pub struct I18n {
    map: HashMap<String, String>,
    fb: HashMap<String, String>,
}

impl I18n {
    pub fn load(lang: &str) -> Self {
        let fb: HashMap<String, String> =
            serde_json::from_str(include_str!("../locales/en.json")).unwrap_or_default();
        let map: HashMap<String, String> = ["de", "en"]
            .contains(&lang)
            .then(|| {
                let f = if lang == "de" { include_str!("../locales/de.json") } else { include_str!("../locales/en.json") };
                serde_json::from_str(f).unwrap_or_default()
            })
            .unwrap_or_default();
        Self { map, fb }
    }
    pub fn t(&self, key: &str) -> String {
        self.map.get(key).or_else(|| self.fb.get(key)).cloned().unwrap_or_else(|| key.to_string())
    }
}
