use crate::dimension::Dimension;
use crate::network::protocol::PlayerEffectWire;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::format::{
    deserialize_player_data, serialize_player_data, DedicatedPlayerFile, IdentityError,
    LegacyDedicatedPlayerFile, LegacyLevelData, LevelData, PlayerData,
    DEDICATED_PLAYER_SAVE_VERSION, PLAYER_IDENTITY_MAX_LEN,
};
use super::region::atomic_write;

/// Single identity key for handshake, login uniqueness, whitelist, operators,
/// and `players/<id>.dat`.
///
/// Raw names are accepted only when lowercasing them yields a 1..=16 character
/// `[a-z0-9_-]` string that is not a Windows reserved stem. Mutating sanitizers
/// such as rewriting `foo.bar` to `foo_bar` are rejected so two logins cannot
/// share one player file.
pub fn normalize_player_identity(raw: &str) -> Result<String, IdentityError> {
    if raw.is_empty() {
        return Err(IdentityError::Empty);
    }
    if raw.len() > PLAYER_IDENTITY_MAX_LEN {
        return Err(IdentityError::TooLong);
    }
    if !raw.is_ascii() {
        return Err(IdentityError::InvalidCharset);
    }
    let lowered = raw.to_ascii_lowercase();
    if is_windows_reserved_stem(&lowered) {
        return Err(IdentityError::ReservedStem);
    }
    if !lowered
        .bytes()
        .all(|ch| matches!(ch, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
    {
        return Err(IdentityError::InvalidCharset);
    }
    Ok(lowered)
}

pub(crate) fn is_windows_reserved_stem(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    matches!(
        stem,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

/// Return the path for a dedicated player identity.
/// Handshake, whitelist, operators, and this file all use the same
/// normalized key; mutating names such as `foo.bar` are rejected instead
/// of being rewritten onto `foo_bar.dat`.
pub fn dedicated_player_file_path(world_dir: &Path, username: &str) -> Result<PathBuf, IdentityError> {
    let identity = normalize_player_identity(username)?;
    Ok(world_dir
        .join("players")
        .join(format!("{identity}.dat")))
}

pub(crate) fn dedicated_player_file_path_io(world_dir: &Path, username: &str) -> io::Result<PathBuf> {
    dedicated_player_file_path(world_dir, username)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// Atomically save a dedicated player payload. A failed replacement does
/// not touch the previous file, so the caller can retry with the same
/// snapshot after reporting the error to the session manager.
pub fn save_dedicated_player(
    world_dir: &Path,
    username: &str,
    current_dimension: Dimension,
    data: &PlayerData,
    effects: &[PlayerEffectWire],
) -> io::Result<()> {
    let file = DedicatedPlayerFile {
        version: DEDICATED_PLAYER_SAVE_VERSION,
        current_dimension,
        data: data.clone(),
        effects: effects.to_vec(),
    };
    let bytes = bincode::serialize(&file).map_err(|error| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("player save encode failed: {error}"),
        )
    })?;
    atomic_write(dedicated_player_file_path_io(world_dir, username)?, &bytes)
}

/// Load a dedicated player payload, migrating the version-1 runtime file
/// (which did not store current dimension) to the explicit Overworld or
/// saved spawn dimension default.
pub fn load_dedicated_player(world_dir: &Path, username: &str) -> io::Result<Option<DedicatedPlayerFile>> {
    let path = dedicated_player_file_path_io(world_dir, username)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path)?;
    if let Ok(file) = bincode::deserialize::<DedicatedPlayerFile>(&bytes) {
        if file.version != DEDICATED_PLAYER_SAVE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported dedicated player save version {}", file.version),
            ));
        }
        return Ok(Some(file));
    }

    let legacy: LegacyDedicatedPlayerFile = bincode::deserialize(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "dedicated player save decode failed for {}: {error}",
                path.display()
            ),
        )
    })?;
    if legacy.version != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported dedicated player save version {}",
                legacy.version
            ),
        ));
    }
    Ok(Some(DedicatedPlayerFile {
        version: DEDICATED_PLAYER_SAVE_VERSION,
        current_dimension: legacy.data.spawn_dimension.unwrap_or_default(),
        data: legacy.data,
        effects: legacy.effects,
    }))
}

pub fn save_player_and_level(
    world_dir: &Path,
    level: &LevelData,
    player: &PlayerData,
) -> io::Result<()> {
    let level_file = world_dir.join("level.dat");
    let player_file = world_dir.join("player.dat");

    let serialized_level =
        bincode::serialize(level).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let serialized_player =
        serialize_player_data(player).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    atomic_write(&level_file, &serialized_level)?;
    atomic_write(&player_file, &serialized_player)?;

    Ok(())
}

pub fn load_player_and_level(world_dir: &Path) -> io::Result<(LevelData, PlayerData)> {
    let level_file = world_dir.join("level.dat");
    let player_file = world_dir.join("player.dat");

    let mut lf = File::open(&level_file)?;
    let mut level_bytes = Vec::new();
    lf.read_to_end(&mut level_bytes)?;
    let level = match bincode::deserialize::<LevelData>(&level_bytes) {
        Ok(level) => level,
        Err(current_error) => bincode::deserialize::<LegacyLevelData>(&level_bytes)
            .map(LevelData::from)
            .map_err(|legacy_error| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "level decode failed (current: {current_error}; legacy: {legacy_error})"
                    ),
                )
            })?,
    };

    let mut pf = File::open(&player_file)?;
    let mut player_bytes = Vec::new();
    pf.read_to_end(&mut player_bytes)?;
    let player = deserialize_player_data(&player_bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;

    Ok((level, player))
}
