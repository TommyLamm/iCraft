use icraft::server_runtime::{ServerProperties, ServerRuntime};
use std::io::BufRead;
use std::path::PathBuf;
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
    properties.validate()?;
    if !config_path.exists() {
        properties.write(&config_path)?;
    }

    eprintln!(
        "[icraft-server] starting {}:{} world={} max_players={} view_distance={} simulation_distance={}",
        properties.bind,
        properties.port,
        properties.world_dir.display(),
        properties.max_players,
        properties.view_distance,
        properties.simulation_distance,
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

fn print_help() {
    println!(
        "icraft-server [--config PATH] [--world PATH] [--bind IP] [--port PORT]\n  [--max-players N] [--view-distance N] [--simulation-distance N]\n  [--ticks N|--once]\n\nRuns the headless authoritative server. Ctrl-C flushes player/level saves."
    );
}
