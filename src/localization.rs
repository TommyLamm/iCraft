//! Structured, parameter-aware UI text with a deterministic English fallback.

use crate::entity::EntityType;
use crate::resources::ResourcePackManager;
use crate::{inventory::Item, world::BlockType};
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
    language: Language,
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
            language,
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
        // Keep the original shared-reference API for callers that do not
        // need diagnostics. Clone the bounded manager so validation still
        // follows the exact same selected-pack path as the mutable API.
        let mut manager = manager.clone();
        Self::from_resource_packs_mut(&mut manager, language)
    }

    /// Build a catalog through validated ResourcePackManager locale bytes.
    /// Invalid UTF-8/JSON entries are skipped with one manager diagnostic and
    /// the next lower-priority pack (usually built-in) is selected.
    pub fn from_resource_packs_mut(manager: &mut ResourcePackManager, language: Language) -> Self {
        let english = manager
            .resolve_locale(Language::English.code())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| include_str!("../assets/lang/en_us.json").to_string());
        let active = manager
            .resolve_locale(language.code())
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
        let value = self.lookup(key);
        if self.active.contains_key(key) {
            return value;
        }
        if self.english.contains_key(key) {
            self.missing.insert(key.to_string());
            return value;
        }
        self.missing.insert(key.to_string());
        value
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// Resolve a localized name and retain the engine's built-in display name
    /// when a selected pack does not provide that logical key.  Resource packs
    /// can therefore override item/block/entity labels without requiring every
    /// built-in key to be duplicated in the pack.
    fn named(&self, namespace: &str, display_name: &str) -> String {
        let key = format!("{namespace}.{}", key_component(display_name));
        let value = self.lookup(&key);
        if value == key {
            display_name.to_string()
        } else {
            value
        }
    }

    pub fn item_name(&self, item: Item) -> String {
        self.named("item", item.properties().name)
    }

    pub fn block_name(&self, block: BlockType) -> String {
        self.named("block", block.properties().name)
    }

    pub fn entity_name(&self, entity: EntityType) -> String {
        let display_name = entity_debug_name(entity);
        self.named("entity", &display_name)
    }

    /// Read a translated value without mutating missing-key diagnostics. UI
    /// render methods use this immutable view while the catalog remains
    /// owned by the menu/state runtime.
    pub fn lookup(&self, key: &str) -> String {
        self.active
            .get(key)
            .or_else(|| self.english.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string())
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

fn key_component(value: &str) -> String {
    let mut key = String::with_capacity(value.len());
    let mut previous_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase()
                && !key.is_empty()
                && !previous_separator
                && key
                    .as_bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_lowercase())
            {
                key.push('_');
            }
            key.push(ch.to_ascii_lowercase());
            previous_separator = false;
        } else if !previous_separator {
            key.push('_');
            previous_separator = true;
        }
    }
    while key.ends_with('_') {
        key.pop();
    }
    key
}

fn entity_debug_name(entity: EntityType) -> String {
    format!("{entity:?}")
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

    #[test]
    fn selected_pack_locale_is_used_and_german_missing_keys_fall_back_to_english() {
        let root = std::env::temp_dir().join(format!(
            "icraft_locale_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let builtin = root.join("builtin");
        let user = root.join("resourcepacks");
        let pack = user.join("selected");
        std::fs::create_dir_all(builtin.join("lang")).unwrap();
        std::fs::write(
            builtin.join("pack.json"),
            r#"{"id":"icraft.builtin","name":"builtin","version":"1","format":1,"description":"builtin"}"#,
        )
        .unwrap();
        std::fs::write(
            builtin.join("lang/en_us.json"),
            br#"{"hello":"Built-in","fallback":"English"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(pack.join("lang")).unwrap();
        std::fs::write(
            pack.join("pack.json"),
            r#"{"id":"test.selected","name":"selected","version":"1","format":1,"description":"selected"}"#,
        )
        .unwrap();
        std::fs::write(
            pack.join("lang/en_us.json"),
            br#"{"hello":"Selected","fallback":"Selected English"}"#,
        )
        .unwrap();
        std::fs::write(pack.join("lang/de_de.json"), br#"{"hello":"Deutsch"}"#).unwrap();

        let mut manager = ResourcePackManager::discover(&builtin, &user);
        manager.apply_enabled_order(["test.selected"]).unwrap();
        let mut catalog =
            TranslationCatalog::from_resource_packs_mut(&mut manager, Language::German);
        assert_eq!(catalog.translate("hello"), "Deutsch");
        assert_eq!(catalog.translate("fallback"), "Selected English");
        assert!(catalog.missing_keys().contains(&"fallback".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_selected_locale_falls_back_and_reports_once() {
        let root = std::env::temp_dir().join(format!(
            "icraft_locale_bad_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let builtin = root.join("builtin");
        let user = root.join("resourcepacks");
        let pack = user.join("bad");
        std::fs::create_dir_all(builtin.join("lang")).unwrap();
        std::fs::write(
            builtin.join("pack.json"),
            r#"{"id":"icraft.builtin","name":"builtin","version":"1","format":1,"description":"builtin"}"#,
        )
        .unwrap();
        std::fs::write(builtin.join("lang/en_us.json"), br#"{"hello":"English"}"#).unwrap();
        std::fs::create_dir_all(pack.join("lang")).unwrap();
        std::fs::write(
            pack.join("pack.json"),
            r#"{"id":"test.bad","name":"bad","version":"1","format":1,"description":"bad"}"#,
        )
        .unwrap();
        std::fs::write(pack.join("lang/en_us.json"), [0xff, 0xfe]).unwrap();

        let mut manager = ResourcePackManager::discover(&builtin, &user);
        manager.apply_enabled_order(["test.bad"]).unwrap();
        let mut catalog =
            TranslationCatalog::from_resource_packs_mut(&mut manager, Language::English);
        assert_eq!(catalog.translate("hello"), "English");
        let count = manager.diagnostics().len();
        assert!(count >= 1);
        manager.resolve_locale(Language::English.code());
        assert_eq!(manager.diagnostics().len(), count);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn selected_pack_can_override_item_block_and_entity_names() {
        let catalog = TranslationCatalog::from_json(
            Language::German,
            r#"{
                "item.diamond":"Diamond (built-in)",
                "block.stone":"Stone (built-in)",
                "entity.zombie":"Zombie (built-in)"
            }"#,
            r#"{
                "item.diamond":"Diamant",
                "block.stone":"Stein",
                "entity.zombie":"Zombie",
                "entity.ender_dragon":"Enderdrache"
            }"#,
        );
        assert_eq!(catalog.item_name(Item::Diamond), "Diamant");
        assert_eq!(catalog.block_name(BlockType::Stone), "Stein");
        assert_eq!(catalog.entity_name(EntityType::Zombie), "Zombie");
        assert_eq!(catalog.entity_name(EntityType::EnderDragon), "Enderdrache");
    }

    #[test]
    fn missing_named_keys_use_builtin_display_names() {
        let catalog = TranslationCatalog::from_json(Language::English, "{}", "{}");
        assert_eq!(catalog.item_name(Item::Diamond), "Diamond");
        assert_eq!(catalog.block_name(BlockType::Stone), "Stone");
        assert_eq!(catalog.entity_name(EntityType::Zombie), "Zombie");
    }
}
