//! The GPU-independent authoritative world.
//!
//! `ServerWorld` owns simulation state, not transport.  It uses the existing
//! CPU voxel/entity primitives (which are also used by headless tests) and
//! never imports wgpu, winit, audio, camera, or UI modules.

use crate::authority::contract::{AuthoritySnapshot, RevisionClock, WorldMutation};
use crate::block_entity::{default_stub_for_block, BlockEntity, ContainerAccess};
use crate::chunk_manager::ChunkManager;
use crate::commands::{self, Command, TimeCommand};
use crate::dimension::{generate_chunk_with_options, Dimension, WorldGenerationOptions};
use crate::entity::EntityManager;
use crate::game_rules::{WorldRules, WorldType};
use crate::network::protocol::{GameplayOperation, GameplayRequest, PlayerId, RejectReason};
use crate::redstone::RedstoneSystem;
use crate::world::BlockType;
use glam::Vec3;
use std::collections::{BTreeMap, BTreeSet};

const WORLD_BOUND: i32 = 30_000_000;
const FIXED_DT: f32 = 1.0 / 20.0;
const MAX_AUTOMATION_TRANSFERS: usize = 64;
const MAX_FLUID_UPDATES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldDispatchError {
    reason: RejectReason,
}

impl WorldDispatchError {
    pub const fn new(reason: RejectReason) -> Self {
        Self { reason }
    }

    pub const fn reason(self) -> RejectReason {
        self.reason
    }
}

/// All state required to advance one deterministic world tick.
pub struct ServerWorld {
    pub seed: u32,
    pub dimension: Dimension,
    pub world_type: WorldType,
    pub generate_structures: bool,
    pub rules: WorldRules,
    pub time: u64,
    pub revisions: RevisionClock,
    pub chunks: ChunkManager,
    pub entities: EntityManager,
    pub redstone: RedstoneSystem,
    pub container_viewers: BTreeMap<(i32, i32, i32), BTreeSet<PlayerId>>,
    pub sleeping_players: BTreeSet<PlayerId>,
    block_revisions: BTreeMap<(i32, i32, i32), u64>,
    pub last_snapshot: AuthoritySnapshot,
}

impl ServerWorld {
    pub fn new(
        seed: u32,
        dimension: Dimension,
        world_type: WorldType,
        generate_structures: bool,
        rules: WorldRules,
        render_distance: i32,
    ) -> Self {
        let mut world = Self {
            seed,
            dimension,
            world_type,
            generate_structures,
            rules: rules.normalized(),
            time: 0,
            revisions: RevisionClock::new(),
            chunks: ChunkManager::new_in_dimension(render_distance.max(1), dimension),
            entities: EntityManager::new(),
            redstone: RedstoneSystem::new(),
            container_viewers: BTreeMap::new(),
            sleeping_players: BTreeSet::new(),
            block_revisions: BTreeMap::new(),
            last_snapshot: AuthoritySnapshot::empty(),
        };
        world.ensure_chunk(0, 0);
        world
    }

    pub fn ensure_chunk(&mut self, chunk_x: i32, chunk_z: i32) {
        if self.chunks.chunks.contains_key(&(chunk_x, chunk_z)) {
            return;
        }
        let options = WorldGenerationOptions {
            world_type: self.world_type,
            generate_structures: self.generate_structures,
        };
        let chunk =
            generate_chunk_with_options(self.dimension, chunk_x, chunk_z, self.seed, options);
        self.chunks.chunks.insert((chunk_x, chunk_z), chunk);
    }

    pub fn valid_coordinate(&self, x: i32, y: i32, z: i32) -> bool {
        self.dimension.height().contains_y(y)
            && x.unsigned_abs() <= WORLD_BOUND as u32
            && z.unsigned_abs() <= WORLD_BOUND as u32
    }

    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockType {
        self.chunks.get_block(x, y, z)
    }

    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> u8 {
        self.chunks.get_block_state(x, y, z)
    }

    pub fn get_block_entity(&self, x: i32, y: i32, z: i32) -> Option<&BlockEntity> {
        self.chunks.get_block_entity(x, y, z)
    }

    /// Apply a real voxel mutation and return the revision-bearing event.
    pub fn set_block(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        block: BlockType,
        state: u8,
    ) -> Result<Option<WorldMutation>, WorldDispatchError> {
        if !self.valid_coordinate(x, y, z) {
            return Err(WorldDispatchError::new(RejectReason::InvalidCoordinate));
        }
        self.ensure_chunk(x.div_euclid(16), z.div_euclid(16));
        let old_block = self.get_block(x, y, z);
        let old_state = self.get_block_state(x, y, z);
        if old_block == block && old_state == state {
            return Ok(None);
        }
        self.chunks.set_block(x, y, z, block);
        self.chunks.set_block_state(x, y, z, state);
        if let Some(entity) = default_stub_for_block(block) {
            if self.chunks.get_block_entity(x, y, z).is_none() {
                self.chunks.set_block_entity(x, y, z, Some(entity));
            }
        } else if self.chunks.get_block_entity(x, y, z).is_some() {
            self.chunks.set_block_entity(x, y, z, None);
        }
        let revision = self.revisions.allocate();
        self.block_revisions.insert((x, y, z), revision);
        Ok(Some(WorldMutation {
            dimension: self.dimension as u8,
            position: (x, y, z),
            block: block.to_wire(),
            state,
            revision,
        }))
    }

    pub fn validate_request(
        &self,
        request: &GameplayRequest,
        dimension: Dimension,
        position: [f32; 3],
        operator: bool,
    ) -> Result<(), RejectReason> {
        if dimension != self.dimension {
            return Err(RejectReason::InvalidDimension);
        }
        if !position.iter().all(|value| value.is_finite()) {
            return Err(RejectReason::InvalidState);
        }
        if let Some((x, y, z)) = operation_position(&request.operation) {
            if !self.valid_coordinate(x, y, z) {
                return Err(RejectReason::InvalidCoordinate);
            }
            let distance = Vec3::from_array(position)
                .distance_squared(Vec3::new(x as f32, y as f32, z as f32));
            if distance > 8.0 * 8.0 {
                return Err(RejectReason::TooFar);
            }
        }
        if matches!(&request.operation, GameplayOperation::Command { .. }) && !operator {
            return Err(RejectReason::PermissionDenied);
        }
        Ok(())
    }

    /// Dispatch a request after session/sequence/revision validation.
    pub fn dispatch(
        &mut self,
        request: &GameplayRequest,
        player_id: PlayerId,
        operator: bool,
    ) -> Result<Option<WorldMutation>, WorldDispatchError> {
        match &request.operation {
            GameplayOperation::BlockUse { x, y, z, block } => {
                let block = BlockType::from_wire(*block)
                    .ok_or_else(|| WorldDispatchError::new(RejectReason::InvalidState))?;
                self.set_block(*x, *y, *z, block, 0)
            }
            GameplayOperation::Container {
                action,
                x,
                y,
                z,
                slot,
            } => self.dispatch_container(*action, *x, *y, *z, *slot, player_id),
            GameplayOperation::Sleep { x, y, z } => {
                if self.get_block(*x, *y, *z) != BlockType::Bed {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                }
                if !self.sleeping_players.insert(player_id) {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                }
                Ok(Some(self.touch_revision(*x, *y, *z)))
            }
            GameplayOperation::Command { command } => {
                self.dispatch_command(command, operator)?;
                Ok(None)
            }
            GameplayOperation::ItemUse { .. }
            | GameplayOperation::Combat { .. }
            | GameplayOperation::Trade { .. }
            | GameplayOperation::Mount { .. } => {
                Err(WorldDispatchError::new(RejectReason::Unsupported))
            }
        }
    }

    fn dispatch_container(
        &mut self,
        action: u8,
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
        player_id: PlayerId,
    ) -> Result<Option<WorldMutation>, WorldDispatchError> {
        let Some(entity) = self.chunks.get_block_entity(x, y, z) else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        let Some(access) = ContainerAccess::for_entity(entity) else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        if action > 1 {
            // The current envelope does not carry a slot payload.  Refuse a
            // click rather than acknowledging a bookkeeping-only operation.
            return Err(WorldDispatchError::new(RejectReason::Unsupported));
        }
        if usize::from(slot) >= access.slot_count && slot != 0 {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        }
        if action == 0 {
            self.container_viewers
                .entry((x, y, z))
                .or_default()
                .remove(&player_id);
        } else {
            self.container_viewers
                .entry((x, y, z))
                .or_default()
                .insert(player_id);
        }
        Ok(Some(self.touch_revision(x, y, z)))
    }

    fn dispatch_command(&mut self, input: &str, _operator: bool) -> Result<(), WorldDispatchError> {
        let command = commands::parse(input)
            .map_err(|_| WorldDispatchError::new(RejectReason::InvalidState))?;
        match command {
            Command::GameRule { rule, value } => {
                let Some(value) = value else {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                };
                if let Ok(bool_value) = value.parse::<bool>() {
                    self.rules
                        .set(&rule, bool_value)
                        .map_err(|_| WorldDispatchError::new(RejectReason::InvalidState))?;
                } else if matches!(
                    rule.as_str(),
                    "playerssleepingpercentage" | "sleepingpercentage" | "sleeping_percentage"
                ) {
                    let percentage = value
                        .parse::<u8>()
                        .map_err(|_| WorldDispatchError::new(RejectReason::InvalidState))?;
                    self.rules.set_sleeping_percentage(percentage);
                } else {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                }
            }
            Command::Time(TimeCommand::Set(time)) => self.time = time,
            Command::Time(TimeCommand::Add(time)) => self.time = self.time.wrapping_add(time),
            // Session commands are handled by AuthorityCore because they need
            // access to authenticated session state.
            Command::GameMode { .. } | Command::Teleport { .. } => {
                return Err(WorldDispatchError::new(RejectReason::Unsupported));
            }
            Command::Help(_)
            | Command::Difficulty(_)
            | Command::Weather(_)
            | Command::Give { .. }
            | Command::Kill(_)
            | Command::SpawnPoint { .. }
            | Command::SetWorldSpawn(_)
            | Command::Locate(_)
            | Command::Seed
            | Command::SaveAll => {
                return Err(WorldDispatchError::new(RejectReason::Unsupported));
            }
        }
        Ok(())
    }

    fn touch_revision(&mut self, x: i32, y: i32, z: i32) -> WorldMutation {
        let revision = self.revisions.allocate();
        self.block_revisions.insert((x, y, z), revision);
        WorldMutation {
            dimension: self.dimension as u8,
            position: (x, y, z),
            block: self.get_block(x, y, z).to_wire(),
            state: self.get_block_state(x, y, z),
            revision,
        }
    }

    /// Advance exactly one 20 Hz tick.  All iteration order is normalized so
    /// the checksum and mutation revisions are topology-independent.
    pub fn tick(&mut self, players: &[(PlayerId, [f32; 3])]) -> AuthoritySnapshot {
        if self.rules.do_daylight_cycle {
            self.time = self.time.wrapping_add(1);
        }
        let mut mutations = Vec::new();

        let mut occupants: Vec<_> = players
            .iter()
            .filter_map(|(_, position)| {
                position.iter().all(|value| value.is_finite()).then_some((
                    position[0].floor() as i32,
                    position[1].floor() as i32,
                    position[2].floor() as i32,
                ))
            })
            .collect();
        occupants.sort_unstable();
        occupants.dedup();
        let redstone = self.redstone.tick(&mut self.chunks, &occupants);
        let mut redstone_mutations = redstone.mutations;
        redstone_mutations.sort_by_key(|mutation| mutation.pos);
        for mutation in redstone_mutations {
            if let Ok(Some(event)) = self.set_block(
                mutation.pos.0,
                mutation.pos.1,
                mutation.pos.2,
                mutation.new_block,
                0,
            ) {
                mutations.push(event);
            }
        }

        // These systems mutate actual block entities/chunks, not a shadow map.
        let _ = crate::world_tick::tick_hoppers_with_entities(
            &mut self.chunks,
            Some(&mut self.entities),
            MAX_AUTOMATION_TRANSFERS,
        );
        for is_lava in [false, true] {
            let (_, fluid_mutations) =
                crate::fluid::tick_fluids(&mut self.chunks, is_lava, MAX_FLUID_UPDATES);
            for (position, block) in fluid_mutations {
                if let Ok(Some(event)) =
                    self.set_block(position.0, position.1, position.2, block, 0)
                {
                    mutations.push(event);
                }
            }
        }

        self.tick_entities(players);
        mutations.sort_by_key(|mutation| mutation.revision);
        let checksum = self.checksum(&mutations);
        let snapshot = AuthoritySnapshot {
            tick: self.time,
            revision: self.revisions.current(),
            checksum,
            mutations,
        };
        self.last_snapshot = snapshot.clone();
        snapshot
    }

    fn tick_entities(&mut self, players: &[(PlayerId, [f32; 3])]) {
        let mut player_positions: Vec<_> = players.to_vec();
        player_positions.sort_by_key(|(id, _)| *id);
        let chunks = &self.chunks;
        for entity in &mut self.entities.entities {
            if entity.entity_type.is_hostile() && self.rules.do_mob_spawning {
                if let Some((_, target)) =
                    player_positions.iter().min_by(|(_, left), (_, right)| {
                        entity
                            .position
                            .distance_squared(Vec3::from_array(*left))
                            .total_cmp(&entity.position.distance_squared(Vec3::from_array(*right)))
                    })
                {
                    let direction =
                        (Vec3::from_array(*target) - entity.position).normalize_or_zero();
                    entity.velocity.x = direction.x * 1.2;
                    entity.velocity.z = direction.z * 1.2;
                    entity.target_player = true;
                }
            }
            entity.ai_phase = entity.ai_phase.wrapping_add(1);
            entity.ai_timer += FIXED_DT;
            entity.update_physics(FIXED_DT, chunks);
        }
        self.entities.sync_positions();
    }

    fn checksum(&self, mutations: &[WorldMutation]) -> u64 {
        // Stable FNV-1a over authoritative values.  HashMap iteration is never
        // used directly; chunks and block revisions are sorted first.
        let mut hash = 0xcbf29ce484222325u64;
        let mut write = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
        };
        write(&self.time.to_le_bytes());
        write(&self.revisions.current().to_le_bytes());
        write(&[
            self.rules.keep_inventory as u8,
            self.rules.mob_griefing as u8,
        ]);
        write(&[
            self.rules.do_daylight_cycle as u8,
            self.rules.do_mob_spawning as u8,
        ]);
        for mutation in mutations {
            write(&mutation.dimension.to_le_bytes());
            write(&mutation.position.0.to_le_bytes());
            write(&mutation.position.1.to_le_bytes());
            write(&mutation.position.2.to_le_bytes());
            write(&mutation.block.to_le_bytes());
            write(&mutation.state.to_le_bytes());
            write(&mutation.revision.to_le_bytes());
        }
        for (&position, &revision) in &self.block_revisions {
            write(&position.0.to_le_bytes());
            write(&position.1.to_le_bytes());
            write(&position.2.to_le_bytes());
            write(&revision.to_le_bytes());
        }
        let mut entities: Vec<_> = self
            .entities
            .entities
            .iter()
            .map(|entity| {
                (
                    entity.id,
                    entity.position.x.to_bits(),
                    entity.position.y.to_bits(),
                    entity.position.z.to_bits(),
                    entity.ai_phase,
                )
            })
            .collect();
        entities.sort_unstable();
        for entity in entities {
            write(&entity.0.to_le_bytes());
            write(&entity.1.to_le_bytes());
            write(&entity.2.to_le_bytes());
            write(&entity.3.to_le_bytes());
            write(&entity.4.to_le_bytes());
        }
        hash
    }
}

fn operation_position(operation: &GameplayOperation) -> Option<(i32, i32, i32)> {
    match operation {
        GameplayOperation::BlockUse { x, y, z, .. }
        | GameplayOperation::Sleep { x, y, z }
        | GameplayOperation::Container { x, y, z, .. } => Some((*x, *y, *z)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::contract::AuthorityTopology;
    use crate::authority::{AuthorityConfig, AuthorityCore};
    use crate::entity::EntityType;
    use crate::network::protocol::{GameplayOutcome, GameplayRequest};

    #[test]
    fn block_mutation_changes_real_chunk_and_revision() {
        let mut world = ServerWorld::new(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2,
        );
        let old = world.get_block(8, 80, 8);
        let mutation = world
            .set_block(8, 80, 8, BlockType::Chest, 0)
            .unwrap()
            .unwrap();
        assert_ne!(old, BlockType::Chest);
        assert_eq!(world.get_block(8, 80, 8), BlockType::Chest);
        assert_eq!(world.get_block_entity(8, 80, 8).is_some(), true);
        assert_eq!(mutation.revision, 1);
    }

    #[test]
    fn fixed_tick_checksum_is_deterministic() {
        let make = || {
            let mut world = ServerWorld::new(
                7,
                Dimension::Overworld,
                WorldType::Superflat,
                false,
                WorldRules::default(),
                2,
            );
            world
                .entities
                .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
            world.tick(&[(7, [8.0, 80.0, 8.0])])
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn unsupported_gameplay_is_explicitly_rejected() {
        let mut core = AuthorityCore::new(AuthorityConfig::default(), AuthorityTopology::Dedicated);
        core.register_session(crate::authority::contract::SessionContract::new(
            7,
            "alex",
            0,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();
        let request = GameplayRequest {
            request_id: 1,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::ItemUse { item: 1, count: 1 },
        };
        assert!(matches!(
            core.submit_request(request).outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::Unsupported
            }
        ));
    }
}
