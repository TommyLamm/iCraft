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

pub const VISIBLE_REQUIRED_KEYS: &[&str] = &[
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
    "menu.host_game",
    "menu.join_game",
    "menu.port",
    "menu.server_address",
    "menu.username",
    "menu.ping_server",
    "menu.connect",
    "menu.no_worlds",
    "menu.scroll_more_worlds",
    "menu.play_selected",
    "menu.create_new_world",
    "menu.delete",
    "menu.copy",
    "menu.backup",
    "menu.world_name",
    "menu.random",
    "menu.seed",
    "menu.game_mode",
    "menu.difficulty",
    "menu.world_type",
    "menu.structures",
    "menu.hardcore",
    "menu.bonus_chest",
    "menu.cheats",
    "menu.fov",
    "menu.render_distance",
    "menu.fullscreen",
    "menu.vsync",
    "menu.fps_cap",
    "menu.master_volume",
    "menu.music_volume",
    "menu.sound_volume",
    "menu.weather_volume",
    "menu.no_user_packs",
    "menu.mouse_sensitivity",
    "menu.press_a_key",
    "menu.delete_world",
    "menu.delete_warning",
    "menu.on",
    "menu.off",
    "menu.ui_scale_value",
    "menu.chat_scale_value",
    "menu.chat_opacity_value",
    "menu.setting_value",
    "menu.control_value",
    "menu.control_forward",
    "menu.control_backward",
    "menu.control_left",
    "menu.control_right",
    "menu.control_jump",
    "menu.control_sprint",
    "menu.control_sneak",
    "menu.control_inventory",
    "hud.save_failed",
    "hud.retry",
    "hud.quit_without_saving",
    "hud.saving_world",
    "hud.connection_lost",
    "hud.return_to_menu",
    "hud.you_died",
    "hud.respawn",
    "hud.game_paused",
    "hud.resume",
    "hud.fov",
    "hud.sensitivity",
    "hud.render_distance",
    "hud.master_volume",
    "hud.weather_volume",
    "hud.save_and_quit",
    "inventory.creative",
    "inventory.hotbar",
    "inventory.inventory",
    "inventory.crafting",
    "inventory.furnace",
    "inventory.book",
    "inventory.recipes",
    "station.enchanting",
    "station.level_bookshelves",
    "station.cost_lapis",
    "station.no_enchantment",
    "station.brewing_stand",
    "station.brewing_progress",
    "station.add_bottles_ingredient",
    "station.anvil",
    "station.type_a_name",
    "station.cost_levels",
    "station.villager_trading",
    "station.trade",
    "command.host_only",
    "command.disabled",
    "command.accepted",
    "command.rejected",
    "command.queued",
    "command.game_rule_updated_authority",
    "command.time_now",
    "command.only_local_player",
    "command.hardcore_survival",
    "command.gamemode_set",
    "command.difficulty_set",
    "command.gamerule_updated",
    "command.gamerule_invalid",
    "command.time_set",
    "command.weather_set",
    "command.teleported",
    "command.teleport_outside",
    "command.gave",
    "command.killed",
    "command.spawn_point_set",
    "command.world_spawn_set",
    "command.world_spawn_outside",
    "command.nearest_structure",
    "command.no_structure",
    "command.unknown_structure",
    "command.seed",
    "command.saved",
    "command.save_failed",
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
        let english = include_str!("../assets/lang/en_us.json");
        let active = match language {
            Language::English => english,
            Language::German => include_str!("../assets/lang/de_de.json"),
        };
        Self::from_json(language, english, active)
    }

    pub fn from_json(language: Language, english_json: &str, active_json: &str) -> Self {
        let english = parse_map(english_json).unwrap_or_default();
        let active = parse_map(active_json).unwrap_or_default();
        Self::from_maps(language, english, active)
    }

    fn from_maps(
        language: Language,
        english: HashMap<String, String>,
        active: HashMap<String, String>,
    ) -> Self {
        let active = if language == Language::English && active.is_empty() {
            english.clone()
        } else {
            active
        };
        Self {
            language,
            english,
            active,
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
    /// lower-priority layers are merged so a partial selected locale does not
    /// hide keys supplied by another enabled pack or the built-in catalog.
    pub fn from_resource_packs_mut(manager: &mut ResourcePackManager, language: Language) -> Self {
        let english_layers = manager.resolve_locale_layers(Language::English.code());
        let english = if english_layers.is_empty() {
            parse_map(include_str!("../assets/lang/en_us.json")).unwrap_or_default()
        } else {
            merge_locale_layers(english_layers)
        };
        let active_layers = manager.resolve_locale_layers(language.code());
        let active = if active_layers.is_empty() {
            if language == Language::English {
                english.clone()
            } else {
                parse_map(include_str!("../assets/lang/de_de.json")).unwrap_or_default()
            }
        } else {
            merge_locale_layers(active_layers)
        };
        Self::from_maps(language, english, active)
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
        replace_tokens(self.translate(key), arguments)
    }

    /// Format a visible UI string without mutating the missing-key diagnostic
    /// set. Render paths are called every frame, so they use this immutable
    /// helper while command/test paths may continue to use `format`.
    pub fn format_lookup(&self, key: &str, arguments: &[(&str, &str)]) -> String {
        replace_tokens(self.lookup(key), arguments)
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

    pub fn visible_coverage(&self) -> f32 {
        if VISIBLE_REQUIRED_KEYS.is_empty() {
            return 1.0;
        }
        VISIBLE_REQUIRED_KEYS
            .iter()
            .filter(|key| self.active.contains_key(**key))
            .count() as f32
            / VISIBLE_REQUIRED_KEYS.len() as f32
    }

    pub fn validate_visible_keys(&self) -> Vec<String> {
        VISIBLE_REQUIRED_KEYS
            .iter()
            .filter(|key| !self.english.contains_key(**key))
            .map(|key| (*key).to_string())
            .collect()
    }
}

fn parse_map(json: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    serde_json::from_str(json)
}

fn merge_locale_layers(layers: Vec<std::sync::Arc<[u8]>>) -> HashMap<String, String> {
    let mut merged = HashMap::new();
    for bytes in layers.into_iter().rev() {
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let Ok(layer) = parse_map(text) else {
            continue;
        };
        merged.extend(layer);
    }
    merged
}

fn key_component(value: &str) -> String {
    let mut key = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && key.chars().last().is_some_and(|c| c.is_ascii_lowercase())
            {
                key.push('_');
            }
            key.push(ch.to_ascii_lowercase());
        } else if !key.is_empty() && !key.ends_with('_') {
            key.push('_');
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

fn replace_tokens(mut template: String, arguments: &[(&str, &str)]) -> String {
    for (name, replacement) in arguments {
        template = template.replace(&format!("{{{name}}}"), replacement);
    }
    template
}

fn builtin_catalog(language: Language) -> &'static TranslationCatalog {
    static ENGLISH: OnceLock<TranslationCatalog> = OnceLock::new();
    static GERMAN: OnceLock<TranslationCatalog> = OnceLock::new();
    match language {
        Language::English => ENGLISH.get_or_init(|| TranslationCatalog::builtin(Language::English)),
        Language::German => GERMAN.get_or_init(|| TranslationCatalog::builtin(Language::German)),
    }
}

pub fn translate(language: Language, key: &str) -> String {
    builtin_catalog(language).lookup(key)
}

pub fn format(language: Language, key: &str, arguments: &[(&str, &str)]) -> String {
    builtin_catalog(language).format_lookup(key, arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_keys_have_builtin_english_and_german_values() {
        let english = TranslationCatalog::builtin(Language::English);
        let german = TranslationCatalog::builtin(Language::German);
        assert!(english.validate_visible_keys().is_empty());
        assert!(german.validate_visible_keys().is_empty());
        assert!((english.visible_coverage() - 1.0).abs() < f32::EPSILON);
        assert!((german.visible_coverage() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn partial_selected_locales_merge_without_swallowing_lower_layers() {
        let root = std::env::temp_dir().join(format!(
            "icraft_locale_layers_test_{}_{}",
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
            br#"{"hello":"Built-in","fallback":"Built-in fallback","menu.multiplayer":"Built-in multiplayer"}"#,
        )
        .unwrap();
        std::fs::write(
            builtin.join("lang/de_de.json"),
            br#"{"hello":"Deutsch builtin","fallback":"Deutscher fallback"}"#,
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
            br#"{"hello":"Selected","menu.multiplayer":"Selected multiplayer"}"#,
        )
        .unwrap();
        std::fs::write(
            pack.join("lang/de_de.json"),
            br#"{"hello":"Deutsch selected"}"#,
        )
        .unwrap();

        let mut manager = ResourcePackManager::discover(&builtin, &user);
        manager.apply_enabled_order(["test.selected"]).unwrap();
        let mut catalog =
            TranslationCatalog::from_resource_packs_mut(&mut manager, Language::German);
        assert_eq!(catalog.translate("hello"), "Deutsch selected");
        assert_eq!(catalog.translate("fallback"), "Deutscher fallback");
        assert_eq!(catalog.lookup("menu.multiplayer"), "Selected multiplayer");

        let english = TranslationCatalog::from_resource_packs_mut(&mut manager, Language::English);
        assert_eq!(english.lookup("fallback"), "Built-in fallback");
        assert_eq!(english.lookup("menu.multiplayer"), "Selected multiplayer");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn immutable_format_and_language_switch_use_catalog_values() {
        let english = TranslationCatalog::builtin(Language::English);
        let german = TranslationCatalog::builtin(Language::German);
        assert_ne!(
            english.lookup("menu.multiplayer"),
            german.lookup("menu.multiplayer")
        );
        assert_eq!(
            english.format_lookup("hud.fov", &[("value", "90")]),
            "FOV < 90 >"
        );
        assert_eq!(
            german.format_lookup("hud.fov", &[("value", "90")]),
            "SICHTFELD < 90 >"
        );
        assert_eq!(
            english.format_lookup("menu.game_mode", &[("value", "SURVIVAL")]),
            "GAME MODE: < SURVIVAL >"
        );
        assert_eq!(
            german.format_lookup("menu.world_type", &[("value", "DEFAULT")]),
            "WELTTYP: < DEFAULT >"
        );
        assert_eq!(
            english.format_lookup(
                "station.cost_lapis",
                &[
                    ("enchantment", "SHARPNESS I"),
                    ("cost", "3"),
                    ("lapis", "2")
                ],
            ),
            "SHARPNESS I  COST 3 + 2 LAPIS"
        );
        assert_eq!(
            english.format_lookup("hud.master_volume", &[("value", "80")]),
            "MASTER VOLUME < 80% >"
        );
        assert_eq!(
            german.format_lookup("station.brewing_progress", &[("value", "50")]),
            "BRAUEN 50 PROZENT"
        );
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
        let _ = manager.resolve_locale_layers(Language::English.code());
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

    #[test]
    fn top_level_translate_and_format_and_key_component() {
        assert_eq!(
            translate(Language::English, "menu.singleplayer"),
            "SINGLEPLAYER"
        );
        assert_eq!(
            translate(Language::German, "menu.singleplayer"),
            "EINZELSPIELER"
        );
        assert_eq!(
            format(Language::English, "hud.fov", &[("value", "90")]),
            "FOV < 90 >"
        );
        assert_eq!(
            format(Language::German, "hud.fov", &[("value", "90")]),
            "SICHTFELD < 90 >"
        );
        assert_eq!(key_component("Diamond"), "diamond");
        assert_eq!(key_component("Oak Planks"), "oak_planks");
        assert_eq!(key_component("EnderDragon"), "ender_dragon");
        assert_eq!(key_component("  Multiple   Spaces  "), "multiple_spaces");
    }
}
