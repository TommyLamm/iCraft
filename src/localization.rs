//! Structured, parameter-aware UI text with a deterministic English fallback.

use crate::resources::ResourcePackManager;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    English,
    German,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en_us",
            Self::German => "de_de",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::English => "ENGLISH",
            Self::German => "DEUTSCH",
        }
    }

    pub fn parse(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("deutsch")
            || value.trim().eq_ignore_ascii_case("german")
            || value.trim().eq_ignore_ascii_case("de_de")
        {
            Self::German
        } else {
            Self::English
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::English => Self::German,
            Self::German => Self::English,
        }
    }
}

pub const REQUIRED_KEYS: &[&str] = &[
    "menu.singleplayer",
    "menu.multiplayer",
    "menu.options",
    "menu.quit_game",
    "menu.select_world",
    "menu.controls",
    "menu.accessibility",
    "menu.resource_packs",
    "menu.done",
    "menu.back",
    "menu.apply",
    "menu.reload",
    "menu.create_world",
    "menu.cancel",
    "menu.enter_valid_multiplayer",
    "menu.world_copied",
    "menu.world_backed_up",
    "menu.language",
    "menu.ui_scale",
    "menu.chat_scale",
    "menu.chat_opacity",
    "menu.subtitles",
    "menu.high_contrast",
    "menu.reduce_flashing",
    "menu.toggle_sprint",
    "menu.toggle_sneak",
    "menu.camera_bobbing",
    "menu.damage_tilt",
    "hud.subtitle.direction_left",
    "hud.subtitle.direction_right",
    "hud.subtitle.direction_front",
    "hud.subtitle.direction_back",
    "hud.subtitle.center",
    "sound.jump",
    "sound.hurt",
    "sound.death",
    "sound.explosion",
    "sound.thunder",
    "sound.arrow",
    "sound.creeper",
    "sound.ui_click",
    "sound.block",
    "death.fall",
    "death.void",
    "death.starved",
    "death.mob",
    "death.explosion",
    "death.drowned",
    "death.lightning",
    "death.generic",
    "command.feedback",
    "disconnect.generic",
    "advancement.toast",
];

#[derive(Debug, Clone)]
pub struct TranslationCatalog {
    english: HashMap<String, String>,
    active: HashMap<String, String>,
    missing: HashSet<String>,
}

impl TranslationCatalog {
    pub fn builtin(language: Language) -> Self {
        Self::from_json(
            language,
            include_str!("../assets/lang/en_us.json"),
            include_str!("../assets/lang/de_de.json"),
        )
    }

    pub fn from_json(language: Language, english_json: &str, active_json: &str) -> Self {
        let english = parse_map(english_json).unwrap_or_default();
        let active = parse_map(active_json).unwrap_or_default();
        Self {
            english,
            active: if language == Language::English && active.is_empty() {
                parse_map(english_json).unwrap_or_default()
            } else {
                active
            },
            missing: HashSet::new(),
        }
    }

    pub fn from_resource_packs(manager: &ResourcePackManager, language: Language) -> Self {
        let english = manager
            .locale_bytes(Language::English.code())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| include_str!("../assets/lang/en_us.json").to_string());
        let active = manager
            .locale_bytes(language.code())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| {
                if language == Language::English {
                    english.clone()
                } else {
                    include_str!("../assets/lang/de_de.json").to_string()
                }
            });
        Self::from_json(language, &english, &active)
    }

    pub fn translate(&mut self, key: &str) -> String {
        if let Some(value) = self.active.get(key) {
            return value.clone();
        }
        if let Some(value) = self.english.get(key) {
            self.missing.insert(key.to_string());
            return value.clone();
        }
        self.missing.insert(key.to_string());
        key.to_string()
    }

    pub fn format(&mut self, key: &str, arguments: &[(&str, &str)]) -> String {
        let mut value = self.translate(key);
        for (name, replacement) in arguments {
            let token = format!("{{{name}}}");
            value = value.replace(&token, replacement);
        }
        value
    }

    pub fn plural(&mut self, key: &str, count: u64) -> String {
        let suffix = if count == 1 { ".one" } else { ".other" };
        let plural_key = format!("{key}{suffix}");
        if self.active.contains_key(&plural_key) || self.english.contains_key(&plural_key) {
            let count_text = count.to_string();
            return self.format(&plural_key, &[("count", &count_text)]);
        }
        let count_text = count.to_string();
        self.format(key, &[("count", &count_text)])
    }

    pub fn missing_keys(&self) -> Vec<String> {
        let mut missing = self.missing.iter().cloned().collect::<Vec<_>>();
        missing.sort();
        missing
    }

    pub fn coverage(&self) -> f32 {
        if REQUIRED_KEYS.is_empty() {
            return 1.0;
        }
        REQUIRED_KEYS
            .iter()
            .filter(|key| self.active.contains_key(**key))
            .count() as f32
            / REQUIRED_KEYS.len() as f32
    }

    pub fn validate_required_keys(&self) -> Vec<String> {
        REQUIRED_KEYS
            .iter()
            .filter(|key| !self.english.contains_key(**key))
            .map(|key| (*key).to_string())
            .collect()
    }
}

fn parse_map(json: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    serde_json::from_str(json)
}

pub fn translate(language: Language, key: &str) -> String {
    static ENGLISH: OnceLock<HashMap<String, String>> = OnceLock::new();
    static GERMAN: OnceLock<HashMap<String, String>> = OnceLock::new();
    let english = ENGLISH
        .get_or_init(|| parse_map(include_str!("../assets/lang/en_us.json")).unwrap_or_default());
    let active = match language {
        Language::English => english,
        Language::German => GERMAN.get_or_init(|| {
            parse_map(include_str!("../assets/lang/de_de.json")).unwrap_or_default()
        }),
    };
    active
        .get(key)
        .or_else(|| english.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

pub fn format(language: Language, key: &str, arguments: &[(&str, &str)]) -> String {
    let mut value = translate(language, key);
    for (name, replacement) in arguments {
        let token = format!("{{{name}}}");
        value = value.replace(&token, replacement);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_required_keys_have_values_and_german_is_complete() {
        let english = TranslationCatalog::builtin(Language::English);
        let german = TranslationCatalog::builtin(Language::German);
        assert!(english.validate_required_keys().is_empty());
        assert!(german.validate_required_keys().is_empty());
        assert!((english.coverage() - 1.0).abs() < f32::EPSILON);
        assert!((german.coverage() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn missing_key_falls_back_without_english_fragments() {
        let mut catalog =
            TranslationCatalog::from_json(Language::German, r#"{"hello":"Hello {name}"}"#, r#"{}"#);
        assert_eq!(catalog.format("hello", &[("name", "Alex")]), "Hello Alex");
        assert_eq!(catalog.translate("missing.key"), "missing.key");
        assert_eq!(catalog.missing_keys(), ["hello", "missing.key"]);
    }

    #[test]
    fn parameter_and_plural_messages_are_structured() {
        let mut catalog = TranslationCatalog::from_json(
            Language::English,
            r#"{"item.one":"{count} item","item.other":"{count} items"}"#,
            r#"{"item.one":"{count} item","item.other":"{count} items"}"#,
        );
        assert_eq!(catalog.plural("item", 1), "1 item");
        assert_eq!(catalog.plural("item", 2), "2 items");
    }
}
