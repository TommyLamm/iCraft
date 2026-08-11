use glam::Vec3;
use icraft::authority::contract::AuthorityTopology;
use icraft::entity::EntityType;
use icraft::game_rules::{ServerDifficulty, WorldRules, WorldType};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, ServerProperties, ServerRuntime, TransportMode,
};
use icraft::server_world::ServerWorld;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_WORLD: AtomicU64 = AtomicU64::new(0);

fn temp_world(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let suffix = NEXT_WORLD.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "icraft-difficulty-{label}-{}-{nonce}-{suffix}",
        std::process::id()
    ))
}

fn available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve an isolated port")
        .local_addr()
        .expect("read isolated port")
        .port()
}

fn properties(label: &str, difficulty: &str) -> ServerProperties {
    ServerProperties {
        bind: "127.0.0.1".into(),
        port: available_port(),
        difficulty: difficulty.into(),
        world_dir: temp_world(label),
        view_distance: 2,
        simulation_distance: 2,
        ..ServerProperties::default()
    }
}

#[test]
fn server_difficulty_is_strict_and_pvp_remains_independent() {
    let mut invalid = properties("invalid", "extreme");
    let invalid_dir = invalid.world_dir.clone();
    assert!(ServerRuntime::new_embedded(
        invalid.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(1, "invalid")),
    )
    .is_err());
    assert!(
        !invalid_dir.exists(),
        "invalid config must fail before SaveManager"
    );

    invalid.difficulty = "PEACEFUL".into();
    invalid.pvp = true;
    let world_dir = invalid.world_dir.clone();
    let (mut runtime, _) = ServerRuntime::new_embedded(
        invalid,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(2, "peaceful")),
    )
    .expect("peaceful config should construct");
    assert_eq!(
        runtime.authority.world.difficulty,
        ServerDifficulty::Peaceful
    );
    assert!(runtime.authority.world.rules.pvp);
    runtime.shutdown().expect("shutdown should persist cleanly");
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn difficulty_policy_is_observable_and_existing_hostiles_are_not_frozen_by_gamerule() {
    let mut rules = WorldRules::default();
    rules.do_mob_spawning = false;
    let mut speeds = Vec::new();
    for difficulty in [
        ServerDifficulty::Easy,
        ServerDifficulty::Normal,
        ServerDifficulty::Hard,
    ] {
        let mut world = ServerWorld::new_with_difficulty(
            7,
            icraft::dimension::Dimension::Overworld,
            WorldType::Superflat,
            false,
            rules,
            2,
            difficulty,
        );
        assert!(!world.allows_hostile_spawning());
        let id = world
            .entities
            .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
        world.tick(&[(7, [8.0, 80.0, 8.0])]);
        let entity = world
            .entities
            .get_by_id(id)
            .expect("hostile remains loaded");
        speeds.push(entity.velocity.x.abs());
    }
    assert!(speeds[0] < speeds[1] && speeds[1] < speeds[2]);

    let mut peaceful = ServerWorld::new_with_difficulty(
        7,
        icraft::dimension::Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2,
        ServerDifficulty::Peaceful,
    );
    peaceful
        .entities
        .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
    peaceful.tick(&[(7, [8.0, 80.0, 8.0])]);
    assert!(peaceful
        .entities
        .entities
        .iter()
        .all(|entity| !entity.entity_type.is_hostile()));
}

#[test]
fn difficulty_persists_through_server_properties_and_embedded_dedicated_parity() {
    let persistence_props = properties("persistence", "hard");
    let world_dir = persistence_props.world_dir.clone();
    let (mut embedded, _) = ServerRuntime::new_embedded(
        persistence_props.clone(),
        EmbeddedRuntimeOptions {
            topology: AuthorityTopology::ListenServer,
            transport: TransportMode::Disabled,
            local_session: Some(LocalSessionProfile::new(3, "embedded")),
        },
    )
    .expect("embedded listen topology should construct");
    assert_eq!(embedded.authority.world.difficulty, ServerDifficulty::Hard);
    embedded
        .save_all()
        .expect("save should persist difficulty policy");
    let properties_path = world_dir.join("server.properties");
    let loaded = ServerProperties::load(&properties_path).expect("reload server.properties");
    assert_eq!(loaded.difficulty_kind().unwrap(), ServerDifficulty::Hard);
    embedded.shutdown().expect("embedded shutdown");

    let (mut reloaded, _) = ServerRuntime::new_embedded(
        loaded.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(4, "reloaded")),
    )
    .expect("reloaded embedded runtime");
    assert_eq!(reloaded.authority.world.difficulty, ServerDifficulty::Hard);
    reloaded.shutdown().expect("reloaded shutdown");

    let dedicated_props = properties("dedicated", "hard");
    let dedicated_dir = dedicated_props.world_dir.clone();
    let mut dedicated = ServerRuntime::new(dedicated_props).expect("dedicated runtime");
    assert_eq!(dedicated.authority.topology, AuthorityTopology::Dedicated);
    assert_eq!(dedicated.authority.world.difficulty, ServerDifficulty::Hard);
    dedicated.shutdown().expect("dedicated shutdown");

    let _ = fs::remove_dir_all(world_dir);
    let _ = fs::remove_dir_all(dedicated_dir);
}
