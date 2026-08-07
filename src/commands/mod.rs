//! Small, typed command surface used by the in-game chat dispatcher.
//!
//! This deliberately is not a Brigadier clone: parsing is deterministic,
//! bounded, and produces typed arguments before any world mutation is
//! attempted.  The executor in `State` remains the authority gate.

use crate::inventory::{GameMode, Item};
use crate::menu::Difficulty;

pub const MAX_COMMAND_BYTES: usize = 256;
pub const MAX_COMMAND_ARGS: usize = 16;
pub const MAX_SELECTOR_BYTES: usize = 32;
pub const MAX_GIVE_COUNT: u32 = 64;
pub const MAX_COORDINATE: i32 = 30_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub position: usize,
    pub message: String,
}

impl CommandError {
    fn new(position: usize, message: impl Into<String>) -> Self {
        Self {
            position,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandTarget {
    SelfPlayer,
    NearestPlayer,
    AllPlayers,
    Name(String),
}

impl CommandTarget {
    fn parse(token: &str, position: usize) -> Result<Self, CommandError> {
        if token.len() > MAX_SELECTOR_BYTES || token.is_empty() {
            return Err(CommandError::new(position, "selector is too long or empty"));
        }
        Ok(match token {
            "@s" => Self::SelfPlayer,
            "@p" => Self::NearestPlayer,
            "@a" => Self::AllPlayers,
            name if name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) =>
            {
                Self::Name(name.to_ascii_uppercase())
            }
            _ => return Err(CommandError::new(position, "invalid selector")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeCommand {
    Set(u64),
    Add(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeatherCommand {
    Clear(Option<u32>),
    Rain(Option<u32>),
    Thunder(Option<u32>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help(Option<String>),
    GameMode {
        mode: GameMode,
        target: Option<CommandTarget>,
    },
    Difficulty(Difficulty),
    GameRule {
        rule: String,
        value: Option<String>,
    },
    Time(TimeCommand),
    Weather(WeatherCommand),
    Teleport {
        target: CommandTarget,
        position: [i32; 3],
    },
    Give {
        target: CommandTarget,
        item: Item,
        count: u32,
    },
    Kill(Option<CommandTarget>),
    SpawnPoint {
        target: CommandTarget,
        position: Option<[i32; 3]>,
    },
    SetWorldSpawn(Option<[i32; 3]>),
    Locate(String),
    Seed,
    SaveAll,
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Help(_) => "help",
            Self::GameMode { .. } => "gamemode",
            Self::Difficulty(_) => "difficulty",
            Self::GameRule { .. } => "gamerule",
            Self::Time(_) => "time",
            Self::Weather(_) => "weather",
            Self::Teleport { .. } => "tp",
            Self::Give { .. } => "give",
            Self::Kill(_) => "kill",
            Self::SpawnPoint { .. } => "spawnpoint",
            Self::SetWorldSpawn(_) => "setworldspawn",
            Self::Locate(_) => "locate",
            Self::Seed => "seed",
            Self::SaveAll => "save-all",
        }
    }
}

fn parse_mode(token: &str, pos: usize) -> Result<GameMode, CommandError> {
    match token.to_ascii_lowercase().as_str() {
        "survival" | "s" | "0" => Ok(GameMode::Survival),
        "creative" | "c" | "1" => Ok(GameMode::Creative),
        "adventure" | "a" | "2" => Ok(GameMode::Adventure),
        "spectator" | "sp" | "3" => Ok(GameMode::Spectator),
        _ => Err(CommandError::new(
            pos,
            "expected survival, creative, adventure, or spectator",
        )),
    }
}

fn parse_difficulty(token: &str, pos: usize) -> Result<Difficulty, CommandError> {
    match token.to_ascii_lowercase().as_str() {
        "peaceful" | "0" => Ok(Difficulty::Peaceful),
        "easy" | "1" => Ok(Difficulty::Easy),
        "normal" | "2" => Ok(Difficulty::Normal),
        "hard" | "3" => Ok(Difficulty::Hard),
        _ => Err(CommandError::new(pos, "unknown difficulty")),
    }
}

fn parse_coordinate(token: &str, pos: usize) -> Result<i32, CommandError> {
    let value = token
        .parse::<i32>()
        .map_err(|_| CommandError::new(pos, "coordinate must be an integer"))?;
    if value.abs() > MAX_COORDINATE {
        return Err(CommandError::new(
            pos,
            "coordinate is outside the world border",
        ));
    }
    Ok(value)
}

fn parse_position(tokens: &[&str], offset: usize) -> Result<[i32; 3], CommandError> {
    if tokens.len() != offset + 3 {
        return Err(CommandError::new(
            offset,
            "expected exactly three coordinates",
        ));
    }
    Ok([
        parse_coordinate(tokens[offset], offset)?,
        parse_coordinate(tokens[offset + 1], offset + 1)?,
        parse_coordinate(tokens[offset + 2], offset + 2)?,
    ])
}

fn parse_item(token: &str, pos: usize) -> Result<Item, CommandError> {
    let normalized = token.trim().to_ascii_lowercase().replace([':', '-'], "_");
    let item = crate::inventory::ALL_ITEMS
        .iter()
        .copied()
        .find(|item| {
            item.properties()
                .name
                .to_ascii_lowercase()
                .replace([' ', '-'], "_")
                == normalized
                || format!("{item:?}").to_ascii_lowercase() == normalized
        })
        .ok_or_else(|| CommandError::new(pos, "unknown item id"))?;
    if item == Item::Air {
        return Err(CommandError::new(pos, "air cannot be given"));
    }
    Ok(item)
}

fn parse_duration(token: Option<&str>, pos: usize) -> Result<Option<u32>, CommandError> {
    let Some(token) = token else { return Ok(None) };
    let duration = token
        .parse::<u32>()
        .map_err(|_| CommandError::new(pos, "duration must be an integer"))?;
    Ok(Some(duration.min(86_400)))
}

/// Parse a user-entered command.  The leading slash is optional so tests and
/// server integrations can use the same parser as chat.
pub fn parse(input: &str) -> Result<Command, CommandError> {
    let input = input.trim();
    if input.len() > MAX_COMMAND_BYTES {
        return Err(CommandError::new(MAX_COMMAND_BYTES, "command is too long"));
    }
    let input = input.strip_prefix('/').unwrap_or(input);
    let tokens: Vec<&str> = input.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(CommandError::new(0, "empty command"));
    }
    if tokens.len() > MAX_COMMAND_ARGS {
        return Err(CommandError::new(MAX_COMMAND_ARGS, "too many arguments"));
    }
    let cmd = tokens[0].to_ascii_lowercase();
    let args = &tokens[1..];
    match cmd.as_str() {
        "help" => Ok(Command::Help(
            args.first().map(|s| (*s).to_ascii_lowercase()),
        )),
        "gamemode" => {
            if !(1..=2).contains(&args.len()) {
                return Err(CommandError::new(1, "usage: gamemode <mode> [target]"));
            }
            Ok(Command::GameMode {
                mode: parse_mode(args[0], 1)?,
                target: args
                    .get(1)
                    .map(|target| CommandTarget::parse(target, 2))
                    .transpose()?,
            })
        }
        "difficulty" => {
            if args.len() != 1 {
                return Err(CommandError::new(1, "usage: difficulty <level>"));
            }
            Ok(Command::Difficulty(parse_difficulty(args[0], 1)?))
        }
        "gamerule" => {
            if !(1..=2).contains(&args.len()) {
                return Err(CommandError::new(1, "usage: gamerule <rule> [value]"));
            }
            let rule = args[0].to_ascii_lowercase();
            if rule.len() > 32 {
                return Err(CommandError::new(1, "rule name is too long"));
            }
            Ok(Command::GameRule {
                rule,
                value: args.get(1).map(|v| (*v).to_ascii_lowercase()),
            })
        }
        "time" => {
            if args.len() != 2 {
                return Err(CommandError::new(
                    1,
                    "usage: time <set|add> <ticks|day|night>",
                ));
            }
            let amount = match args[1].to_ascii_lowercase().as_str() {
                "day" => 1_000,
                "night" => 13_000,
                value => value
                    .parse::<u64>()
                    .map_err(|_| CommandError::new(2, "time must be a non-negative integer"))?,
            };
            if amount > 3_000_000_000 {
                return Err(CommandError::new(2, "time value is too large"));
            }
            match args[0].to_ascii_lowercase().as_str() {
                "set" => Ok(Command::Time(TimeCommand::Set(amount))),
                "add" => Ok(Command::Time(TimeCommand::Add(amount))),
                _ => Err(CommandError::new(1, "expected set or add")),
            }
        }
        "weather" => {
            if !(1..=2).contains(&args.len()) {
                return Err(CommandError::new(
                    1,
                    "usage: weather <clear|rain|thunder> [duration]",
                ));
            }
            let duration = parse_duration(args.get(1).copied(), 2)?;
            match args[0].to_ascii_lowercase().as_str() {
                "clear" => Ok(Command::Weather(WeatherCommand::Clear(duration))),
                "rain" => Ok(Command::Weather(WeatherCommand::Rain(duration))),
                "thunder" => Ok(Command::Weather(WeatherCommand::Thunder(duration))),
                _ => Err(CommandError::new(1, "unknown weather type")),
            }
        }
        "tp" | "teleport" => {
            if args.len() == 3 {
                Ok(Command::Teleport {
                    target: CommandTarget::SelfPlayer,
                    position: parse_position(args, 0)?,
                })
            } else if args.len() == 4 {
                Ok(Command::Teleport {
                    target: CommandTarget::parse(args[0], 1)?,
                    position: parse_position(args, 1)?,
                })
            } else {
                Err(CommandError::new(1, "usage: tp [target] <x> <y> <z>"))
            }
        }
        "give" => {
            if !(2..=3).contains(&args.len()) {
                return Err(CommandError::new(1, "usage: give <target> <item> [count]"));
            }
            let count = args
                .get(2)
                .map(|value| {
                    value
                        .parse::<u32>()
                        .map_err(|_| CommandError::new(3, "count must be an integer"))
                        .and_then(|count| {
                            if count == 0 || count > MAX_GIVE_COUNT {
                                Err(CommandError::new(3, "count must be between 1 and 64"))
                            } else {
                                Ok(count)
                            }
                        })
                })
                .transpose()?
                .unwrap_or(1);
            Ok(Command::Give {
                target: CommandTarget::parse(args[0], 1)?,
                item: parse_item(args[1], 2)?,
                count,
            })
        }
        "kill" => Ok(Command::Kill(
            args.first()
                .map(|target| CommandTarget::parse(target, 1))
                .transpose()?,
        )),
        "spawnpoint" => {
            if args.is_empty() {
                Ok(Command::SpawnPoint {
                    target: CommandTarget::SelfPlayer,
                    position: None,
                })
            } else if args.len() == 1 {
                Ok(Command::SpawnPoint {
                    target: CommandTarget::parse(args[0], 1)?,
                    position: None,
                })
            } else if args.len() == 4 {
                Ok(Command::SpawnPoint {
                    target: CommandTarget::parse(args[0], 1)?,
                    position: Some(parse_position(args, 1)?),
                })
            } else {
                Err(CommandError::new(1, "usage: spawnpoint [target] [x y z]"))
            }
        }
        "setworldspawn" => Ok(Command::SetWorldSpawn(if args.is_empty() {
            None
        } else {
            Some(parse_position(args, 0)?)
        })),
        "locate" => {
            if args.len() != 1 || args[0].len() > 64 {
                return Err(CommandError::new(1, "usage: locate <structure>"));
            }
            Ok(Command::Locate(args[0].to_ascii_lowercase()))
        }
        "seed" => {
            if !args.is_empty() {
                return Err(CommandError::new(1, "usage: seed"));
            }
            Ok(Command::Seed)
        }
        "save-all" | "saveall" => {
            if !args.is_empty() {
                return Err(CommandError::new(1, "usage: save-all"));
            }
            Ok(Command::SaveAll)
        }
        _ => Err(CommandError::new(0, format!("unknown command `{cmd}`"))),
    }
}

/// Kept public for UI help and for a future dedicated server shell.
pub fn help_text(command: Option<&str>) -> &'static str {
    match command.map(str::to_ascii_lowercase).as_deref() {
        Some("gamemode") => "/gamemode <survival|creative|adventure|spectator> [target]",
        Some("difficulty") => "/difficulty <peaceful|easy|normal|hard>",
        Some("gamerule") => "/gamerule <rule> [true|false|value]",
        Some("time") => "/time <set|add> <ticks|day|night>",
        Some("weather") => "/weather <clear|rain|thunder> [duration]",
        Some("tp") | Some("teleport") => "/tp [target] <x> <y> <z>",
        Some("give") => "/give <target> <item> [count]",
        Some("kill") => "/kill [target]",
        Some("spawnpoint") => "/spawnpoint [target] [x y z]",
        Some("setworldspawn") => "/setworldspawn [x y z]",
        Some("locate") => "/locate <structure>",
        Some("seed") => "/seed",
        Some("save-all") | Some("saveall") => "/save-all",
        _ => "/help, /gamemode, /difficulty, /gamerule, /time, /weather, /tp, /give, /kill, /spawnpoint, /setworldspawn, /locate, /seed, /save-all",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_covers_typed_commands() {
        assert_eq!(parse("/gamemode adventure @s").unwrap().name(), "gamemode");
        assert_eq!(
            parse("time set night").unwrap(),
            Command::Time(TimeCommand::Set(13_000))
        );
        assert_eq!(
            parse("weather thunder 999999").unwrap(),
            Command::Weather(WeatherCommand::Thunder(Some(86_400)))
        );
        assert!(matches!(
            parse("give @s diamond 64").unwrap(),
            Command::Give { count: 64, .. }
        ));
    }

    #[test]
    fn parser_rejects_bad_numbers_without_panicking() {
        assert!(parse("tp 1 NaN 3").is_err());
        assert!(parse("give @s stone 0").is_err());
        assert!(parse(&format!("help {}", "x".repeat(MAX_COMMAND_BYTES))).is_err());
    }

    #[test]
    fn world_type_is_part_of_the_shared_creation_contract() {
        assert_eq!(
            crate::game_rules::WorldType::parse("superflat"),
            crate::game_rules::WorldType::Superflat
        );
        assert_eq!(crate::game_rules::WorldType::Default.as_str(), "DEFAULT");
    }
}
