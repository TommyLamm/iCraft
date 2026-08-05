use crate::chunk_manager::ChunkManager;
use crate::dimension::Dimension;
use crate::entity::{Entity, EntityManager, EntityType};
use crate::menu::Difficulty;
use crate::world::BlockType;
use glam::Vec3;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MobCategory {
    Monster,
    Creature,
    Ambient,
    WaterCreature,
}

impl MobCategory {
    pub const fn cap_per_player(self) -> usize {
        match self {
            Self::Monster => 70,
            Self::Creature => 10,
            Self::Ambient => 15,
            Self::WaterCreature => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnReason {
    Natural,
    Structure,
}

#[derive(Debug, Clone)]
pub struct SpawningSystem {
    pub category_counts: HashMap<MobCategory, usize>,
}

impl Default for SpawningSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl SpawningSystem {
    pub fn new() -> Self {
        let mut category_counts = HashMap::new();
        category_counts.insert(MobCategory::Monster, 0);
        category_counts.insert(MobCategory::Creature, 0);
        category_counts.insert(MobCategory::Ambient, 0);
        category_counts.insert(MobCategory::WaterCreature, 0);
        Self { category_counts }
    }

    pub fn update_category_counts(&mut self, entity_manager: &EntityManager) {
        let mut counts = HashMap::new();
        counts.insert(MobCategory::Monster, 0);
        counts.insert(MobCategory::Creature, 0);
        counts.insert(MobCategory::Ambient, 0);
        counts.insert(MobCategory::WaterCreature, 0);

        for entity in &entity_manager.entities {
            if entity.health <= 0.0 {
                continue;
            }
            if let Some(cat) = mob_category_for_type(entity.entity_type) {
                *counts.entry(cat).or_insert(0) += 1;
            }
        }
        self.category_counts = counts;
    }

    pub fn can_spawn_category(&self, category: MobCategory, player_count: usize) -> bool {
        let max_players = player_count.max(1);
        let cap = category.cap_per_player() * max_players;
        let current = self.category_counts.get(&category).copied().unwrap_or(0);
        current < cap
    }

    pub fn is_valid_spawn_location(
        chunk_manager: &ChunkManager,
        pos: Vec3,
        entity_type: EntityType,
        dimension: Dimension,
        difficulty: Difficulty,
        reason: SpawnReason,
        player_positions: &[Vec3],
    ) -> bool {
        if entity_type.is_hostile() && difficulty == Difficulty::Peaceful {
            return false;
        }

        let cat = match mob_category_for_type(entity_type) {
            Some(c) => c,
            None => return false,
        };

        // Player distance checks for natural spawns
        if reason == SpawnReason::Natural && !player_positions.is_empty() {
            let min_dist_sq = player_positions
                .iter()
                .map(|p| p.distance_squared(pos))
                .fold(f32::INFINITY, f32::min);

            // Natural spawn boundary: 24.0 .. 128.0 blocks from any player
            if min_dist_sq < 24.0 * 24.0 || min_dist_sq > 128.0 * 128.0 {
                return false;
            }
        }

        let bx = pos.x.floor() as i32;
        let by = pos.y.floor() as i32;
        let bz = pos.z.floor() as i32;

        let height = dimension.height();
        if !height.contains_y(by) || !height.contains_y(by - 1) {
            return false;
        }

        let ground_block = chunk_manager.get_block(bx, by - 1, bz);
        let spawn_block = chunk_manager.get_block(bx, by, bz);
        let head_block = chunk_manager.get_block(bx, by + 1, bz);

        match cat {
            MobCategory::WaterCreature => {
                // Must spawn in water
                spawn_block == BlockType::Water
            }
            MobCategory::Monster => {
                if entity_type == EntityType::Drowned {
                    spawn_block == BlockType::Water
                } else if dimension == Dimension::Nether {
                    // Nether monsters spawn on Netherrack, Soul Sand, Basalt, etc.
                    ground_block != BlockType::Air
                        && ground_block != BlockType::Lava
                        && ground_block != BlockType::Bedrock
                        && spawn_block == BlockType::Air
                } else {
                    // Surface/cave monster
                    if ground_block == BlockType::Air
                        || ground_block == BlockType::Water
                        || ground_block == BlockType::Lava
                        || ground_block == BlockType::Bedrock
                    {
                        return false;
                    }
                    if spawn_block != BlockType::Air || head_block != BlockType::Air {
                        return false;
                    }
                    // Light check for hostile natural spawns (block light <= 7)
                    if reason == SpawnReason::Natural {
                        let light = chunk_manager.get_block_light(bx, by, bz);
                        if light > 7 {
                            return false;
                        }
                    }
                    true
                }
            }
            MobCategory::Creature | MobCategory::Ambient => {
                if ground_block == BlockType::Air
                    || ground_block == BlockType::Water
                    || ground_block == BlockType::Lava
                {
                    return false;
                }
                spawn_block == BlockType::Air && head_block == BlockType::Air
            }
        }
    }

    pub fn evaluate_despawn(
        entity: &Entity,
        difficulty: Difficulty,
        player_positions: &[Vec3],
    ) -> bool {
        if entity.health <= 0.0 {
            return false;
        }

        // Hostile mobs in Peaceful mode despawn immediately
        if entity.entity_type.is_hostile() && difficulty == Difficulty::Peaceful {
            return true;
        }

        // Persistent exemptions: named, tamed, sitting, persistent flag, dropped items, boss
        if entity.entity_type.is_persistent()
            || entity.entity_type.is_boss()
            || entity.is_tamed()
            || entity.is_sitting()
            || entity.owner_id.is_some()
            || !entity.username.is_empty()
            || entity.is_persistent
        {
            return false;
        }

        if player_positions.is_empty() {
            return false;
        }

        let min_dist_sq = player_positions
            .iter()
            .map(|p| p.distance_squared(entity.position))
            .fold(f32::INFINITY, f32::min);

        // Despawn rule 1: > 128 blocks from any player -> instant despawn
        if min_dist_sq > 128.0 * 128.0 {
            return true;
        }

        // Despawn rule 2: > 32 blocks from player, lifetime > 30s
        if min_dist_sq > 32.0 * 32.0 && entity.life_time > 30.0 {
            // 1 in 800 chance per tick (~every 40 seconds on average)
            let pseudo_rand = ((entity.id ^ (entity.life_time as u64)) % 800) == 0;
            if pseudo_rand {
                return true;
            }
        }

        false
    }
}

pub fn mob_category_for_type(entity_type: EntityType) -> Option<MobCategory> {
    match entity_type {
        EntityType::Zombie
        | EntityType::Skeleton
        | EntityType::Creeper
        | EntityType::Blaze
        | EntityType::Piglin
        | EntityType::Husk
        | EntityType::Shulker
        | EntityType::Enderman
        | EntityType::EnderDragon
        | EntityType::Wither
        | EntityType::Spider
        | EntityType::Slime
        | EntityType::Witch
        | EntityType::Drowned
        | EntityType::Ghast
        | EntityType::MagmaCube
        | EntityType::WitherSkeleton => Some(MobCategory::Monster),

        EntityType::Pig
        | EntityType::Cow
        | EntityType::Sheep
        | EntityType::Chicken
        | EntityType::Wolf
        | EntityType::Cat
        | EntityType::Horse => Some(MobCategory::Creature),

        EntityType::Bat => Some(MobCategory::Ambient),

        EntityType::Squid => Some(MobCategory::WaterCreature),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mob_category_caps() {
        assert_eq!(MobCategory::Monster.cap_per_player(), 70);
        assert_eq!(MobCategory::Creature.cap_per_player(), 10);
        assert_eq!(MobCategory::Ambient.cap_per_player(), 15);
        assert_eq!(MobCategory::WaterCreature.cap_per_player(), 5);
    }

    #[test]
    fn test_peaceful_despawns_hostile() {
        let entity = Entity::new(1, EntityType::Zombie, Vec3::new(10.0, 64.0, 10.0));
        let player_pos = vec![Vec3::new(0.0, 64.0, 0.0)];

        assert!(SpawningSystem::evaluate_despawn(
            &entity,
            Difficulty::Peaceful,
            &player_pos
        ));
    }

    #[test]
    fn test_far_distance_despawns() {
        let entity = Entity::new(1, EntityType::Zombie, Vec3::new(200.0, 64.0, 200.0));
        let player_pos = vec![Vec3::new(0.0, 64.0, 0.0)];

        assert!(SpawningSystem::evaluate_despawn(
            &entity,
            Difficulty::Normal,
            &player_pos
        ));
    }

    #[test]
    fn test_persistent_entity_does_not_despawn() {
        let mut entity = Entity::new(1, EntityType::Wolf, Vec3::new(200.0, 64.0, 200.0));
        entity.is_tamed = true;
        let player_pos = vec![Vec3::new(0.0, 64.0, 0.0)];

        assert!(!SpawningSystem::evaluate_despawn(
            &entity,
            Difficulty::Normal,
            &player_pos
        ));
    }
}
