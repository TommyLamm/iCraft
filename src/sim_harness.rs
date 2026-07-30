//! Deterministic, CPU-only simulation harness used to validate the fixed-step kernel.
use crate::{
    chunk_manager::ChunkManager,
    entity::{EntityManager, EntityType},
    fluid, lighting,
    physics::{PlayerPhysics, PLAYER_PHYSICS_TICK_DT},
    redstone::RedstoneSystem,
    world::{BlockType, Chunk},
};
use glam::Vec3;
use std::collections::HashSet;

pub const TICK_HZ: f64 = 20.0;
const TICK_DT: f32 = PLAYER_PHYSICS_TICK_DT;
const SEED: u32 = 0x1C4F_0007;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldTime {
    pub ticks: u64,
}

pub struct SimHarness {
    pub chunks: ChunkManager,
    pub lighting_dirty: HashSet<(i32, i32)>,
    pub redstone: RedstoneSystem,
    pub entities: EntityManager,
    pub player: PlayerPhysics,
    pub player_health: f32,
    pub world_time: WorldTime,
    pub tick_count: u64,
}

impl SimHarness {
    pub fn new() -> Self {
        let mut chunks = ChunkManager::new_in_dimension(2, crate::dimension::Dimension::Overworld);
        for cx in -1..=1 {
            for cz in -1..=1 {
                chunks
                    .chunks
                    .insert((cx, cz), Chunk::new_with_seed(cx, cz, SEED));
            }
        }
        let mut h = Self {
            chunks,
            lighting_dirty: HashSet::new(),
            redstone: RedstoneSystem::new(),
            entities: EntityManager::new(),
            player: PlayerPhysics::new(Vec3::new(8.5, 72.0, 8.5)),
            player_health: 20.0,
            world_time: WorldTime::default(),
            tick_count: 0,
        };
        h.script_inputs();
        h
    }

    fn script_inputs(&mut self) {
        self.chunks.set_block(8, 70, 8, BlockType::Stone);
        self.chunks.set_block(8, 71, 8, BlockType::Torch);
        self.chunks.set_block(10, 70, 8, BlockType::Water);
        self.chunks.set_fluid_level(10, 70, 8, 0);
        self.chunks.set_block_state(8, 71, 8, 1);
        lighting::update_sky_light_after_placed(
            &mut self.chunks,
            8,
            70,
            8,
            &mut self.lighting_dirty,
        );
        lighting::update_block_light_after_placed(
            &mut self.chunks,
            8,
            71,
            8,
            BlockType::Torch.properties().light_emission,
            &mut self.lighting_dirty,
        );
        self.redstone
            .on_block_changed(&self.chunks, (8, 70, 8), crate::redstone::Direction::North);
        self.entities
            .spawn(EntityType::Pig, Vec3::new(12.5, 72.0, 8.5));
    }

    pub fn tick(&mut self) {
        let _ = fluid::tick_fluids(&mut self.chunks, false, 64);
        let _ = fluid::tick_fluids(&mut self.chunks, true, 64);
        let occupants = [(
            self.player.position.x.floor() as i32,
            self.player.position.y.floor() as i32,
            self.player.position.z.floor() as i32,
        )];
        let _ = self.redstone.tick(&mut self.chunks, &occupants);
        self.player.update(
            TICK_DT,
            &self.chunks,
            Vec3::new(0.2, 0.0, 0.1),
            false,
            false,
        );
        for entity in &mut self.entities.entities {
            entity.update_physics(TICK_DT, &self.chunks);
        }
        self.entities.sync_positions();
        self.world_time.ticks += 1;
        self.tick_count += 1;
    }

    pub fn checksum(&self) -> u64 {
        let mut bytes = Vec::new();
        let mut coords: Vec<_> = self.chunks.chunks.keys().copied().collect();
        coords.sort_unstable();
        for (cx, cz) in coords {
            let chunk = &self.chunks.chunks[&(cx, cz)];
            for y in 0..crate::world::CHUNK_HEIGHT {
                for z in 0..16 {
                    for x in 0..16 {
                        let p = (cx * 16 + x as i32, y as i32, cz * 16 + z as i32);
                        bytes.extend_from_slice(&p.0.to_le_bytes());
                        bytes.extend_from_slice(&p.1.to_le_bytes());
                        bytes.extend_from_slice(&p.2.to_le_bytes());
                        bytes.extend_from_slice(
                            &chunk.get_block_local(x, y, z).to_wire().to_le_bytes(),
                        );
                        bytes.push(self.chunks.get_block_state(p.0, p.1, p.2));
                        bytes.push(self.chunks.get_sky_light(p.0, p.1, p.2));
                        bytes.push(self.chunks.get_block_light(p.0, p.1, p.2));
                        bytes.push(self.chunks.get_fluid_level(p.0, p.1, p.2));
                    }
                }
            }
        }
        // Redstone keeps mutable runtime state outside chunk storage. Use its
        // canonical snapshot so component metadata, queued work, dirty sets,
        // and sleep/occupant bookkeeping all participate in the world digest.
        bytes.extend_from_slice(&self.redstone.canonical_snapshot());
        let mut entities: Vec<_> = self.entities.entities.iter().collect();
        entities.sort_by_key(|e| e.id);
        for e in entities {
            bytes.extend_from_slice(&e.id.to_le_bytes());
            bytes.extend_from_slice(&e.position.x.to_bits().to_le_bytes());
            bytes.extend_from_slice(&e.position.y.to_bits().to_le_bytes());
            bytes.extend_from_slice(&e.position.z.to_bits().to_le_bytes());
            bytes.extend_from_slice(&e.health.to_bits().to_le_bytes());
            bytes.extend_from_slice(format!("{:?}", e.entity_type).as_bytes());
        }
        bytes.extend_from_slice(&self.player.position.x.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.player.position.y.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.player.position.z.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.player_health.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.world_time.ticks.to_le_bytes());
        fnv1a(&bytes)
    }
}

fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

pub fn run_for_fps(fps: u32, seconds: f64) -> (u64, u64) {
    let mut sim = SimHarness::new();
    let mut debt = 0.0;
    let frames = (seconds * fps as f64).round() as u32;
    for _ in 0..frames {
        debt += 1.0 / fps as f64;
        let mut n = 0;
        while debt + 1.0e-9 >= 1.0 / TICK_HZ && n < 4 {
            sim.tick();
            debt -= 1.0 / TICK_HZ;
            n += 1;
        }
    }
    (sim.tick_count, sim.checksum())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fps_is_deterministic() {
        let baseline = run_for_fps(30, 3.0);
        assert_eq!(baseline.0, 60);
        for fps in [30, 60, 144, 240] {
            assert_eq!(baseline, run_for_fps(fps, 3.0));
        }
    }
    #[test]
    fn checksum_domains_are_covered() {
        let base = SimHarness::new().checksum();
        let mut b = SimHarness::new();
        b.chunks.set_block(3, 70, 3, BlockType::Glass);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_block_state(8, 71, 8, 7);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_sky_light(3, 70, 3, 4);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_block_light(3, 70, 3, 4);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_fluid_level(10, 70, 8, 3);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.entities.spawn(EntityType::Cow, Vec3::new(2.0, 72.0, 2.0));
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.entities.entities[0].health -= 1.0;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.player.position.x += 1.0;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.player_health -= 1.0;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.world_time.ticks += 1;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        let _ = b.redstone.tick(&mut b.chunks, &[]);
        assert_ne!(base, b.checksum());

        let mut b = SimHarness::new();
        b.chunks.set_block(8, 71, 8, BlockType::Repeater);
        b.redstone
            .on_block_changed(&b.chunks, (8, 71, 8), crate::redstone::Direction::North);
        let component_default = b.checksum();
        b.redstone.set_repeater_delay((8, 71, 8), 4);
        assert_ne!(component_default, b.checksum());
    }
    #[test]
    fn high_speed_player_collision_is_bounded() {
        let mut h = SimHarness::new();
        h.player.position = Vec3::new(0.5, 200.0, 0.5);
        h.player.highest_y = h.player.position.y;
        h.chunks.set_block(3, 200, 0, BlockType::Stone);
        h.chunks.set_block(3, 201, 0, BlockType::Stone);
        h.player.set_flying(true);
        h.player.update(
            TICK_DT,
            &h.chunks,
            Vec3::new(1_000.0, 0.0, 0.0),
            false,
            true,
        );
        assert!((h.player.position.x - 2.7).abs() < 1.0e-5);
        assert!(!h
            .player
            .get_aabb()
            .intersects(&crate::physics::unit_block_aabb((3, 200, 0))));
    }
}
