use super::*;

/// The supported `server.properties` surface.  Unknown keys are ignored for
/// forward compatibility; known keys are parsed strictly and validated before
/// a world directory is created or opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerProperties {
    pub bind: String,
    pub port: u16,
    pub motd: String,
    pub max_players: usize,
    pub difficulty: String,
    /// LAN/offline account switch. `true` is rejected until challenge /
    /// shared-secret auth exists; names are accounts and operators come only
    /// from the dedicated-server console `op` command (or this file).
    pub online_mode: bool,
    pub whitelist: HashSet<String>,
    pub operators: HashSet<String>,
    pub view_distance: u8,
    pub simulation_distance: u8,
    pub pvp: bool,
    pub world_dir: PathBuf,
    pub seed: u64,
}

impl Default for ServerProperties {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".into(),
            port: 25565,
            motd: "iCraft server".into(),
            max_players: 20,
            difficulty: "normal".into(),
            online_mode: false,
            whitelist: HashSet::new(),
            operators: HashSet::new(),
            view_distance: 10,
            simulation_distance: 8,
            pvp: true,
            world_dir: PathBuf::from("world"),
            seed: 0,
        }
    }
}

impl ServerProperties {
    pub fn difficulty_kind(&self) -> Result<Difficulty, ServerConfigError> {
        Difficulty::parse_strict(&self.difficulty).ok_or_else(|| {
            invalid(
                "difficulty",
                &self.difficulty,
                "expected peaceful, easy, normal, or hard",
            )
        })
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ServerConfigError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        let mut properties = Self::default();
        for (line_number, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((raw_key, raw_value)) = line.split_once('=') else {
                return Err(ServerConfigError::Invalid {
                    key: format!("line {}", line_number + 1),
                    value: line.into(),
                    reason: "expected key=value".into(),
                });
            };
            let key = raw_key.trim();
            let value = raw_value.trim();
            match key {
                "bind" | "server-ip" => properties.bind = value.to_string(),
                "port" | "server-port" => {
                    properties.port = parse_range(key, value, 1..=u16::MAX)?;
                }
                "motd" => properties.motd = value.to_string(),
                "max-players" => {
                    properties.max_players = parse_range(key, value, 1..=64)? as usize;
                }
                "difficulty" => {
                    let normalized = value.to_ascii_lowercase();
                    if !matches!(normalized.as_str(), "peaceful" | "easy" | "normal" | "hard") {
                        return Err(invalid(
                            key,
                            value,
                            "expected peaceful, easy, normal, or hard",
                        ));
                    }
                    properties.difficulty = normalized;
                }
                "online-mode" => properties.online_mode = parse_bool(key, value)?,
                "whitelist" => {
                    properties.whitelist = parse_identity_set(key, value)?;
                }
                "operators" | "ops" => {
                    properties.operators = parse_identity_set(key, value)?;
                }
                "view-distance" => {
                    properties.view_distance = parse_range(key, value, 2..=32)?;
                }
                "simulation-distance" => {
                    properties.simulation_distance = parse_range(key, value, 2..=32)?;
                }
                "pvp" => properties.pvp = parse_bool(key, value)?,
                "level-name" | "world" | "world-dir" => properties.world_dir = PathBuf::from(value),
                "level-seed" | "seed" => {
                    properties.seed = value
                        .parse::<i64>()
                        .map_err(|_| invalid(key, value, "expected a signed 64-bit integer"))?
                        as u64;
                }
                _ => {}
            }
        }
        properties.validate()?;
        Ok(properties)
    }

    pub fn validate(&self) -> Result<(), ServerConfigError> {
        self.difficulty_kind()?;
        if self.bind.trim().is_empty() {
            return Err(invalid("bind", &self.bind, "must not be empty"));
        }
        if self.bind.parse::<IpAddr>().is_err() && self.bind != "localhost" {
            return Err(invalid(
                "bind",
                &self.bind,
                "expected an IP address or localhost",
            ));
        }
        if self.port == 0 {
            return Err(invalid("port", self.port, "must be between 1 and 65535"));
        }
        if !(1..=64).contains(&self.max_players) {
            return Err(invalid(
                "max-players",
                self.max_players,
                "must be between 1 and 64",
            ));
        }
        if !(2..=32).contains(&self.view_distance) {
            return Err(invalid(
                "view-distance",
                self.view_distance,
                "must be between 2 and 32",
            ));
        }
        if !(2..=32).contains(&self.simulation_distance) {
            return Err(invalid(
                "simulation-distance",
                self.simulation_distance,
                "must be between 2 and 32",
            ));
        }
        if self.online_mode {
            return Err(invalid(
                "online-mode",
                true,
                "online authentication is not implemented; refuse to treat this as a credential switch (尚未實作驗證，拒絕當憑證開關)",
            ));
        }
        validate_identity_set("whitelist", &self.whitelist)?;
        validate_identity_set("operators", &self.operators)?;
        if self.motd.trim().is_empty() || self.motd.len() > 256 {
            return Err(invalid("motd", &self.motd, "must contain 1..=256 bytes"));
        }
        Ok(())
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), ServerConfigError> {
        self.validate()?;
        let mut whitelist: Vec<_> = self.whitelist.iter().cloned().collect();
        whitelist.sort();
        let content = format!(
            "# online-mode=false is LAN/offline: names are accounts until real credentials exist.\n\
             # Operators are granted only by the dedicated-server console `op` command (or this file),\n\
             # bound to the normalized identity of later connections. There is still no password.\n\
             bind={}\nport={}\nmotd={}\nmax-players={}\ndifficulty={}\nonline-mode={}\nwhitelist={}\noperators={}\nview-distance={}\nsimulation-distance={}\npvp={}\nlevel-name={}\nlevel-seed={}\n",
            self.bind,
            self.port,
            self.motd,
            self.max_players,
            self.difficulty,
            self.online_mode,
            whitelist.join(","),
            sorted_names(&self.operators).join(","),
            self.view_distance,
            self.simulation_distance,
            self.pvp,
            self.world_dir.display(),
            self.seed as i64,
        );
        atomic_write(path.as_ref(), content.as_bytes())?;
        Ok(())
    }
}

fn invalid(
    key: impl Into<String>,
    value: impl ToString,
    reason: impl Into<String>,
) -> ServerConfigError {
    ServerConfigError::Invalid {
        key: key.into(),
        value: value.to_string(),
        reason: reason.into(),
    }
}

pub(super) fn sorted_names(names: &HashSet<String>) -> Vec<String> {
    let mut values: Vec<_> = names.iter().cloned().collect();
    values.sort();
    values
}

fn parse_identity_set(key: &str, value: &str) -> Result<HashSet<String>, ServerConfigError> {
    let mut names = HashSet::new();
    for raw in value.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match normalize_player_identity(raw) {
            Ok(identity) => {
                names.insert(identity);
            }
            Err(error) => return Err(invalid(key, raw, error.to_string())),
        }
    }
    Ok(names)
}

fn validate_identity_set(key: &str, names: &HashSet<String>) -> Result<(), ServerConfigError> {
    for name in names {
        match normalize_player_identity(name) {
            Ok(normalized) if normalized == *name => {}
            Ok(_) | Err(_) => {
                return Err(invalid(
                    key,
                    name,
                    "must already be a normalized player identity",
                ));
            }
        }
    }
    Ok(())
}

fn parse_bool(key: &str, value: &str) -> Result<bool, ServerConfigError> {
    crate::game_rules::parse_bool_flag(value).ok_or_else(|| invalid(key, value, "expected true or false"))
}

fn parse_range<T>(
    key: &str,
    value: &str,
    range: std::ops::RangeInclusive<T>,
) -> Result<T, ServerConfigError>
where
    T: std::str::FromStr + PartialOrd + Copy + fmt::Display + fmt::Debug,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| invalid(key, value, "expected an integer"))?;
    if range.contains(&parsed) {
        Ok(parsed)
    } else {
        Err(invalid(key, value, format!("must be in {range:?}")))
    }
}
