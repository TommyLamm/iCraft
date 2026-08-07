//! Bounded iCraft resource-pack discovery and asset resolution.
//!
//! This is deliberately a small, iCraft-owned format.  It understands the
//! subset needed by the client (`textures`, `sounds`, `font`, `lang`, and
//! model descriptors) without pretending to be a complete Minecraft pack
//! loader.  Packs are read into bounded byte maps instead of being extracted,
//! so a malformed archive cannot write outside the workspace.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

pub const MAX_PACK_ENTRIES: usize = 4096;
pub const MAX_PACK_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_PACK_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_COMPRESSION_RATIO: u64 = 1_000;
const BUILTIN_PACK_ID: &str = "icraft.builtin";
const MAX_PACK_DESCRIPTION_BYTES: usize = 1024;
const MAX_PACK_DEPENDENCIES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourcePackManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub format: u32,
    pub description: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

impl ResourcePackManifest {
    pub fn validate(&self) -> Result<(), PackError> {
        if !valid_pack_id(&self.id) {
            return Err(PackError::InvalidManifest(
                "id must use lowercase ASCII letters, digits, '.', '-' or '_'".into(),
            ));
        }
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(PackError::InvalidManifest(
                "name must contain 1..=128 bytes".into(),
            ));
        }
        if self.version.trim().is_empty() || self.version.len() > 32 {
            return Err(PackError::InvalidManifest(
                "version must contain 1..=32 bytes".into(),
            ));
        }
        if self.format == 0 || self.format > 1 {
            return Err(PackError::InvalidManifest(
                "unsupported iCraft pack format".into(),
            ));
        }
        if self.description.len() > MAX_PACK_DESCRIPTION_BYTES {
            return Err(PackError::InvalidManifest(
                "description must be at most 1024 bytes".into(),
            ));
        }
        if self.dependencies.len() > MAX_PACK_DEPENDENCIES {
            return Err(PackError::InvalidManifest(
                "dependencies exceed the 128-entry limit".into(),
            ));
        }
        let mut seen = HashSet::new();
        for dependency in &self.dependencies {
            if !valid_pack_id(dependency) {
                return Err(PackError::InvalidManifest(format!(
                    "dependency id is invalid: {dependency}"
                )));
            }
            if dependency == &self.id || !seen.insert(dependency) {
                return Err(PackError::InvalidManifest(
                    "dependencies must be unique and cannot include the pack itself".into(),
                ));
            }
        }
        Ok(())
    }
}

fn valid_pack_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ".-_".contains(ch))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    Io(String),
    InvalidManifest(String),
    UnsafePath(String),
    EntryTooLarge(String),
    PackTooLarge,
    TooManyEntries,
    CompressionBomb(String),
    Archive(String),
    MissingDependency { pack: String, dependency: String },
    DependencyCycle(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(f, "I/O error: {message}"),
            Self::InvalidManifest(message) => write!(f, "invalid manifest: {message}"),
            Self::UnsafePath(path) => write!(f, "unsafe archive path: {path}"),
            Self::EntryTooLarge(path) => write!(f, "resource-pack entry too large: {path}"),
            Self::PackTooLarge => write!(f, "resource pack exceeds the byte budget"),
            Self::TooManyEntries => write!(f, "resource pack has too many entries"),
            Self::CompressionBomb(path) => write!(f, "compression ratio is unsafe: {path}"),
            Self::Archive(message) => write!(f, "invalid zip archive: {message}"),
            Self::MissingDependency { pack, dependency } => {
                write!(f, "pack {pack} requires missing dependency {dependency}")
            }
            Self::DependencyCycle(pack) => write!(f, "resource-pack dependency cycle at {pack}"),
        }
    }
}

impl std::error::Error for PackError {}

impl From<io::Error> for PackError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePackSummary {
    pub manifest: ResourcePackManifest,
    pub source: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackDiagnostic {
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone)]
struct LoadedPack {
    manifest: ResourcePackManifest,
    source: String,
    assets: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ResourcePackManager {
    builtin_root: PathBuf,
    user_root: PathBuf,
    override_path: Option<PathBuf>,
    packs: Vec<LoadedPack>,
    enabled_order: Vec<String>,
    diagnostics: Vec<PackDiagnostic>,
    diagnostic_keys: HashSet<String>,
}

impl Default for ResourcePackManager {
    fn default() -> Self {
        Self::discover_default()
    }
}

impl ResourcePackManager {
    pub fn discover_default() -> Self {
        let override_path = std::env::var_os("ICRAFT_RESOURCE_PACK").map(PathBuf::from);
        Self::discover_with_override("assets", "resourcepacks", override_path)
    }

    pub fn discover(builtin_root: impl Into<PathBuf>, user_root: impl Into<PathBuf>) -> Self {
        Self::discover_with_override(builtin_root, user_root, None)
    }

    /// Discover packs with an explicit development/test override.  The
    /// override is intentionally injected by the caller so tests never need
    /// to mutate process-wide environment variables.  When present it is the
    /// only user pack root considered; the normal workspace `resourcepacks`
    /// directory is not implicitly mixed into the override.
    pub fn discover_with_override(
        builtin_root: impl Into<PathBuf>,
        user_root: impl Into<PathBuf>,
        override_path: Option<PathBuf>,
    ) -> Self {
        let mut manager = Self {
            builtin_root: builtin_root.into(),
            user_root: user_root.into(),
            override_path,
            packs: Vec::new(),
            enabled_order: Vec::new(),
            diagnostics: Vec::new(),
            diagnostic_keys: HashSet::new(),
        };
        if let Err(error) = manager.reload() {
            manager.record_diagnostic(Path::new("resourcepacks"), &error.to_string());
        }
        manager
    }

    pub fn reload(&mut self) -> Result<(), PackError> {
        self.packs.clear();
        self.enabled_order.clear();
        self.diagnostics.clear();
        self.diagnostic_keys.clear();

        let builtin = load_directory_pack(&self.builtin_root)
            .map_err(|error| PackError::InvalidManifest(format!("built-in assets: {error}")))?;
        if builtin.manifest.id != BUILTIN_PACK_ID {
            return Err(PackError::InvalidManifest(format!(
                "built-in pack id must be {BUILTIN_PACK_ID}"
            )));
        }
        self.packs.push(builtin);

        let mut candidates = if let Some(path) = &self.override_path {
            vec![path.clone()]
        } else {
            fs::read_dir(&self.user_root)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        candidates.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
        for path in candidates {
            let loaded = load_pack_path(&path);
            match loaded {
                Ok(pack) => {
                    if self
                        .packs
                        .iter()
                        .any(|existing| existing.manifest.id == pack.manifest.id)
                    {
                        self.record_diagnostic(&path, "duplicate pack id; skipped");
                    } else {
                        self.packs.push(pack);
                    }
                }
                Err(error) => self.record_diagnostic(&path, &error.to_string()),
            }
        }

        let order = self.dependency_order()?;
        self.enabled_order = order
            .into_iter()
            .filter(|id| id != BUILTIN_PACK_ID)
            .collect();
        Ok(())
    }

    pub fn apply_enabled_order<I, S>(&mut self, ids: I) -> Result<(), PackError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let requested: Vec<String> = ids.into_iter().map(|id| id.as_ref().to_string()).collect();
        let available: HashSet<_> = self
            .packs
            .iter()
            .map(|pack| pack.manifest.id.as_str())
            .collect();
        let mut seen = HashSet::new();
        for id in &requested {
            if id == BUILTIN_PACK_ID || !available.contains(id.as_str()) {
                return Err(PackError::MissingDependency {
                    pack: id.clone(),
                    dependency: "selected pack".into(),
                });
            }
            if !seen.insert(id) {
                return Err(PackError::InvalidManifest(
                    "enabled order contains duplicates".into(),
                ));
            }
        }
        for id in &requested {
            let pack = self
                .packs
                .iter()
                .find(|pack| pack.manifest.id == *id)
                .expect("available pack checked above");
            for dependency in &pack.manifest.dependencies {
                if dependency != BUILTIN_PACK_ID && !seen.contains(dependency) {
                    return Err(PackError::MissingDependency {
                        pack: id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        let requested_set: HashSet<&str> = requested.iter().map(String::as_str).collect();
        let topological = self.dependency_order()?;
        self.enabled_order = topological
            .into_iter()
            .filter(|id| id != BUILTIN_PACK_ID && requested_set.contains(id.as_str()))
            .collect();
        Ok(())
    }

    pub fn available(&self) -> Vec<ResourcePackSummary> {
        self.packs
            .iter()
            .filter(|pack| pack.manifest.id != BUILTIN_PACK_ID)
            .map(|pack| ResourcePackSummary {
                manifest: pack.manifest.clone(),
                source: pack.source.clone(),
                enabled: self.enabled_order.contains(&pack.manifest.id),
            })
            .collect()
    }

    pub fn enabled_order(&self) -> &[String] {
        &self.enabled_order
    }

    pub fn diagnostics(&self) -> &[PackDiagnostic] {
        &self.diagnostics
    }

    pub fn take_diagnostics(&mut self) -> Vec<PackDiagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Resolve a logical asset path from the highest-priority enabled pack,
    /// then the built-in pack.  Missing assets are diagnosed once per path.
    pub fn resolve_asset(&mut self, relative: &str) -> Option<Vec<u8>> {
        let value = self.read_asset(relative);
        if value.is_none() {
            self.record_diagnostic(
                Path::new(relative),
                "asset missing; using procedural fallback",
            );
        }
        value
    }

    /// Resolve and validate a texture through the pack manager.  Invalid
    /// overrides are skipped so a valid built-in texture still wins over a
    /// corrupt selected-pack entry.
    pub fn resolve_texture(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_validated_asset(relative, "texture", |bytes| {
            image::load_from_memory(bytes).is_ok()
        })
    }

    pub fn texture_bytes(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_texture(relative)
    }

    /// Resolve a JSON item/block model descriptor.  The small client model
    /// format intentionally accepts any JSON object; malformed JSON and
    /// scalar/array descriptors fall back to the built-in/procedural model.
    pub fn resolve_model(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_validated_asset(relative, "model", |bytes| {
            serde_json::from_slice::<serde_json::Value>(bytes)
                .map(|value| value.is_object())
                .unwrap_or(false)
        })
    }

    pub fn model_bytes(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_model(relative)
    }

    /// Resolve a TrueType/OpenType/WebFont payload.  Font parsing is kept
    /// dependency-free and bounded by checking the format's mandatory magic.
    pub fn resolve_font(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_validated_asset(relative, "font", font_bytes_are_decodable)
    }

    pub fn font_bytes(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_font(relative)
    }

    /// Resolve a sound through the same manager entry point used by the audio
    /// consumer.  Rodio performs the actual bounded decoder validation.
    pub fn resolve_sound(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_validated_asset(relative, "sound", sound_bytes_are_decodable)
    }

    pub fn sound_bytes(&mut self, relative: &str) -> Option<Vec<u8>> {
        self.resolve_sound(relative)
    }

    pub fn read_asset(&self, relative: &str) -> Option<Vec<u8>> {
        let relative = normalize_logical_path(relative).ok()?;
        for id in self.enabled_order.iter().rev() {
            if let Some(pack) = self.packs.iter().find(|pack| pack.manifest.id == *id) {
                if let Some(bytes) = lookup_asset(pack, &relative) {
                    return Some(bytes.to_vec());
                }
            }
        }
        self.packs
            .iter()
            .find(|pack| pack.manifest.id == BUILTIN_PACK_ID)
            .and_then(|pack| lookup_asset(pack, &relative).map(ToOwned::to_owned))
    }

    pub fn locale_bytes(&self, language: &str) -> Option<Vec<u8>> {
        let language = normalize_locale_code(language)?;
        self.read_asset(&format!("lang/{language}.json"))
    }

    pub fn resolve_locale(&mut self, language: &str) -> Option<Vec<u8>> {
        let language = match normalize_locale_code(language) {
            Some(language) => language,
            None => {
                self.record_asset_diagnostic(
                    language,
                    "locale",
                    "invalid locale code; using built-in fallback",
                );
                return None;
            }
        };
        self.resolve_validated_asset(&format!("lang/{language}.json"), "locale", |bytes| {
            let Ok(text) = std::str::from_utf8(bytes) else {
                return false;
            };
            serde_json::from_str::<HashMap<String, String>>(text).is_ok()
        })
    }

    /// Record a consumer-side validation failure once for a logical asset.
    /// This is used by audio after a decoder-specific check and keeps the
    /// diagnostic key independent of the selected pack's physical path.
    pub fn record_asset_diagnostic(&mut self, relative: &str, kind: &str, message: &str) {
        let normalized =
            normalize_logical_path(relative).unwrap_or_else(|_| relative.replace('\\', "/"));
        let key = format!("asset::{kind}::{normalized}");
        if self.diagnostic_keys.insert(key) {
            self.diagnostics.push(PackDiagnostic {
                source: normalized,
                message: message.to_string(),
            });
        }
    }

    fn resolve_validated_asset<F>(
        &mut self,
        relative: &str,
        kind: &str,
        validator: F,
    ) -> Option<Vec<u8>>
    where
        F: Fn(&[u8]) -> bool,
    {
        let normalized = match normalize_logical_path(relative) {
            Ok(path) => path,
            Err(error) => {
                self.record_asset_diagnostic(relative, kind, &error.to_string());
                return None;
            }
        };
        let mut candidates = Vec::new();
        for id in self.enabled_order.iter().rev() {
            if let Some(pack) = self.packs.iter().find(|pack| pack.manifest.id == *id) {
                if let Some(bytes) = lookup_asset(pack, &normalized) {
                    candidates.push(bytes.to_vec());
                }
            }
        }
        if let Some(pack) = self
            .packs
            .iter()
            .find(|pack| pack.manifest.id == BUILTIN_PACK_ID)
        {
            if let Some(bytes) = lookup_asset(pack, &normalized) {
                candidates.push(bytes.to_vec());
            }
        }
        for bytes in candidates {
            if validator(&bytes) {
                return Some(bytes);
            }
            self.record_asset_diagnostic(
                &normalized,
                kind,
                &format!("invalid {kind} asset; using built-in/procedural fallback"),
            );
        }
        self.record_asset_diagnostic(
            &normalized,
            kind,
            &format!("{kind} asset missing or invalid; using built-in/procedural fallback"),
        );
        None
    }

    fn dependency_order(&self) -> Result<Vec<String>, PackError> {
        let by_id: HashMap<_, _> = self
            .packs
            .iter()
            .map(|pack| (pack.manifest.id.as_str(), pack))
            .collect();
        let mut state = HashMap::<&str, u8>::new();
        let mut output = Vec::new();
        for pack in &self.packs {
            visit_dependency(pack.manifest.id.as_str(), &by_id, &mut state, &mut output)?;
        }
        Ok(output)
    }

    fn record_diagnostic(&mut self, source: &Path, message: &str) {
        let key = format!("{}::{message}", source.display());
        if self.diagnostic_keys.insert(key) {
            self.diagnostics.push(PackDiagnostic {
                source: source.display().to_string(),
                message: message.to_string(),
            });
        }
    }
}

fn visit_dependency<'a>(
    id: &'a str,
    by_id: &HashMap<&'a str, &'a LoadedPack>,
    state: &mut HashMap<&'a str, u8>,
    output: &mut Vec<String>,
) -> Result<(), PackError> {
    match state.get(id).copied().unwrap_or(0) {
        1 => return Err(PackError::DependencyCycle(id.into())),
        2 => return Ok(()),
        _ => {}
    }
    state.insert(id, 1);
    let pack = by_id.get(id).ok_or_else(|| PackError::MissingDependency {
        pack: id.into(),
        dependency: "unknown".into(),
    })?;
    for dependency in &pack.manifest.dependencies {
        if !by_id.contains_key(dependency.as_str()) {
            return Err(PackError::MissingDependency {
                pack: id.into(),
                dependency: dependency.clone(),
            });
        }
        visit_dependency(dependency, by_id, state, output)?;
    }
    state.insert(id, 2);
    output.push(id.into());
    Ok(())
}

fn load_pack_path(path: &Path) -> Result<LoadedPack, PackError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(PackError::UnsafePath(path.display().to_string()));
    }
    if metadata.is_dir() {
        load_directory_pack(path)
    } else if metadata.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        load_zip_pack(path)
    } else {
        Err(PackError::InvalidManifest(format!(
            "resource-pack path is not a directory or .zip: {}",
            path.display()
        )))
    }
}

fn load_directory_pack(root: &Path) -> Result<LoadedPack, PackError> {
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(PackError::UnsafePath(root.display().to_string()));
    }
    let manifest_path = [root.join("pack.json"), root.join("assets/pack.json")]
        .into_iter()
        .find(|path| {
            fs::symlink_metadata(path)
                .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                .unwrap_or(false)
        })
        .ok_or_else(|| PackError::InvalidManifest("pack.json is missing".into()))?;
    let mut assets = HashMap::new();
    let mut manifest_bytes = None;
    let mut total = 0u64;
    let mut entries = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut children = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.path());
        for entry in children {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(PackError::UnsafePath(path.display().to_string()));
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            entries += 1;
            if entries > MAX_PACK_ENTRIES {
                return Err(PackError::TooManyEntries);
            }
            let size = metadata.len();
            if size > MAX_PACK_ENTRY_BYTES {
                return Err(PackError::EntryTooLarge(path.display().to_string()));
            }
            total = total.saturating_add(size);
            if total > MAX_PACK_BYTES {
                return Err(PackError::PackTooLarge);
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| PackError::UnsafePath(path.display().to_string()))?;
            let relative = normalize_logical_path(&relative.to_string_lossy())?;
            let bytes = fs::read(&path)?;
            if path == manifest_path {
                manifest_bytes = Some(bytes.clone());
            }
            insert_asset_aliases(&mut assets, &relative, bytes);
        }
    }
    let manifest = parse_manifest(
        &manifest_bytes.ok_or_else(|| PackError::InvalidManifest("pack.json is missing".into()))?,
    )?;
    Ok(LoadedPack {
        manifest,
        source: root.display().to_string(),
        assets,
    })
}

fn load_zip_pack(path: &Path) -> Result<LoadedPack, PackError> {
    // Keep archive handling dependency-free and bounded.  We parse the ZIP
    // central directory, then read only stored/deflate entries into memory;
    // no entry is ever extracted to disk.
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > MAX_PACK_BYTES {
        return Err(PackError::PackTooLarge);
    }
    let eocd = find_zip_end(&bytes)
        .ok_or_else(|| PackError::Archive("ZIP end record is missing".into()))?;
    if (eocd >= 20 && read_u32(&bytes, eocd - 20) == Some(0x0706_4b50))
        || bytes.get(..eocd).is_some_and(|prefix| {
            prefix
                .windows(4)
                .any(|window| window == [0x50, 0x4b, 0x06, 0x06])
        })
    {
        return Err(PackError::Archive(
            "ZIP64 archives are not supported".into(),
        ));
    }
    let entry_count = read_u16(&bytes, eocd + 10)
        .ok_or_else(|| PackError::Archive("truncated ZIP end record".into()))?
        as usize;
    if entry_count == u16::MAX as usize {
        return Err(PackError::Archive(
            "ZIP64 archives are not supported".into(),
        ));
    }
    let central_size = read_u32(&bytes, eocd + 12)
        .ok_or_else(|| PackError::Archive("truncated ZIP end record".into()))?
        as usize;
    let central_offset = read_u32(&bytes, eocd + 16)
        .ok_or_else(|| PackError::Archive("truncated ZIP end record".into()))?
        as usize;
    if central_size == u32::MAX as usize || central_offset == u32::MAX as usize {
        return Err(PackError::Archive(
            "ZIP64 archives are not supported".into(),
        ));
    }
    if entry_count > MAX_PACK_ENTRIES {
        return Err(PackError::TooManyEntries);
    }
    if central_offset.checked_add(central_size).is_none()
        || central_offset + central_size > bytes.len()
    {
        return Err(PackError::Archive(
            "ZIP central directory is out of bounds".into(),
        ));
    }
    let mut assets = HashMap::new();
    let mut manifest_bytes = None;
    let mut total = 0u64;
    let mut cursor = central_offset;
    for _ in 0..entry_count {
        if read_u32(&bytes, cursor) != Some(0x0201_4b50) || cursor + 46 > bytes.len() {
            return Err(PackError::Archive(
                "invalid ZIP central directory entry".into(),
            ));
        }
        let flags = read_u16(&bytes, cursor + 8)
            .ok_or_else(|| PackError::Archive("truncated ZIP flags".into()))?;
        let compression = read_u16(&bytes, cursor + 10)
            .ok_or_else(|| PackError::Archive("truncated ZIP method".into()))?;
        let compressed_size = read_u32(&bytes, cursor + 20)
            .ok_or_else(|| PackError::Archive("truncated ZIP compressed size".into()))?
            as u64;
        let central_crc = read_u32(&bytes, cursor + 16)
            .ok_or_else(|| PackError::Archive("truncated ZIP CRC".into()))?;
        let uncompressed_size = read_u32(&bytes, cursor + 24)
            .ok_or_else(|| PackError::Archive("truncated ZIP uncompressed size".into()))?
            as u64;
        let name_len = read_u16(&bytes, cursor + 28)
            .ok_or_else(|| PackError::Archive("truncated ZIP name length".into()))?
            as usize;
        let extra_len = read_u16(&bytes, cursor + 30)
            .ok_or_else(|| PackError::Archive("truncated ZIP extra length".into()))?
            as usize;
        let comment_len = read_u16(&bytes, cursor + 32)
            .ok_or_else(|| PackError::Archive("truncated ZIP comment length".into()))?
            as usize;
        let local_offset = read_u32(&bytes, cursor + 42)
            .ok_or_else(|| PackError::Archive("truncated ZIP local offset".into()))?
            as usize;
        let record_len = 46usize
            .checked_add(name_len)
            .and_then(|value| value.checked_add(extra_len))
            .and_then(|value| value.checked_add(comment_len))
            .ok_or_else(|| PackError::Archive("ZIP entry length overflow".into()))?;
        if cursor + record_len > central_offset + central_size {
            return Err(PackError::Archive(
                "ZIP central entry is out of bounds".into(),
            ));
        }
        if compressed_size == u32::MAX as u64
            || uncompressed_size == u32::MAX as u64
            || local_offset == u32::MAX as usize
        {
            return Err(PackError::Archive(
                "ZIP64 archives are not supported".into(),
            ));
        }
        let raw_name =
            String::from_utf8_lossy(&bytes[cursor + 46..cursor + 46 + name_len]).to_string();
        let relative = normalize_logical_path(&raw_name)?;
        let external_attributes = read_u32(&bytes, cursor + 38).unwrap_or_default();
        let unix_mode = external_attributes >> 16;
        if unix_mode & 0o170000 == 0o120000 {
            return Err(PackError::UnsafePath(raw_name));
        }
        cursor += record_len;
        // Directory entries carry no asset bytes. Validate their path before
        // skipping so names such as `../` cannot bypass the path guard.
        if raw_name.ends_with('/') {
            continue;
        }
        if flags & 0x1 != 0 {
            return Err(PackError::Archive(format!(
                "encrypted ZIP entry: {raw_name}"
            )));
        }
        if uncompressed_size > MAX_PACK_ENTRY_BYTES {
            return Err(PackError::EntryTooLarge(raw_name));
        }
        if uncompressed_size > compressed_size.saturating_mul(MAX_COMPRESSION_RATIO) {
            return Err(PackError::CompressionBomb(raw_name));
        }
        total = total.saturating_add(uncompressed_size);
        if total > MAX_PACK_BYTES {
            return Err(PackError::PackTooLarge);
        }
        let local_end = local_offset
            .checked_add(30)
            .ok_or_else(|| PackError::Archive("ZIP local header overflow".into()))?;
        if local_end > bytes.len() || read_u32(&bytes, local_offset) != Some(0x0403_4b50) {
            return Err(PackError::Archive("invalid ZIP local header".into()));
        }
        let local_name_len = read_u16(&bytes, local_offset + 26)
            .ok_or_else(|| PackError::Archive("truncated ZIP local name length".into()))?
            as usize;
        let local_extra_len = read_u16(&bytes, local_offset + 28)
            .ok_or_else(|| PackError::Archive("truncated ZIP local extra length".into()))?
            as usize;
        let local_flags = read_u16(&bytes, local_offset + 6)
            .ok_or_else(|| PackError::Archive("truncated ZIP local flags".into()))?;
        let local_compression = read_u16(&bytes, local_offset + 8)
            .ok_or_else(|| PackError::Archive("truncated ZIP local method".into()))?;
        let local_crc = read_u32(&bytes, local_offset + 14)
            .ok_or_else(|| PackError::Archive("truncated ZIP local CRC".into()))?;
        if local_flags != flags || local_compression != compression {
            return Err(PackError::Archive(format!(
                "ZIP local/central header mismatch for {raw_name}"
            )));
        }
        if flags & 0x08 == 0 && local_crc != central_crc {
            return Err(PackError::Archive(format!(
                "ZIP local CRC mismatch for {raw_name}"
            )));
        }
        let local_name_end = local_end
            .checked_add(local_name_len)
            .ok_or_else(|| PackError::Archive("ZIP local name length overflow".into()))?;
        let local_extra_end = local_name_end
            .checked_add(local_extra_len)
            .ok_or_else(|| PackError::Archive("ZIP local extra length overflow".into()))?;
        if local_extra_end > bytes.len() {
            return Err(PackError::Archive("ZIP local header is truncated".into()));
        }
        if bytes.get(local_end..local_name_end) != Some(raw_name.as_bytes()) {
            return Err(PackError::Archive(format!(
                "ZIP local/central name mismatch for {raw_name}"
            )));
        }
        let data_offset = local_end
            .checked_add(local_name_len)
            .and_then(|value| value.checked_add(local_extra_len))
            .ok_or_else(|| PackError::Archive("ZIP data offset overflow".into()))?;
        let data_end = data_offset
            .checked_add(compressed_size as usize)
            .ok_or_else(|| PackError::Archive("ZIP data length overflow".into()))?;
        if data_end > bytes.len() {
            return Err(PackError::Archive("ZIP entry data is out of bounds".into()));
        }
        let compressed_bytes = &bytes[data_offset..data_end];
        let data = match compression {
            0 => compressed_bytes.to_vec(),
            8 => {
                let decoder = flate2::read::DeflateDecoder::new(compressed_bytes);
                let mut decoded = Vec::with_capacity(uncompressed_size as usize);
                decoder
                    .take(MAX_PACK_ENTRY_BYTES + 1)
                    .read_to_end(&mut decoded)
                    .map_err(|error| PackError::Archive(error.to_string()))?;
                decoded
            }
            method => {
                return Err(PackError::Archive(format!(
                    "unsupported ZIP compression method {method}"
                )))
            }
        };
        if data.len() as u64 != uncompressed_size {
            return Err(PackError::Archive(format!(
                "ZIP size mismatch for {raw_name}"
            )));
        }
        if crc32(&data) != central_crc {
            return Err(PackError::Archive(format!(
                "ZIP CRC mismatch for {raw_name}"
            )));
        }
        if relative == "pack.json" || relative == "assets/pack.json" {
            manifest_bytes = Some(data.clone());
        }
        insert_asset_aliases(&mut assets, &relative, data);
    }
    if cursor != central_offset + central_size {
        return Err(PackError::Archive(
            "ZIP central directory size does not match entry count".into(),
        ));
    }
    let manifest = parse_manifest(
        &manifest_bytes.ok_or_else(|| PackError::InvalidManifest("pack.json is missing".into()))?,
    )?;
    Ok(LoadedPack {
        manifest,
        source: path.display().to_string(),
        assets,
    })
}

fn find_zip_end(bytes: &[u8]) -> Option<usize> {
    let start = bytes.len().saturating_sub(65_557);
    (start..bytes.len().saturating_sub(3))
        .rev()
        .find(|&index| read_u32(bytes, index) == Some(0x0605_4b50))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn parse_manifest(bytes: &[u8]) -> Result<ResourcePackManifest, PackError> {
    let manifest: ResourcePackManifest = serde_json::from_slice(bytes)
        .map_err(|error| PackError::InvalidManifest(error.to_string()))?;
    manifest.validate()?;
    Ok(manifest)
}

fn normalize_logical_path(raw: &str) -> Result<String, PackError> {
    let raw = raw.replace('\\', "/");
    if raw.is_empty() || raw.starts_with('/') || raw.contains(':') {
        return Err(PackError::UnsafePath(raw));
    }
    let mut parts = Vec::new();
    for component in raw.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(PackError::UnsafePath(raw));
        }
        parts.push(component);
    }
    if parts.is_empty() {
        return Err(PackError::UnsafePath(raw));
    }
    let normalized = parts.join("/");
    if normalized.len() > 256 {
        return Err(PackError::UnsafePath(normalized));
    }
    Ok(normalized)
}

fn normalize_locale_code(raw: &str) -> Option<String> {
    let code = raw.trim().to_ascii_lowercase();
    if code.is_empty()
        || code.len() > 32
        || !code
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    {
        return None;
    }
    Some(code)
}

fn font_bytes_are_decodable(bytes: &[u8]) -> bool {
    bytes.starts_with(b"OTTO")
        || bytes.starts_with(b"true")
        || bytes.starts_with(b"typ1")
        || bytes.starts_with(b"ttcf")
        || bytes.starts_with(b"wOFF")
        || bytes.starts_with(b"wOF2")
        || bytes.starts_with(&[0, 1, 0, 0])
}

pub(crate) fn sound_bytes_are_decodable(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    rodio::Decoder::new(Cursor::new(bytes.to_vec())).is_ok()
}

fn insert_asset_aliases(assets: &mut HashMap<String, Vec<u8>>, relative: &str, bytes: Vec<u8>) {
    let aliases = logical_aliases(relative);
    for alias in aliases {
        assets.entry(alias).or_insert_with(|| bytes.clone());
    }
}

fn logical_aliases(relative: &str) -> Vec<String> {
    let mut aliases = vec![relative.to_string()];
    for prefix in ["assets/minecraft/", "assets/vanilla/"] {
        if let Some(stripped) = relative.strip_prefix(prefix) {
            aliases.push(stripped.to_string());
        }
    }
    if let Some(stripped) = relative.strip_prefix("vanilla/textures/") {
        aliases.push(stripped.to_string());
    }
    if let Some(stripped) = relative.strip_prefix("assets/vanilla/textures/") {
        aliases.push(stripped.to_string());
    }
    if let Some(stripped) = relative.strip_prefix("assets/minecraft/textures/") {
        aliases.push(stripped.to_string());
    }
    if let Some(stripped) = relative.strip_prefix("assets/minecraft/sounds/") {
        aliases.push(format!("sounds/{stripped}"));
    }
    if let Some(stripped) = relative.strip_prefix("assets/minecraft/lang/") {
        aliases.push(format!("lang/{stripped}"));
    }
    if let Some(stripped) = relative.strip_prefix("assets/lang/") {
        aliases.push(format!("lang/{stripped}"));
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn lookup_asset<'a>(pack: &'a LoadedPack, relative: &str) -> Option<&'a [u8]> {
    if let Some(bytes) = pack.assets.get(relative) {
        return Some(bytes);
    }
    let candidates = [
        format!("textures/{relative}"),
        format!("sounds/{relative}"),
        format!("lang/{relative}"),
        format!("assets/minecraft/{relative}"),
        format!("assets/minecraft/textures/{relative}"),
        format!("assets/minecraft/sounds/{relative}"),
        format!("assets/minecraft/lang/{relative}"),
        format!("vanilla/textures/{relative}"),
        format!("assets/vanilla/textures/{relative}"),
    ];
    candidates
        .iter()
        .find_map(|candidate| pack.assets.get(candidate).map(Vec::as_slice))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("icraft_{label}_{stamp}_{id}"));
        fs::create_dir_all(&path).expect("temporary directory should be created");
        path
    }

    fn manifest(id: &str, dependencies: &[&str]) -> String {
        serde_json::json!({
            "id": id,
            "name": id,
            "version": "1.0.0",
            "format": 1,
            "description": "test",
            "dependencies": dependencies,
        })
        .to_string()
    }

    fn write_pack(root: &Path, id: &str, dependencies: &[&str]) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("pack.json"), manifest(id, dependencies)).unwrap();
    }

    fn single_entry_zip(name: &[u8], payload: &[u8], central_crc: u32) -> Vec<u8> {
        let mut archive = Vec::new();
        archive.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(name);
        archive.extend_from_slice(payload);
        let central_offset = archive.len() as u32;
        archive.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        archive.extend_from_slice(&[20, 0, 20, 0]);
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&central_crc.to_le_bytes());
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&[0; 2]);
        archive.extend_from_slice(&[0; 2]);
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.extend_from_slice(name);
        let central_size = archive.len() as u32 - central_offset;
        archive.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&central_size.to_le_bytes());
        archive.extend_from_slice(&central_offset.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive
    }

    #[test]
    fn manifest_and_dependency_order_are_deterministic() {
        let root = temp_dir("packs");
        fs::write(root.join("pack.json"), manifest(BUILTIN_PACK_ID, &[])).unwrap();
        fs::write(root.join("stone.txt"), b"builtin").unwrap();
        let user = root.join("resourcepacks");
        fs::create_dir_all(user.join("base")).unwrap();
        fs::write(user.join("base/pack.json"), manifest("test.base", &[])).unwrap();
        fs::create_dir_all(user.join("theme")).unwrap();
        fs::write(
            user.join("theme/pack.json"),
            manifest("test.theme", &["test.base"]),
        )
        .unwrap();
        fs::create_dir_all(user.join("theme/lang")).unwrap();
        fs::write(user.join("theme/lang/de_de.json"), b"{}").unwrap();
        let manager = ResourcePackManager::discover(&root, &user);
        assert_eq!(
            manager
                .enabled_order()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["test.base", "test.theme"]
        );
        let mut manager = manager;
        assert_eq!(manager.read_asset("lang/de_de.json"), Some(b"{}".to_vec()));
        manager
            .apply_enabled_order(["test.theme", "test.base"])
            .unwrap();
        assert_eq!(manager.enabled_order(), ["test.base", "test.theme"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn typed_consumers_skip_invalid_override_and_deduplicate_diagnostics() {
        let root = temp_dir("typed_consumers");
        write_pack(&root, BUILTIN_PACK_ID, &[]);
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("sounds")).unwrap();
        fs::create_dir_all(root.join("lang")).unwrap();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::create_dir_all(root.join("font")).unwrap();
        let texture = include_bytes!("../assets/vanilla/textures/block/stone.png");
        let sound = include_bytes!("../assets/sounds/click.wav");
        fs::write(root.join("textures/stone.png"), texture).unwrap();
        fs::write(root.join("sounds/click.wav"), sound).unwrap();
        fs::write(root.join("lang/en_us.json"), br#"{"hello":"Hello"}"#).unwrap();
        fs::write(root.join("models/block.json"), br#"{"parent":"builtin"}"#).unwrap();
        fs::write(root.join("font/main.ttf"), b"OTTOfont").unwrap();

        let user = root.join("resourcepacks");
        let override_pack = user.join("override");
        write_pack(&override_pack, "test.override", &[]);
        fs::create_dir_all(override_pack.join("textures")).unwrap();
        fs::create_dir_all(override_pack.join("sounds")).unwrap();
        fs::create_dir_all(override_pack.join("lang")).unwrap();
        fs::create_dir_all(override_pack.join("models")).unwrap();
        fs::create_dir_all(override_pack.join("font")).unwrap();
        fs::write(override_pack.join("textures/stone.png"), b"not png").unwrap();
        fs::write(override_pack.join("sounds/click.wav"), b"not sound").unwrap();
        fs::write(override_pack.join("lang/en_us.json"), [0xff, 0xfe]).unwrap();
        fs::write(override_pack.join("models/block.json"), b"[]").unwrap();
        fs::write(override_pack.join("font/main.ttf"), b"not font").unwrap();

        let mut manager = ResourcePackManager::discover(&root, &user);
        manager.apply_enabled_order(["test.override"]).unwrap();
        assert_eq!(
            manager.resolve_texture("textures/stone.png"),
            Some(texture.to_vec())
        );
        assert_eq!(
            manager.resolve_sound("sounds/click.wav"),
            Some(sound.to_vec())
        );
        assert_eq!(
            manager.resolve_locale("en_us"),
            Some(br#"{"hello":"Hello"}"#.to_vec())
        );
        assert_eq!(
            manager.resolve_model("models/block.json"),
            Some(br#"{"parent":"builtin"}"#.to_vec())
        );
        assert_eq!(
            manager.resolve_font("font/main.ttf"),
            Some(b"OTTOfont".to_vec())
        );
        let diagnostics = manager.diagnostics().len();
        assert!(diagnostics >= 5);
        manager.resolve_texture("textures/stone.png");
        manager.resolve_sound("sounds/click.wav");
        manager.resolve_locale("en_us");
        manager.resolve_model("models/block.json");
        manager.resolve_font("font/main.ttf");
        assert_eq!(manager.diagnostics().len(), diagnostics);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_validates_description_and_dependency_ids() {
        let mut value = ResourcePackManifest {
            id: "test.pack".into(),
            name: "Test".into(),
            version: "1".into(),
            format: 1,
            description: "x".into(),
            dependencies: vec!["bad id".into()],
        };
        assert!(matches!(
            value.validate(),
            Err(PackError::InvalidManifest(message)) if message.contains("dependency id")
        ));
        value.dependencies.clear();
        value.description = "x".repeat(MAX_PACK_DESCRIPTION_BYTES + 1);
        assert!(
            matches!(value.validate(), Err(PackError::InvalidManifest(message)) if message.contains("description"))
        );
    }

    #[test]
    fn missing_asset_records_one_diagnostic() {
        let root = temp_dir("missing");
        fs::write(root.join("pack.json"), manifest(BUILTIN_PACK_ID, &[])).unwrap();
        let mut manager = ResourcePackManager::discover(&root, root.join("none"));
        assert!(manager.resolve_asset("block/not_present.png").is_none());
        assert!(manager.resolve_asset("block/not_present.png").is_none());
        assert_eq!(manager.diagnostics().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn zip_path_traversal_is_rejected_before_extraction() {
        let root = temp_dir("zip_traversal");
        let archive_path = root.join("bad.zip");
        let name = b"../escape.txt";
        let payload = b"escape";
        let mut archive = Vec::new();
        archive.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes()); // version needed
        archive.extend_from_slice(&0u16.to_le_bytes()); // flags
        archive.extend_from_slice(&0u16.to_le_bytes()); // stored
        archive.extend_from_slice(&[0; 4]); // time/date
        archive.extend_from_slice(&[0; 4]); // crc
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(name);
        archive.extend_from_slice(payload);
        let central_offset = archive.len() as u32;
        archive.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        archive.extend_from_slice(&[20, 0, 20, 0]); // versions
        archive.extend_from_slice(&0u16.to_le_bytes()); // flags
        archive.extend_from_slice(&0u16.to_le_bytes()); // stored
        archive.extend_from_slice(&[0; 4]); // time/date
        archive.extend_from_slice(&[0; 4]); // crc
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&[0; 2]); // extra
        archive.extend_from_slice(&[0; 2]); // comment
        archive.extend_from_slice(&[0; 4]); // disk/internal attrs
        archive.extend_from_slice(&[0; 4]); // external attrs
        archive.extend_from_slice(&0u32.to_le_bytes()); // local offset
        archive.extend_from_slice(name);
        let central_size = archive.len() as u32 - central_offset;
        archive.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        archive.extend_from_slice(&[0; 4]); // disk numbers
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&central_size.to_le_bytes());
        archive.extend_from_slice(&central_offset.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes()); // comment length
        fs::write(&archive_path, archive).unwrap();
        assert!(matches!(
            load_zip_pack(&archive_path),
            Err(PackError::UnsafePath(_))
        ));
        assert!(!root.parent().unwrap().join("escape.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn zip_crc_mismatch_and_zip64_markers_are_rejected() {
        let root = temp_dir("zip_integrity");
        let archive_path = root.join("bad_crc.zip");
        let payload = b"payload";
        let archive = single_entry_zip(b"asset.txt", payload, 0);
        fs::write(&archive_path, archive).unwrap();
        assert!(matches!(
            load_zip_pack(&archive_path),
            Err(PackError::Archive(message)) if message.contains("CRC mismatch")
        ));

        let mut zip64 = single_entry_zip(b"asset.txt", payload, crc32(payload));
        let eocd = zip64.len() - 22;
        zip64[eocd + 10..eocd + 12].copy_from_slice(&u16::MAX.to_le_bytes());
        fs::write(root.join("zip64.zip"), zip64).unwrap();
        assert!(matches!(
            load_zip_pack(&root.join("zip64.zip")),
            Err(PackError::Archive(message)) if message.contains("ZIP64")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dependency_cycles_are_rejected() {
        let root = temp_dir("cycle");
        fs::write(root.join("pack.json"), manifest(BUILTIN_PACK_ID, &[])).unwrap();
        let user = root.join("resourcepacks");
        fs::create_dir_all(user.join("one")).unwrap();
        fs::write(user.join("one/pack.json"), manifest("one", &["two"])).unwrap();
        fs::create_dir_all(user.join("two")).unwrap();
        fs::write(user.join("two/pack.json"), manifest("two", &["one"])).unwrap();
        assert!(matches!(
            ResourcePackManager::discover(&root, &user).reload(),
            Err(PackError::DependencyCycle(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn logical_paths_reject_absolute_and_parent_components() {
        assert!(normalize_logical_path("/tmp/file").is_err());
        assert!(normalize_logical_path("assets/../escape").is_err());
        assert_eq!(
            normalize_logical_path("assets\\minecraft\\lang\\en_us.json").unwrap(),
            "assets/minecraft/lang/en_us.json"
        );
    }
}
