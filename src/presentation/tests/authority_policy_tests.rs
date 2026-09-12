// Tests extracted from state.rs::authority_policy_tests (Plan 27).

use super::*;

use super::*;
use crate::server_runtime::projection::entity_state_wire;

#[test]
fn multiplayer_host_keeps_world_ticks_running_while_paused_or_dead() {
    let host = MultiplayerRole::Host { port: 25565 };
    assert!(should_advance_simulation(&host, true, true, false));
    assert!(should_advance_simulation(&host, true, false, true));
    assert!(should_advance_simulation(&host, true, true, true));
    assert!(!should_advance_simulation(&host, false, false, false));
}

#[test]
fn singleplayer_pause_and_death_still_stop_world_ticks() {
    assert!(!should_advance_simulation(
        &MultiplayerRole::Singleplayer,
        true,
        true,
        false,
    ));
    assert!(!should_advance_simulation(
        &MultiplayerRole::Singleplayer,
        true,
        false,
        true,
    ));
    assert!(should_advance_simulation(
        &MultiplayerRole::Singleplayer,
        true,
        false,
        false,
    ));
}

#[test]
fn replicated_entity_samples_interpolate_without_mutating_authority() {
    let mut replicated = ReplicatedEntityState::new(99);
    let state = |sequence, x| crate::network::protocol::EntityStateWire {
        entity_id: 7,
        entity_type: crate::entity::EntityType::Zombie.to_wire(),
        position: [x, 64.0, 0.0],
        velocity: [1.0, 0.0, 0.0],
        yaw: 0.0,
        pitch: 0.0,
        health: 20.0 - sequence as f32,
        animation_state: 0,
        item: None,
    };
    assert!(!replicated.push(state(1, 0.0), 1, 1.0));
    assert!(!replicated.push(state(2, 2.0), 2, 2.0));
    let sample = replicated.sample(1.5).unwrap();
    assert_eq!(sample.position, [1.0, 64.0, 0.0]);
    assert_eq!(sample.health, 18.5);
    assert_eq!(replicated.snapshots.back().unwrap().state.position[0], 2.0);
}

#[test]
fn host_client_sixty_second_entity_checksum_converges_without_client_spawns() {
    let host_player = Vec3::new(0.0, 64.0, 0.0);
    let client_player = Vec3::new(96.0, 64.0, 96.0);
    assert!(host_player.distance(client_player) > 128.0);

    let mut host = crate::entity::Entity::new(
        7,
        crate::entity::EntityType::Zombie,
        Vec3::new(8.0, 64.0, 8.0),
    );
    host.velocity = Vec3::new(0.5, 0.0, -0.25);
    let mut client_entities = crate::entity::EntityManager::new();
    let local_id = client_entities.spawn(host.entity_type, host.position);
    let mut replica = ReplicatedEntityState::new(local_id);

    for tick in 1..=1_200u64 {
        host.position += host.velocity * SIM_TICK_TIME;
        host.yaw += 0.0025;
        let state = entity_state_wire(&host);
        assert!(!replica.push(state, tick, tick as f64 * f64::from(SIM_TICK_TIME)));
        let visual = replica
            .sample(tick as f64 * f64::from(SIM_TICK_TIME))
            .unwrap();
        apply_entity_wire_state(client_entities.get_by_id_mut(local_id).unwrap(), visual);
        assert_eq!(
            client_entities.entities.len(),
            1,
            "client spawned an authority-owned living entity at tick {tick}"
        );
    }

    let client = client_entities.get_by_id(local_id).unwrap();
    let checksum = |entity: &crate::entity::Entity| {
        entity.position.x.to_bits() as u64
            ^ (entity.position.y.to_bits() as u64).rotate_left(11)
            ^ (entity.position.z.to_bits() as u64).rotate_left(22)
            ^ (entity.health.to_bits() as u64).rotate_left(33)
    };
    assert_eq!(checksum(client), checksum(&host));
}

#[test]
fn host_clamps_remote_pose_before_echoing_authoritative_correction() {
    let latest = PlayerSnapshot {
        position: Vec3::ZERO,
        yaw: 0.0,
        pitch: 0.0,
        time: 0.0,
        sequence: 1,
        sender_time_millis: 1_000,
    };
    let accepted = validated_remote_position(Some(&latest), Vec3::new(1.2, 0.0, 0.0), 1_050);
    assert_eq!(accepted, Vec3::new(1.2, 0.0, 0.0));

    let corrected = validated_remote_position(Some(&latest), Vec3::new(100.0, 0.0, 0.0), 1_050);
    assert!((corrected.x - 1.6).abs() < f32::EPSILON);
    assert_eq!(
        validated_remote_position(Some(&latest), Vec3::ONE, 999),
        Vec3::ZERO
    );
}
