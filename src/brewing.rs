use crate::inventory::{Item, ItemStack};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PotionKind {
    Water,
    Awkward,
    Speed,
    Strength,
    Healing,
    Regeneration,
    NightVision,
    Invisibility,
    FireResistance,
    WaterBreathing,
    Poison,
    Slowness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PotionData {
    pub kind: PotionKind,
    pub level: u8,
    pub duration_seconds: u16,
    pub splash: bool,
}

impl PotionData {
    pub const fn water() -> Self {
        Self {
            kind: PotionKind::Water,
            level: 1,
            duration_seconds: 0,
            splash: false,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self.kind {
            PotionKind::Water => "WATER BOTTLE",
            PotionKind::Awkward => "AWKWARD POTION",
            PotionKind::Speed => "POTION OF SPEED",
            PotionKind::Strength => "POTION OF STRENGTH",
            PotionKind::Healing => "POTION OF HEALING",
            PotionKind::Regeneration => "POTION OF REGENERATION",
            PotionKind::NightVision => "POTION OF NIGHT VISION",
            PotionKind::Invisibility => "POTION OF INVISIBILITY",
            PotionKind::FireResistance => "POTION OF FIRE RESISTANCE",
            PotionKind::WaterBreathing => "POTION OF WATER BREATHING",
            PotionKind::Poison => "POTION OF POISON",
            PotionKind::Slowness => "POTION OF SLOWNESS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PotionEffect {
    Speed { level: u8, duration: f32 },
    Strength { level: u8, duration: f32 },
    Healing { level: u8 },
    Regeneration { level: u8, duration: f32 },
    NightVision { duration: f32 },
    Invisibility { duration: f32 },
    FireResistance { duration: f32 },
    WaterBreathing { duration: f32 },
    Poison { level: u8, duration: f32 },
    Slowness { level: u8, duration: f32 },
}

impl PotionEffect {
    pub fn name(self) -> &'static str {
        match self {
            Self::Speed { .. } => "SPEED",
            Self::Strength { .. } => "STRENGTH",
            Self::Healing { .. } => "HEALING",
            Self::Regeneration { .. } => "REGENERATION",
            Self::NightVision { .. } => "NIGHT VISION",
            Self::Invisibility { .. } => "INVISIBILITY",
            Self::FireResistance { .. } => "FIRE RESISTANCE",
            Self::WaterBreathing { .. } => "WATER BREATHING",
            Self::Poison { .. } => "POISON",
            Self::Slowness { .. } => "SLOWNESS",
        }
    }

    pub fn remaining(self) -> f32 {
        match self {
            Self::Healing { .. } => 0.0,
            Self::Speed { duration, .. }
            | Self::Strength { duration, .. }
            | Self::Regeneration { duration, .. }
            | Self::NightVision { duration }
            | Self::Invisibility { duration }
            | Self::FireResistance { duration }
            | Self::WaterBreathing { duration }
            | Self::Poison { duration, .. }
            | Self::Slowness { duration, .. } => duration,
        }
    }

    fn ticked(self, dt: f32) -> Self {
        match self {
            Self::Speed { level, duration } => Self::Speed {
                level,
                duration: duration - dt,
            },
            Self::Strength { level, duration } => Self::Strength {
                level,
                duration: duration - dt,
            },
            Self::Regeneration { level, duration } => Self::Regeneration {
                level,
                duration: duration - dt,
            },
            Self::NightVision { duration } => Self::NightVision {
                duration: duration - dt,
            },
            Self::Invisibility { duration } => Self::Invisibility {
                duration: duration - dt,
            },
            Self::FireResistance { duration } => Self::FireResistance {
                duration: duration - dt,
            },
            Self::WaterBreathing { duration } => Self::WaterBreathing {
                duration: duration - dt,
            },
            Self::Poison { level, duration } => Self::Poison {
                level,
                duration: duration - dt,
            },
            Self::Slowness { level, duration } => Self::Slowness {
                level,
                duration: duration - dt,
            },
            healing => healing,
        }
    }
}

pub fn effect_from_potion(potion: PotionData) -> Option<PotionEffect> {
    let duration = potion.duration_seconds as f32;
    match potion.kind {
        PotionKind::Speed => Some(PotionEffect::Speed {
            level: potion.level,
            duration,
        }),
        PotionKind::Strength => Some(PotionEffect::Strength {
            level: potion.level,
            duration,
        }),
        PotionKind::Healing => Some(PotionEffect::Healing {
            level: potion.level,
        }),
        PotionKind::Regeneration => Some(PotionEffect::Regeneration {
            level: potion.level,
            duration,
        }),
        PotionKind::NightVision => Some(PotionEffect::NightVision { duration }),
        PotionKind::Invisibility => Some(PotionEffect::Invisibility { duration }),
        PotionKind::FireResistance => Some(PotionEffect::FireResistance { duration }),
        PotionKind::WaterBreathing => Some(PotionEffect::WaterBreathing { duration }),
        PotionKind::Poison => Some(PotionEffect::Poison {
            level: potion.level,
            duration,
        }),
        PotionKind::Slowness => Some(PotionEffect::Slowness {
            level: potion.level,
            duration,
        }),
        PotionKind::Water | PotionKind::Awkward => None,
    }
}

#[derive(Default)]
pub struct EffectManager {
    pub active: Vec<PotionEffect>,
    periodic_timer: f32,
}

impl EffectManager {
    pub fn apply(&mut self, potion: PotionData) -> f32 {
        let Some(effect) = effect_from_potion(potion) else {
            return 0.0;
        };
        if let PotionEffect::Healing { level } = effect {
            return 4.0 * level as f32;
        }
        let name = effect.name();
        self.active.retain(|current| current.name() != name);
        self.active.push(effect);
        0.0
    }

    pub fn update(&mut self, dt: f32) -> f32 {
        for effect in &mut self.active {
            *effect = effect.ticked(dt);
        }
        self.active.retain(|effect| effect.remaining() > 0.0);
        self.periodic_timer += dt;
        if self.periodic_timer < 1.0 {
            return 0.0;
        }
        self.periodic_timer -= 1.0;
        let regeneration = self
            .active
            .iter()
            .find_map(|effect| match effect {
                PotionEffect::Regeneration { level, .. } => Some(*level),
                _ => None,
            })
            .unwrap_or(0);
        let poison = self
            .active
            .iter()
            .find_map(|effect| match effect {
                PotionEffect::Poison { level, .. } => Some(*level),
                _ => None,
            })
            .unwrap_or(0);
        regeneration as f32 - poison as f32
    }

    pub fn speed_multiplier(&self) -> f32 {
        let speed = self
            .active
            .iter()
            .find_map(|effect| match effect {
                PotionEffect::Speed { level, .. } => Some(*level),
                _ => None,
            })
            .unwrap_or(0);
        let slow = self
            .active
            .iter()
            .find_map(|effect| match effect {
                PotionEffect::Slowness { level, .. } => Some(*level),
                _ => None,
            })
            .unwrap_or(0);
        (1.0 + speed as f32 * 0.2 - slow as f32 * 0.15).max(0.2)
    }

    pub fn strength_bonus(&self) -> f32 {
        self.active
            .iter()
            .find_map(|effect| match effect {
                PotionEffect::Strength { level, .. } => Some(*level as f32 * 3.0),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    pub fn has_invisibility(&self) -> bool {
        self.active
            .iter()
            .any(|e| matches!(e, PotionEffect::Invisibility { .. }))
    }
    pub fn has_fire_resistance(&self) -> bool {
        self.active
            .iter()
            .any(|e| matches!(e, PotionEffect::FireResistance { .. }))
    }
    pub fn has_water_breathing(&self) -> bool {
        self.active
            .iter()
            .any(|e| matches!(e, PotionEffect::WaterBreathing { .. }))
    }
    pub fn has_night_vision(&self) -> bool {
        self.active
            .iter()
            .any(|e| matches!(e, PotionEffect::NightVision { .. }))
    }
}

pub fn brew(potion: PotionData, ingredient: Item) -> Option<PotionData> {
    let mut result = potion;
    match (potion.kind, ingredient) {
        (PotionKind::Water, Item::NetherWart) => result.kind = PotionKind::Awkward,
        (PotionKind::Awkward, Item::Sugar) => {
            result.kind = PotionKind::Speed;
            result.duration_seconds = 180;
        }
        (PotionKind::Awkward, Item::BlazePowder) => {
            result.kind = PotionKind::Strength;
            result.duration_seconds = 180;
        }
        (PotionKind::Awkward, Item::GlisteringMelon) => {
            result.kind = PotionKind::Healing;
            result.duration_seconds = 0;
        }
        (PotionKind::Awkward, Item::GhastTear) => {
            result.kind = PotionKind::Regeneration;
            result.duration_seconds = 45;
        }
        (PotionKind::Awkward, Item::GoldenCarrot) => {
            result.kind = PotionKind::NightVision;
            result.duration_seconds = 180;
        }
        (PotionKind::NightVision, Item::FermentedSpiderEye) => {
            result.kind = PotionKind::Invisibility
        }
        (PotionKind::Awkward, Item::MagmaCream) => {
            result.kind = PotionKind::FireResistance;
            result.duration_seconds = 180;
        }
        (PotionKind::Awkward, Item::Pufferfish) => {
            result.kind = PotionKind::WaterBreathing;
            result.duration_seconds = 180;
        }
        (PotionKind::Awkward, Item::SpiderEye) => {
            result.kind = PotionKind::Poison;
            result.duration_seconds = 45;
        }
        (PotionKind::Speed, Item::FermentedSpiderEye) => result.kind = PotionKind::Slowness,
        (_, Item::RedstoneDust) if potion.kind != PotionKind::Healing => {
            result.duration_seconds = result.duration_seconds.saturating_mul(2)
        }
        (_, Item::GlowstoneDust)
            if !matches!(potion.kind, PotionKind::Water | PotionKind::Awkward) =>
        {
            result.level = 2;
            result.duration_seconds /= 2;
        }
        (_, Item::Gunpowder) if !matches!(potion.kind, PotionKind::Water | PotionKind::Awkward) => {
            result.splash = true
        }
        _ => return None,
    }
    Some(result)
}

#[derive(Default)]
pub struct BrewingStandState {
    pub bottles: [Option<ItemStack>; 3],
    pub ingredient: Option<ItemStack>,
    pub progress: f32,
}

impl BrewingStandState {
    pub fn can_brew(&self) -> bool {
        let Some(ingredient) = self.ingredient else {
            return false;
        };
        self.bottles.iter().flatten().any(|stack| {
            stack
                .potion
                .is_some_and(|potion| brew(potion, ingredient.item).is_some())
        })
    }

    pub fn update(&mut self, dt: f32) -> bool {
        if !self.can_brew() {
            self.progress = 0.0;
            return false;
        }
        self.progress += dt;
        if self.progress < 10.0 {
            return false;
        }
        let ingredient = self.ingredient.unwrap().item;
        for stack in self.bottles.iter_mut().flatten() {
            if let Some(next) = stack.potion.and_then(|potion| brew(potion, ingredient)) {
                stack.potion = Some(next);
                stack.item = if next.splash {
                    Item::SplashPotion
                } else {
                    Item::Potion
                };
            }
        }
        if let Some(stack) = &mut self.ingredient {
            if stack.count > 1 {
                stack.count -= 1;
            } else {
                self.ingredient = None;
            }
        }
        self.progress = 0.0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brewing_chain_and_modifiers_work() {
        let awkward = brew(PotionData::water(), Item::NetherWart).unwrap();
        let speed = brew(awkward, Item::Sugar).unwrap();
        assert_eq!(speed.kind, PotionKind::Speed);
        let strong = brew(speed, Item::GlowstoneDust).unwrap();
        assert_eq!(strong.level, 2);
        let splash = brew(strong, Item::Gunpowder).unwrap();
        assert!(splash.splash);
    }

    #[test]
    fn effects_tick_and_modify_stats() {
        let mut effects = EffectManager::default();
        effects.apply(PotionData {
            kind: PotionKind::Speed,
            level: 2,
            duration_seconds: 2,
            splash: false,
        });
        assert!((effects.speed_multiplier() - 1.4).abs() < 0.001);
        effects.update(2.1);
        assert_eq!(effects.speed_multiplier(), 1.0);
    }

    #[test]
    fn stand_finishes_after_ten_seconds_and_consumes_ingredient() {
        let mut stand = BrewingStandState::default();
        stand.bottles[0] = Some(ItemStack::new(Item::Potion, 1));
        stand.ingredient = Some(ItemStack::new(Item::NetherWart, 1));
        assert!(!stand.update(9.9));
        assert!(stand.update(0.1));
        assert_eq!(
            stand.bottles[0].unwrap().potion.unwrap().kind,
            PotionKind::Awkward
        );
        assert!(stand.ingredient.is_none());
    }
}
