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
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const MAX_PACK_ENTRIES: usize = 4096;
pub const MAX_PACK_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_PACK_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_COMPRESSION_RATIO: u64 = 1_000;
const BUILTIN_PACK_ID: &str = "icraft.builtin";

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
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ".-_".contains(ch))
        {
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
        let mut seen = HashSet::new();
        for dependency in &self.dependencies {
            if dependency == &self.id || !seen.insert(dependency) {
                return Err(PackError::InvalidManifest(
                    "dependencies must be unique and cannot include the pack itself".into(),
                ));
            }
        }
        Ok(())
    }
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
        Self::discover("assets", "resourcepacks")
    }

    pub fn discover(builtin_root: impl Into<PathBuf>, user_root: impl Into<PathBuf>) -> Self {
        let mut manager = Self {
            builtin_root: builtin_root.into(),
            user_root: user_root.into(),
            packs: Vec::new(),
            enabled_order: Vec::new(),
            diagnostics: Vec::new(),
            diagnostic_keys: HashSet::new(),
        };
        let _ = manager.reload();
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

        let mut candidates = fs::read_dir(&self.user_root)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        candidates.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
        for path in candidates {
            let loaded = if path.is_dir() {
                load_directory_pack(&path)
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
            {
                load_zip_pack(&path)
            } else {
                continue;
            };
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
        self.enabled_order = requested;
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
        self.read_asset(&format!("lang/{}.json", language.to_ascii_lowercase()))
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

fn load_directory_pack(root: &Path) -> Result<LoadedPack, PackError> {
    let manifest_path = [root.join("pack.json"), root.join("assets/pack.json")]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| PackError::InvalidManifest("pack.json is missing".into()))?;
    let manifest = parse_manifest(&fs::read(&manifest_path)?)?;
    let mut assets = HashMap::new();
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
            insert_asset_aliases(&mut assets, &relative, bytes);
        }
    }
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
    let entry_count = read_u16(&bytes, eocd + 10)
        .ok_or_else(|| PackError::Archive("truncated ZIP end record".into()))?
        as usize;
    let central_size = read_u32(&bytes, eocd + 12)
        .ok_or_else(|| PackError::Archive("truncated ZIP end record".into()))?
        as usize;
    let central_offset = read_u32(&bytes, eocd + 16)
        .ok_or_else(|| PackError::Archive("truncated ZIP end record".into()))?
        as usize;
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
        if compressed_size > 0 && uncompressed_size / compressed_size.max(1) > MAX_COMPRESSION_RATIO
        {
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
        if relative == "pack.json" || relative == "assets/pack.json" {
            manifest_bytes = Some(data.clone());
        }
        insert_asset_aliases(&mut assets, &relative, data);
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
        assert_eq!(manager.read_asset("lang/de_de.json"), Some(b"{}".to_vec()));
        let _ = fs::remove_dir_all(root);
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
