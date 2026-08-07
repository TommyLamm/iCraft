use icraft::server_runtime::{ServerProperties, ServerRuntime};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("[icraft-server] fatal: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    let config_path = value(&args, "--config")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("server.properties"));
    let mut properties = ServerProperties::load(&config_path)?;
    if let Some(world) = value(&args, "--world") {
        properties.world_dir = PathBuf::from(world);
    }
    if let Some(bind) = value(&args, "--bind") {
        properties.bind = bind;
    }
    if let Some(port) = value(&args, "--port") {
        properties.port = port.parse()?;
    }
    if let Some(max_players) = value(&args, "--max-players") {
        properties.max_players = max_players.parse()?;
    }
    if let Some(view_distance) = value(&args, "--view-distance") {
        properties.view_distance = view_distance.parse()?;
    }
    if let Some(simulation_distance) = value(&args, "--simulation-distance") {
        properties.simulation_distance = simulation_distance.parse()?;
    }
    if let Some(difficulty) = value(&args, "--difficulty") {
        properties.difficulty = parse_difficulty(&difficulty)?;
    }
    if let Some(motd) = value(&args, "--motd") {
        if motd.trim().is_empty() || motd.len() > 256 {
            return Err("--motd must contain 1..=256 bytes".into());
        }
        properties.motd = motd;
    }
    if let Some(pvp) = value(&args, "--pvp") {
        properties.pvp = parse_bool_flag("--pvp", &pvp)?;
    }
    if let Some(online_mode) = value(&args, "--online-mode") {
        properties.online_mode = parse_bool_flag("--online-mode", &online_mode)?;
    }
    if let Some(whitelist) = value(&args, "--whitelist") {
        properties.whitelist = parse_names(&whitelist);
    }
    if let Some(operators) = value(&args, "--operators") {
        properties.operators = parse_names(&operators);
    }
    if let Some(seed) = value(&args, "--seed") {
        properties.seed = seed.parse::<i64>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--seed must be a signed 64-bit integer",
            )
        })? as u64;
    }
    properties.validate()?;
    validate_world_path(&properties.world_dir)?;
    if !config_path.exists() {
        properties.write(&config_path)?;
    }

    eprintln!(
        "[icraft-server] starting {}:{} world={} seed={} difficulty={} pvp={} motd={:?} max_players={} view_distance={} simulation_distance={} whitelist={} operators={} online_mode={}",
        properties.bind,
        properties.port,
        properties.world_dir.display(),
        properties.seed,
        properties.difficulty,
        properties.pvp,
        properties.motd,
        properties.max_players,
        properties.view_distance,
        properties.simulation_distance,
        properties.whitelist.len(),
        properties.operators.len(),
        properties.online_mode,
    );
    let mut server = ServerRuntime::new(properties)?;
    if let Some(ticks) = value(&args, "--ticks") {
        server.run_for_ticks(ticks.parse()?)?;
        return server.shutdown().map_err(Into::into);
    }
    if args.iter().any(|arg| arg == "--once") {
        server.tick()?;
        return server.shutdown().map_err(Into::into);
    }

    let (console_tx, console_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if console_tx.send(line).is_err() {
                break;
            }
        }
    });

    let tokio_runtime = tokio::runtime::Runtime::new()?;
    tokio_runtime.block_on(async {
        loop {
            while let Ok(command) = console_rx.try_recv() {
                match server.execute_console_command(&command) {
                    Ok(message) if !message.is_empty() => eprintln!("[icraft-server] {message}"),
                    Ok(_) => {}
                    Err(error) => eprintln!("[icraft-server] {error}"),
                }
            }
            if server.is_stopped() {
                break;
            }
            tokio::select! {
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    eprintln!("[icraft-server] shutdown signal received; flushing saves");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    server.tick()?;
                }
            }
        }
        server.shutdown().map_err(Into::into)
    })
}

fn value(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == key)
        .map(|window| window[1].clone())
}

fn parse_difficulty(value: &str) -> Result<String, io::Error> {
    let normalized = value.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "peaceful" | "easy" | "normal" | "hard") {
        Ok(normalized)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("--difficulty must be peaceful, easy, normal, or hard (got {value:?})"),
        ))
    }
}

fn parse_bool_flag(key: &str, value: &str) -> Result<bool, io::Error> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{key} must be true/false (got {value:?})"),
        )),
    }
}

fn parse_names(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn validate_world_path(path: &Path) -> Result<(), io::Error> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "world path must not be empty",
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("world path {} must not be a symlink", path.display()),
            ));
        }
        if !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("world path {} is not a directory", path.display()),
            ));
        }
        return Ok(());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Ok(metadata) = fs::symlink_metadata(parent) {
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("world parent {} must not be a symlink", parent.display()),
            ));
        }
    }
    Ok(())
}

fn print_help() {
    println!(
        "icraft-server [--config PATH] [--world PATH] [--bind IP] [--port PORT]\n  [--max-players N] [--view-distance N] [--simulation-distance N]\n  [--difficulty peaceful|easy|normal|hard] [--motd TEXT] [--pvp BOOL]\n  [--online-mode BOOL] [--whitelist USERS] [--operators USERS] [--seed N]\n  [--ticks N|--once]\n\nRuns the headless authoritative server. Ctrl-C flushes player/level saves."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_management_fields_are_strict_and_normalized() {
        assert_eq!(parse_difficulty(" HARD ").unwrap(), "hard");
        assert!(parse_difficulty("unknown").is_err());
        assert!(parse_bool_flag("--pvp", "on").unwrap());
        assert!(!parse_bool_flag("--pvp", "off").unwrap());
        assert!(parse_bool_flag("--pvp", "maybe").is_err());
        let names = parse_names(" Alex, steve, ,ALEX ");
        assert_eq!(names.len(), 2);
        assert!(names.contains("alex"));
        assert!(names.contains("steve"));
    }

    #[test]
    fn invalid_world_path_fails_before_runtime_creation() {
        let path =
            std::env::temp_dir().join(format!("icraft-server-file-world-{}", std::process::id()));
        fs::write(&path, b"not a directory").unwrap();
        assert!(validate_world_path(&path).is_err());
        let _ = fs::remove_file(path);
    }
}
