use super::*;
use crate::authority::combat as combat_logic;

impl AuthorityCore {
    pub(super) fn apply_authoritative_combat(
        &mut self,
        request: &GameplayRequest,
        session_id: PlayerId,
        target: u64,
        action: u8,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        use crate::authority::combat::{
            self, AuthorityDamageInput, CombatantId, DamageEvent, EntityCombatSnapshot,
            PlayerCombatSnapshot,
        };
        use crate::inventory::GameMode;
        use crate::player::DamageSource;

        if action != 0 || target == 0 || target == session_id {
            return Err(RejectReason::InvalidState);
        }
        let Some(dimension) = self
            .sessions
            .get(&session_id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return Err(RejectReason::Unauthorized);
        };
        let Some(attacker) = self.sessions.get(&session_id).map(|s| s.action_view()) else {
            return Err(RejectReason::Unauthorized);
        };
        if attacker.gameplay.is_dead {
            return Err(RejectReason::InvalidState);
        }
        let attacker_position_milli = position_to_milli(attacker.position)?;
        let attacker_look_milli = look_from_angles(attacker.yaw, attacker.pitch)?;
        let profile = combat_profile(&attacker.gameplay)?;
        let cooldown_ready =
            attacker.gameplay.attack_cooldown_ticks >= crate::authority::ATTACK_COOLDOWN_TICKS;

        if let Some(target_session) = self.sessions.get(&target).map(|s| s.action_view()) {
            if !self.world(dimension).rules.pvp
                || target_session.dimension != attacker.dimension
                || matches!(
                    target_session.game_mode,
                    GameMode::Creative | GameMode::Spectator
                )
            {
                return Err(RejectReason::PermissionDenied);
            }
            let target_position_milli = position_to_milli(target_session.position)?;
            let event = DamageEvent::from_authority(AuthorityDamageInput {
                event_id: request.request_id,
                attacker: CombatantId::Player(session_id),
                target: CombatantId::Player(target),
                source: DamageSource::Mob,
                base_damage_milli: profile.base_damage_milli,
                attacker_position_milli,
                target_position_milli,
                attacker_look_milli,
                target_look_milli: look_from_angles(target_session.yaw, target_session.pitch)?,
                cooldown_ready,
                has_line_of_sight: self
                    .world_mut_expect(dimension)
                    .has_line_of_sight(attacker.position, target_session.position),
                attacker_used_axe: profile.used_axe,
                knockback_milli: profile.knockback_milli,
                fire_ticks: profile.fire_ticks,
                looting_level: profile.looting_level,
            })?;
            let mut target_snapshot = PlayerCombatSnapshot {
                player_id: target,
                gameplay: target_session.gameplay,
                velocity_milli: target_session.gameplay.velocity_milli,
                last_applied_event: None,
            };
            let outcome = combat_logic::resolve_player_hit(&event, &mut target_snapshot)?;
            target_snapshot.gameplay.velocity_milli = target_snapshot.velocity_milli;
            let mut attacker_gameplay = attacker.gameplay;
            attacker_gameplay.attack_cooldown_ticks = 0;

            if outcome.death.is_some() && !self.world(dimension).rules.keep_inventory {
                target_snapshot.gameplay.inventory = [None; contract::SESSION_INVENTORY_SLOTS];
                target_snapshot.gameplay.experience = 0;
                target_snapshot.gameplay.experience_level = 0;
            }
            if outcome.death.is_some() {
                target_snapshot.gameplay.mounted_entity = None;
                self.world_mut_expect(dimension).remove_passenger(target);
            }
            self.sessions
                .get_mut(&session_id)
                .ok_or(RejectReason::Unauthorized)?
                .gameplay = attacker_gameplay;
            self.sessions
                .get_mut(&target)
                .ok_or(RejectReason::InvalidState)?
                .gameplay = target_snapshot.gameplay;
            self.pending_session_revisions.insert(target);
            if !self.world(dimension).rules.keep_inventory {
                if let Some(death) = outcome.death {
                    self.spawn_death_outcome(dimension, target_session.position, death);
                }
            }
            return Ok(None);
        }

        let Some(entity) = self.world(dimension).entities.get_by_id(target) else {
            return Err(RejectReason::InvalidState);
        };
        let target_entity_type = entity.entity_type;
        let target_position = entity.position.to_array();
        let target_bounds = entity.get_aabb();
        let attacker_position = glam::Vec3::from_array(attacker.position);
        let target_hit_position = attacker_position.clamp(target_bounds.min, target_bounds.max);
        let mut target_snapshot = EntityCombatSnapshot {
            entity_id: entity.id,
            entity_type: entity.entity_type,
            health_milli: quantize_health(entity.health),
            max_health_milli: quantize_health(entity.max_health),
            velocity_milli: position_to_milli(entity.velocity.to_array())?,
            armor_points_milli: 0,
            toughness_milli: 0,
            enchantment_protection_factor: 0,
            knockback_resistance_milli: 0,
            invulnerability_ticks: (entity.invulnerable_time.max(0.0) * 20.0)
                .round()
                .min(u16::MAX as f32) as u16,
            fire_ticks_remaining: (entity.fire_aspect_timer.max(0.0) * 20.0)
                .round()
                .min(u16::MAX as f32) as u16,
            has_wool: entity.has_wool,
            death_source: None,
            death_settled: entity.player_kill_rewarded,
            last_applied_event: None,
        };
        let event = DamageEvent::from_authority(AuthorityDamageInput {
            event_id: request.request_id,
            attacker: CombatantId::Player(session_id),
            target: CombatantId::Entity(target),
            source: DamageSource::Mob,
            base_damage_milli: profile.base_damage_milli,
            attacker_position_milli,
            target_position_milli: position_to_milli(target_hit_position.to_array())?,
            attacker_look_milli,
            target_look_milli: look_from_angles(entity.yaw, entity.pitch)?,
            cooldown_ready,
            has_line_of_sight: self
                .world_mut_expect(dimension)
                .has_line_of_sight(attacker.position, target_hit_position.to_array()),
            attacker_used_axe: profile.used_axe,
            knockback_milli: profile.knockback_milli,
            fire_ticks: profile.fire_ticks,
            looting_level: profile.looting_level,
        })?;
        let outcome =
            combat_logic::resolve_entity_hit(&event, &mut target_snapshot)?;

        let mut attacker_gameplay = attacker.gameplay;
        attacker_gameplay.attack_cooldown_ticks = 0;
        self.sessions
            .get_mut(&session_id)
            .ok_or(RejectReason::Unauthorized)?
            .gameplay = attacker_gameplay;
        if target_snapshot.health_milli == 0 {
            let _ = self.world_mut_expect(dimension).entities.remove_by_id(target);
            if target_entity_type == crate::entity::EntityType::EnderDragon {
                self.world_mut_expect(dimension).handle_dragon_completion();
            }
        } else if let Some(entity) = self.world_mut_expect(dimension).entities.get_by_id_mut(target) {
            entity.health = target_snapshot.health_milli as f32 / 1_000.0;
            entity.velocity = glam::Vec3::new(
                target_snapshot.velocity_milli[0] as f32 / 1_000.0,
                target_snapshot.velocity_milli[1] as f32 / 1_000.0,
                target_snapshot.velocity_milli[2] as f32 / 1_000.0,
            );
            entity.invulnerable_time = f32::from(target_snapshot.invulnerability_ticks) / 20.0;
            entity.fire_aspect_timer = f32::from(target_snapshot.fire_ticks_remaining) / 20.0;
            entity.player_kill_rewarded = target_snapshot.death_settled;
        }
        if let Some(death) = outcome.death {
            self.spawn_death_outcome(dimension, target_position, death);
        }
        Ok(None)
    }

    pub(super) fn spawn_death_outcome(
        &mut self,
        dimension: Dimension,
        position: [f32; 3],
        death: combat_logic::DeathOutcome,
    ) {
        for slot in death.drops {
            let id = self.next_unique_entity_id(dimension, None);
            self.claim_entity_id(id);
            let _ = self
                .world_mut_expect(dimension)
                .spawn_authority_drop(id, slot, position);
        }
        if death.experience > 0 {
            let id = self.next_unique_entity_id(dimension, None);
            self.claim_entity_id(id);
            let _ =
                self.world_mut_expect(dimension)
                    .spawn_authority_experience(id, death.experience, position);
        }
    }

}
