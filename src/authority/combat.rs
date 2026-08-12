//! Pure authoritative combat resolution.
//!
//! The network protocol never carries a damage amount. `AuthorityCore` derives
//! a `DamageEvent` from authenticated state and trusted world queries, then
//! calls one of the two clone/commit resolvers in this module.

use super::contract::{SessionGameplayState, SessionInventorySlot};
use crate::enchantment::Enchantment;
use crate::entity::EntityType;
use crate::inventory::{Item, ItemStack};
use crate::network::protocol::{ItemWire, PlayerId};
use crate::player::{calculate_damage_reduction, DamageSource};

pub const MELEE_REACH_MILLI: u32 = 4_000;
pub const DAMAGE_INVULNERABILITY_TICKS: u16 = 10;
pub const SHIELD_DISABLE_TICKS: u16 = 100;
pub const OFFHAND_SLOT: usize = 40;
pub const ARMOR_SLOT_RANGE: std::ops::Range<usize> = 36..40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CombatantId {
    Player(PlayerId),
    Entity(u64),
}

/// Inputs already derived or sampled by the authority. This type is not a
/// wire payload: in particular, clients cannot submit `base_damage_milli`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityDamageInput {
    pub event_id: u128,
    pub attacker: CombatantId,
    pub target: CombatantId,
    pub source: DamageSource,
    pub base_damage_milli: u32,
    pub attacker_position_milli: [i32; 3],
    pub target_position_milli: [i32; 3],
    pub attacker_look_milli: [i16; 3],
    pub target_look_milli: [i16; 3],
    pub cooldown_ready: bool,
    pub has_line_of_sight: bool,
    pub attacker_used_axe: bool,
    pub knockback_milli: u32,
    pub fire_ticks: u16,
    pub looting_level: u8,
}

/// Validated authority-internal damage event. Fields remain private so
/// transport code cannot accidentally copy an untrusted amount into combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageEvent {
    event_id: u128,
    attacker: CombatantId,
    target: CombatantId,
    source: DamageSource,
    base_damage_milli: u32,
    attacker_position_milli: [i32; 3],
    target_position_milli: [i32; 3],
    attacker_look_milli: [i16; 3],
    target_look_milli: [i16; 3],
    cooldown_ready: bool,
    has_line_of_sight: bool,
    attacker_used_axe: bool,
    knockback_milli: u32,
    fire_ticks: u16,
    looting_level: u8,
}

impl DamageEvent {
    /// Only crate-internal authority composition code can create an event.
    pub(super) fn from_authority(input: AuthorityDamageInput) -> Result<Self, CombatReject> {
        if input.event_id == 0
            || input.attacker == input.target
            || input.base_damage_milli == 0
            || input.base_damage_milli > 100_000
            || input.knockback_milli > 32_000
            || input.fire_ticks > 1_200
            || input.looting_level > 3
        {
            return Err(CombatReject::InvalidEvent);
        }
        validate_look(input.attacker_look_milli)?;
        validate_look(input.target_look_milli)?;
        Ok(Self {
            event_id: input.event_id,
            attacker: input.attacker,
            target: input.target,
            source: input.source,
            base_damage_milli: input.base_damage_milli,
            attacker_position_milli: input.attacker_position_milli,
            target_position_milli: input.target_position_milli,
            attacker_look_milli: input.attacker_look_milli,
            target_look_milli: input.target_look_milli,
            cooldown_ready: input.cooldown_ready,
            has_line_of_sight: input.has_line_of_sight,
            attacker_used_axe: input.attacker_used_axe,
            knockback_milli: input.knockback_milli,
            fire_ticks: input.fire_ticks,
            looting_level: input.looting_level,
        })
    }

    pub const fn event_id(&self) -> u128 {
        self.event_id
    }

    pub const fn attacker(&self) -> CombatantId {
        self.attacker
    }

    pub const fn target(&self) -> CombatantId {
        self.target
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerCombatSnapshot {
    pub player_id: PlayerId,
    pub gameplay: SessionGameplayState,
    pub velocity_milli: [i32; 3],
    pub last_applied_event: Option<u128>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityCombatSnapshot {
    pub entity_id: u64,
    pub entity_type: EntityType,
    pub health_milli: u32,
    pub max_health_milli: u32,
    pub velocity_milli: [i32; 3],
    pub armor_points_milli: u16,
    pub toughness_milli: u16,
    pub enchantment_protection_factor: u8,
    pub knockback_resistance_milli: u16,
    pub invulnerability_ticks: u16,
    pub fire_ticks_remaining: u16,
    pub has_wool: bool,
    pub death_source: Option<DamageSource>,
    pub death_settled: bool,
    pub last_applied_event: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathOutcome {
    pub source: DamageSource,
    /// World spawning is deliberately left to the authority integration layer.
    pub drops: Vec<SessionInventorySlot>,
    pub experience: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatOutcome {
    pub event_id: u128,
    pub attacker: CombatantId,
    pub target: CombatantId,
    pub raw_damage_milli: u32,
    pub applied_damage_milli: u32,
    pub remaining_health_milli: u32,
    pub blocked_by_shield: bool,
    pub shield_broke: bool,
    pub disable_shield_ticks: u16,
    pub knockback_delta_milli: [i32; 3],
    pub fire_ticks: u16,
    pub consume_attacker_cooldown: bool,
    pub death: Option<DeathOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatReject {
    InvalidEvent,
    IdentityMismatch,
    ReplayedEvent,
    Cooldown,
    OutOfRange,
    NoLineOfSight,
    NotFacingTarget,
    TargetDead,
    TargetInvulnerable,
    InvalidTargetState,
}

/// Resolve damage against a player snapshot. All validation and computation
/// happens on a clone; the original is replaced only after a complete success.
pub fn resolve_player_hit(
    event: &DamageEvent,
    target: &mut PlayerCombatSnapshot,
) -> Result<CombatOutcome, CombatReject> {
    validate_common(
        event,
        CombatantId::Player(target.player_id),
        target.last_applied_event,
    )?;
    if target.gameplay.is_dead || target.gameplay.health_milli == 0 {
        return Err(CombatReject::TargetDead);
    }
    if target.gameplay.max_health_milli == 0
        || target.gameplay.health_milli > target.gameplay.max_health_milli
    {
        return Err(CombatReject::InvalidTargetState);
    }
    if target.gameplay.invulnerability_ticks > 0 {
        return Err(CombatReject::TargetInvulnerable);
    }

    let mut candidate = target.clone();
    if shield_blocks(event, &candidate.gameplay) {
        let shield_broke = damage_shield(&mut candidate.gameplay, event.base_damage_milli);
        let disable_shield_ticks = if event.attacker_used_axe {
            candidate.gameplay.shield_active = false;
            candidate.gameplay.shield_cooldown_ticks = SHIELD_DISABLE_TICKS;
            SHIELD_DISABLE_TICKS
        } else {
            0
        };
        candidate.last_applied_event = Some(event.event_id);
        let outcome = CombatOutcome {
            event_id: event.event_id,
            attacker: event.attacker,
            target: event.target,
            raw_damage_milli: event.base_damage_milli,
            applied_damage_milli: 0,
            remaining_health_milli: candidate.gameplay.health_milli,
            blocked_by_shield: true,
            shield_broke,
            disable_shield_ticks,
            knockback_delta_milli: [0; 3],
            fire_ticks: 0,
            consume_attacker_cooldown: true,
            death: None,
        };
        *target = candidate;
        return Ok(outcome);
    }

    let defense = player_defense(&candidate.gameplay)?;
    let applied_damage_milli = reduced_damage(event, defense)
        .max(1)
        .min(candidate.gameplay.health_milli);
    let knockback = knockback_delta(event, defense.knockback_resistance_milli);
    candidate.gameplay.health_milli = candidate
        .gameplay
        .health_milli
        .saturating_sub(applied_damage_milli);
    candidate.gameplay.invulnerability_ticks = DAMAGE_INVULNERABILITY_TICKS;
    add_velocity(&mut candidate.velocity_milli, knockback);
    let death = if candidate.gameplay.health_milli == 0 {
        candidate.gameplay.is_dead = true;
        candidate.gameplay.death_source = Some(event.source.to_wire());
        candidate.gameplay.shield_active = false;
        Some(DeathOutcome {
            source: event.source,
            drops: candidate
                .gameplay
                .inventory
                .iter()
                .flatten()
                .copied()
                .collect(),
            experience: candidate
                .gameplay
                .experience_level
                .saturating_mul(7)
                .min(100),
        })
    } else {
        None
    };
    candidate.last_applied_event = Some(event.event_id);
    let outcome = CombatOutcome {
        event_id: event.event_id,
        attacker: event.attacker,
        target: event.target,
        raw_damage_milli: event.base_damage_milli,
        applied_damage_milli,
        remaining_health_milli: candidate.gameplay.health_milli,
        blocked_by_shield: false,
        shield_broke: false,
        disable_shield_ticks: 0,
        knockback_delta_milli: knockback,
        fire_ticks: event.fire_ticks,
        consume_attacker_cooldown: true,
        death,
    };
    *target = candidate;
    Ok(outcome)
}

/// Resolve damage against an entity snapshot with the same validation,
/// reduction, invulnerability and knockback semantics as player targets.
pub fn resolve_entity_hit(
    event: &DamageEvent,
    target: &mut EntityCombatSnapshot,
) -> Result<CombatOutcome, CombatReject> {
    validate_common(
        event,
        CombatantId::Entity(target.entity_id),
        target.last_applied_event,
    )?;
    if target.health_milli == 0
        || !(target.entity_type.is_living() || target.entity_type == EntityType::EndCrystal)
    {
        return Err(CombatReject::TargetDead);
    }
    if target.invulnerability_ticks > 0 {
        return Err(CombatReject::TargetInvulnerable);
    }
    validate_entity_defense(*target)?;

    let mut candidate = *target;
    let defense = DefenseProfile {
        armor_points_milli: candidate.armor_points_milli,
        toughness_milli: candidate.toughness_milli,
        enchantment_protection_factor: candidate.enchantment_protection_factor,
        knockback_resistance_milli: candidate.knockback_resistance_milli,
    };
    let applied_damage_milli = reduced_damage(event, defense)
        .max(1)
        .min(candidate.health_milli);
    let knockback = knockback_delta(event, defense.knockback_resistance_milli);
    candidate.health_milli = if candidate.entity_type == EntityType::EndCrystal {
        0
    } else {
        candidate.health_milli.saturating_sub(applied_damage_milli)
    };
    candidate.invulnerability_ticks = DAMAGE_INVULNERABILITY_TICKS;
    candidate.fire_ticks_remaining = candidate.fire_ticks_remaining.max(event.fire_ticks);
    add_velocity(&mut candidate.velocity_milli, knockback);
    let death = if candidate.health_milli == 0 && !candidate.death_settled {
        candidate.death_source = Some(event.source);
        candidate.death_settled = true;
        Some(entity_death_outcome(&candidate, event.looting_level))
    } else {
        None
    };
    candidate.last_applied_event = Some(event.event_id);
    let outcome = CombatOutcome {
        event_id: event.event_id,
        attacker: event.attacker,
        target: event.target,
        raw_damage_milli: event.base_damage_milli,
        applied_damage_milli,
        remaining_health_milli: candidate.health_milli,
        blocked_by_shield: false,
        shield_broke: false,
        disable_shield_ticks: 0,
        knockback_delta_milli: knockback,
        fire_ticks: event.fire_ticks,
        consume_attacker_cooldown: true,
        death,
    };
    *target = candidate;
    Ok(outcome)
}

#[derive(Debug, Clone, Copy, Default)]
struct DefenseProfile {
    armor_points_milli: u16,
    toughness_milli: u16,
    enchantment_protection_factor: u8,
    knockback_resistance_milli: u16,
}

fn validate_common(
    event: &DamageEvent,
    expected_target: CombatantId,
    last_applied_event: Option<u128>,
) -> Result<(), CombatReject> {
    if event.target != expected_target {
        return Err(CombatReject::IdentityMismatch);
    }
    if last_applied_event == Some(event.event_id) {
        return Err(CombatReject::ReplayedEvent);
    }
    if !event.cooldown_ready {
        return Err(CombatReject::Cooldown);
    }
    let delta = position_delta(event.attacker_position_milli, event.target_position_milli);
    if squared_length(delta) > u128::from(MELEE_REACH_MILLI).pow(2) {
        return Err(CombatReject::OutOfRange);
    }
    if !event.has_line_of_sight {
        return Err(CombatReject::NoLineOfSight);
    }
    let horizontal_distance_sq = i128::from(delta[0]).pow(2) + i128::from(delta[2]).pow(2);
    if horizontal_distance_sq > 1_000i128.pow(2)
        && horizontal_dot(event.attacker_look_milli, delta) <= 0
    {
        return Err(CombatReject::NotFacingTarget);
    }
    Ok(())
}

fn validate_entity_defense(target: EntityCombatSnapshot) -> Result<(), CombatReject> {
    if target.armor_points_milli > 30_000
        || target.toughness_milli > 20_000
        || target.enchantment_protection_factor > 20
        || target.knockback_resistance_milli > 1_000
        || target.health_milli > target.max_health_milli
    {
        return Err(CombatReject::InvalidTargetState);
    }
    Ok(())
}

fn player_defense(gameplay: &SessionGameplayState) -> Result<DefenseProfile, CombatReject> {
    let mut defense = DefenseProfile::default();
    for slot in gameplay.inventory[ARMOR_SLOT_RANGE].iter().flatten() {
        let Some(stack) = slot.item.to_stack() else {
            return Err(CombatReject::InvalidTargetState);
        };
        let Some(armor) = stack.item.armor_properties() else {
            continue;
        };
        defense.armor_points_milli = defense
            .armor_points_milli
            .saturating_add(quantize_nonnegative(armor.armor_points) as u16);
        defense.toughness_milli = defense
            .toughness_milli
            .saturating_add(quantize_nonnegative(armor.toughness) as u16);
        defense.knockback_resistance_milli = defense
            .knockback_resistance_milli
            .saturating_add(quantize_nonnegative(armor.knockback_resistance) as u16);
        defense.enchantment_protection_factor = defense
            .enchantment_protection_factor
            .saturating_add(stack.enchantments.level_of(Enchantment::Protection(1)));
    }
    defense.enchantment_protection_factor = defense.enchantment_protection_factor.min(20);
    defense.knockback_resistance_milli = defense.knockback_resistance_milli.min(1_000);
    Ok(defense)
}

fn reduced_damage(event: &DamageEvent, defense: DefenseProfile) -> u32 {
    quantize_nonnegative(calculate_damage_reduction(
        event.base_damage_milli as f32 / 1_000.0,
        event.source,
        defense.armor_points_milli as f32 / 1_000.0,
        defense.toughness_milli as f32 / 1_000.0,
        u32::from(defense.enchantment_protection_factor),
    ))
}

fn shield_blocks(event: &DamageEvent, gameplay: &SessionGameplayState) -> bool {
    if !gameplay.shield_active
        || gameplay.shield_cooldown_ticks > 0
        || active_shield_slot(gameplay).is_none()
        || matches!(
            event.source,
            DamageSource::Void | DamageSource::Hunger | DamageSource::Drowning | DamageSource::Fall
        )
    {
        return false;
    }
    let toward_attacker =
        position_delta(event.target_position_milli, event.attacker_position_milli);
    horizontal_dot(event.target_look_milli, toward_attacker) > 0
}

fn active_shield_slot(gameplay: &SessionGameplayState) -> Option<usize> {
    let selected = usize::from(gameplay.selected_hotbar_slot);
    [OFFHAND_SLOT, selected].into_iter().find(|index| {
        gameplay.inventory.get(*index).is_some_and(|slot| {
            slot.as_ref().is_some_and(|slot| {
                slot.item.item == Item::Shield.to_u32()
                    && slot.item.count == 1
                    && slot.item.durability > 0
            })
        })
    })
}

fn damage_shield(gameplay: &mut SessionGameplayState, raw_damage_milli: u32) -> bool {
    let Some(index) = active_shield_slot(gameplay) else {
        return false;
    };
    let loss = if raw_damage_milli > 3_000 {
        raw_damage_milli / 1_000 + 1
    } else {
        1
    };
    let slot = gameplay.inventory[index]
        .as_mut()
        .expect("active shield slot was validated");
    if u32::from(slot.item.durability) <= loss {
        gameplay.inventory[index] = None;
        gameplay.shield_active = false;
        true
    } else {
        slot.item.durability -= loss as u16;
        false
    }
}

fn entity_death_outcome(entity: &EntityCombatSnapshot, looting: u8) -> DeathOutcome {
    if !entity.entity_type.uses_standard_player_kill_rewards() {
        return DeathOutcome {
            source: entity.death_source.unwrap_or(DamageSource::Mob),
            drops: Vec::new(),
            experience: 0,
        };
    }
    let mut drops = Vec::new();
    for _ in 0..=(looting.min(3) / 2) {
        match entity.entity_type {
            EntityType::Zombie => push_drop(&mut drops, Item::RottenFlesh),
            EntityType::Skeleton => {
                push_drop(&mut drops, Item::Bone);
                push_drop(&mut drops, Item::Arrow);
            }
            EntityType::Creeper => push_drop(&mut drops, Item::Gunpowder),
            EntityType::Pig => push_drop(
                &mut drops,
                if entity.fire_ticks_remaining > 0 {
                    Item::CookedPorkchop
                } else {
                    Item::RawPorkchop
                },
            ),
            EntityType::Cow => push_drop(&mut drops, Item::RawBeef),
            EntityType::Sheep => {
                push_drop(&mut drops, Item::RawMutton);
                if entity.has_wool {
                    push_drop(&mut drops, Item::Wool);
                }
            }
            EntityType::Chicken => {
                push_drop(&mut drops, Item::RawChicken);
                push_drop(&mut drops, Item::Feather);
            }
            _ => {}
        }
    }
    let experience = match entity.entity_type {
        EntityType::Zombie | EntityType::Skeleton | EntityType::Creeper => 5,
        _ => 2,
    };
    DeathOutcome {
        source: entity.death_source.unwrap_or(DamageSource::Mob),
        drops,
        experience,
    }
}

fn push_drop(drops: &mut Vec<SessionInventorySlot>, item: Item) {
    drops.push(SessionInventorySlot::from_wire(
        ItemWire::from_stack(&ItemStack::new(item, 1)),
        0,
        0,
    ));
}

fn knockback_delta(event: &DamageEvent, resistance_milli: u16) -> [i32; 3] {
    let delta = position_delta(event.attacker_position_milli, event.target_position_milli);
    let horizontal_sq =
        u128::from(delta[0].unsigned_abs()).pow(2) + u128::from(delta[2].unsigned_abs()).pow(2);
    let length = integer_sqrt(horizontal_sq).max(1);
    let effective = u64::from(event.knockback_milli)
        * u64::from(1_000u16.saturating_sub(resistance_milli.min(1_000)))
        / 1_000;
    [
        clamp_i128_i32(i128::from(delta[0]) * i128::from(effective) / i128::from(length)),
        (3_000i64 * i64::try_from(effective).unwrap_or(i64::MAX) / 8_000)
            .clamp(0, i64::from(i32::MAX)) as i32,
        clamp_i128_i32(i128::from(delta[2]) * i128::from(effective) / i128::from(length)),
    ]
}

fn validate_look(look: [i16; 3]) -> Result<(), CombatReject> {
    let x = i64::from(look[0]);
    let y = i64::from(look[1]);
    let z = i64::from(look[2]);
    if !(250_000..=1_210_000).contains(&(x * x + y * y + z * z)) {
        return Err(CombatReject::InvalidEvent);
    }
    Ok(())
}

fn position_delta(from: [i32; 3], to: [i32; 3]) -> [i64; 3] {
    [
        i64::from(to[0]) - i64::from(from[0]),
        i64::from(to[1]) - i64::from(from[1]),
        i64::from(to[2]) - i64::from(from[2]),
    ]
}

fn squared_length(delta: [i64; 3]) -> u128 {
    delta
        .into_iter()
        .map(|component| u128::from(component.unsigned_abs()).pow(2))
        .sum()
}

fn horizontal_dot(look: [i16; 3], delta: [i64; 3]) -> i128 {
    i128::from(look[0]) * i128::from(delta[0]) + i128::from(look[2]) * i128::from(delta[2])
}

fn integer_sqrt(value: u128) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut current = value;
    let mut next = (current + 1) / 2;
    while next < current {
        current = next;
        next = (current + value / current) / 2;
    }
    current.min(u128::from(u64::MAX)) as u64
}

fn clamp_i128_i32(value: i128) -> i32 {
    value.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32
}

fn add_velocity(velocity: &mut [i32; 3], delta: [i32; 3]) {
    for axis in 0..3 {
        velocity[axis] = velocity[axis].saturating_add(delta[axis]);
    }
}

fn quantize_nonnegative(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        (value * 1_000.0).round().clamp(0.0, u32::MAX as f32) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enchantment::EnchantmentSet;

    fn event(event_id: u128, target: CombatantId) -> DamageEvent {
        DamageEvent::from_authority(AuthorityDamageInput {
            event_id,
            attacker: CombatantId::Player(1),
            target,
            source: DamageSource::Mob,
            base_damage_milli: 10_000,
            attacker_position_milli: [0, 64_000, 0],
            target_position_milli: [2_000, 64_000, 0],
            attacker_look_milli: [1_000, 0, 0],
            target_look_milli: [-1_000, 0, 0],
            cooldown_ready: true,
            has_line_of_sight: true,
            attacker_used_axe: false,
            knockback_milli: 8_000,
            fire_ticks: 0,
            looting_level: 0,
        })
        .expect("valid authority event")
    }

    fn player(id: PlayerId) -> PlayerCombatSnapshot {
        PlayerCombatSnapshot {
            player_id: id,
            gameplay: SessionGameplayState::default(),
            velocity_milli: [0; 3],
            last_applied_event: None,
        }
    }

    fn zombie(id: u64, health_milli: u32) -> EntityCombatSnapshot {
        EntityCombatSnapshot {
            entity_id: id,
            entity_type: EntityType::Zombie,
            health_milli,
            max_health_milli: 20_000,
            velocity_milli: [0; 3],
            armor_points_milli: 0,
            toughness_milli: 0,
            enchantment_protection_factor: 0,
            knockback_resistance_milli: 0,
            invulnerability_ticks: 0,
            fire_ticks_remaining: 0,
            has_wool: false,
            death_source: None,
            death_settled: false,
            last_applied_event: None,
        }
    }

    fn slot(stack: ItemStack) -> SessionInventorySlot {
        SessionInventorySlot::from_wire(ItemWire::from_stack(&stack), 0, 0)
    }

    #[test]
    fn cooldown_reject_keeps_player_snapshot_unchanged() {
        let mut target = player(2);
        let before = target.clone();
        let mut input = event(1, CombatantId::Player(2));
        input.cooldown_ready = false;
        assert_eq!(
            resolve_player_hit(&input, &mut target),
            Err(CombatReject::Cooldown)
        );
        assert_eq!(target, before);
    }

    #[test]
    fn range_and_los_reject_keep_entity_snapshot_unchanged() {
        let mut target = zombie(9, 20_000);
        let before = target;
        let mut too_far = event(2, CombatantId::Entity(9));
        too_far.target_position_milli[0] = 4_001;
        assert_eq!(
            resolve_entity_hit(&too_far, &mut target),
            Err(CombatReject::OutOfRange)
        );
        assert_eq!(target, before);
        let mut blocked = event(3, CombatantId::Entity(9));
        blocked.has_line_of_sight = false;
        assert_eq!(
            resolve_entity_hit(&blocked, &mut target),
            Err(CombatReject::NoLineOfSight)
        );
        assert_eq!(target, before);
    }

    #[test]
    fn armor_and_enchantments_reduce_player_damage() {
        let mut target = player(2);
        for (offset, item) in [
            Item::DiamondHelmet,
            Item::DiamondChestplate,
            Item::DiamondLeggings,
            Item::DiamondBoots,
        ]
        .into_iter()
        .enumerate()
        {
            let mut stack = ItemStack::new(item, 1);
            let mut enchantments = EnchantmentSet::default();
            enchantments.add_or_upgrade(Enchantment::Protection(1));
            stack.enchantments = enchantments;
            target.gameplay.inventory[ARMOR_SLOT_RANGE.start + offset] = Some(slot(stack));
        }
        let outcome = resolve_player_hit(&event(4, CombatantId::Player(2)), &mut target)
            .expect("armored hit");
        assert!(outcome.applied_damage_milli < outcome.raw_damage_milli);
        assert_eq!(
            target.gameplay.health_milli,
            20_000 - outcome.applied_damage_milli
        );
    }

    #[test]
    fn shield_blocks_only_from_front_and_loses_durability() {
        let mut target = player(2);
        target.gameplay.shield_active = true;
        target.gameplay.inventory[OFFHAND_SLOT] = Some(slot(ItemStack::new(Item::Shield, 1)));
        let durability = target.gameplay.inventory[OFFHAND_SLOT]
            .expect("shield")
            .item
            .durability;
        let outcome = resolve_player_hit(&event(5, CombatantId::Player(2)), &mut target)
            .expect("front block");
        assert!(outcome.blocked_by_shield);
        assert_eq!(target.gameplay.health_milli, 20_000);
        assert!(
            target.gameplay.inventory[OFFHAND_SLOT]
                .expect("shield")
                .item
                .durability
                < durability
        );

        let mut behind = player(3);
        behind.gameplay.shield_active = true;
        behind.gameplay.inventory[OFFHAND_SLOT] = Some(slot(ItemStack::new(Item::Shield, 1)));
        let mut hit = event(6, CombatantId::Player(3));
        hit.target_look_milli = [1_000, 0, 0];
        let outcome = resolve_player_hit(&hit, &mut behind).expect("rear hit");
        assert!(!outcome.blocked_by_shield);
        assert!(outcome.applied_damage_milli > 0);
    }

    #[test]
    fn invulnerability_is_identical_for_player_and_entity_and_never_mutates() {
        let mut player = player(2);
        player.gameplay.invulnerability_ticks = 1;
        let player_before = player.clone();
        assert_eq!(
            resolve_player_hit(&event(7, CombatantId::Player(2)), &mut player),
            Err(CombatReject::TargetInvulnerable)
        );
        assert_eq!(player, player_before);

        let mut entity = zombie(9, 20_000);
        entity.invulnerability_ticks = 1;
        let entity_before = entity;
        assert_eq!(
            resolve_entity_hit(&event(8, CombatantId::Entity(9)), &mut entity),
            Err(CombatReject::TargetInvulnerable)
        );
        assert_eq!(entity, entity_before);
    }

    #[test]
    fn lethal_entity_settlement_is_returned_exactly_once() {
        let mut target = zombie(9, 1_000);
        let hit = event(9, CombatantId::Entity(9));
        let outcome = resolve_entity_hit(&hit, &mut target).expect("lethal hit");
        let death = outcome.death.expect("one death settlement");
        assert_eq!(death.experience, 5);
        assert_eq!(death.drops.len(), 1);
        assert_eq!(death.drops[0].item.item, Item::RottenFlesh.to_u32());
        let settled = target;
        assert_eq!(
            resolve_entity_hit(&hit, &mut target),
            Err(CombatReject::ReplayedEvent)
        );
        assert_eq!(target, settled);
    }
}
