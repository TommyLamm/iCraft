use crate::inventory::{Item, ItemStack, ToolType};

pub const MAX_ENCHANTMENTS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Enchantment {
    Efficiency(u8),
    Unbreaking(u8),
    SilkTouch,
    Fortune(u8),
    Sharpness(u8),
    Knockback(u8),
    FireAspect(u8),
    Looting(u8),
    Protection(u8),
    FeatherFalling(u8),
    Respiration(u8),
    Power(u8),
    Infinity,
}

impl Enchantment {
    pub fn level(self) -> u8 {
        match self {
            Self::Efficiency(v)
            | Self::Unbreaking(v)
            | Self::Fortune(v)
            | Self::Sharpness(v)
            | Self::Knockback(v)
            | Self::FireAspect(v)
            | Self::Looting(v)
            | Self::Protection(v)
            | Self::FeatherFalling(v)
            | Self::Respiration(v)
            | Self::Power(v) => v,
            Self::SilkTouch | Self::Infinity => 1,
        }
    }

    fn kind(self) -> u8 {
        match self {
            Self::Efficiency(_) => 0,
            Self::Unbreaking(_) => 1,
            Self::SilkTouch => 2,
            Self::Fortune(_) => 3,
            Self::Sharpness(_) => 4,
            Self::Knockback(_) => 5,
            Self::FireAspect(_) => 6,
            Self::Looting(_) => 7,
            Self::Protection(_) => 8,
            Self::FeatherFalling(_) => 9,
            Self::Respiration(_) => 10,
            Self::Power(_) => 11,
            Self::Infinity => 12,
        }
    }

    fn with_level(self, level: u8) -> Self {
        match self {
            Self::Efficiency(_) => Self::Efficiency(level.min(5)),
            Self::Unbreaking(_) => Self::Unbreaking(level.min(3)),
            Self::Fortune(_) => Self::Fortune(level.min(3)),
            Self::Sharpness(_) => Self::Sharpness(level.min(5)),
            Self::Knockback(_) => Self::Knockback(level.min(2)),
            Self::FireAspect(_) => Self::FireAspect(level.min(2)),
            Self::Looting(_) => Self::Looting(level.min(3)),
            Self::Protection(_) => Self::Protection(level.min(4)),
            Self::FeatherFalling(_) => Self::FeatherFalling(level.min(4)),
            Self::Respiration(_) => Self::Respiration(level.min(3)),
            Self::Power(_) => Self::Power(level.min(5)),
            Self::SilkTouch => Self::SilkTouch,
            Self::Infinity => Self::Infinity,
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Efficiency(_) => "EFFICIENCY",
            Self::Unbreaking(_) => "UNBREAKING",
            Self::SilkTouch => "SILK TOUCH",
            Self::Fortune(_) => "FORTUNE",
            Self::Sharpness(_) => "SHARPNESS",
            Self::Knockback(_) => "KNOCKBACK",
            Self::FireAspect(_) => "FIRE ASPECT",
            Self::Looting(_) => "LOOTING",
            Self::Protection(_) => "PROTECTION",
            Self::FeatherFalling(_) => "FEATHER FALLING",
            Self::Respiration(_) => "RESPIRATION",
            Self::Power(_) => "POWER",
            Self::Infinity => "INFINITY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnchantmentSet {
    pub entries: [Option<Enchantment>; MAX_ENCHANTMENTS],
}

impl Default for EnchantmentSet {
    fn default() -> Self {
        Self {
            entries: [None; MAX_ENCHANTMENTS],
        }
    }
}

impl EnchantmentSet {
    pub fn is_empty(&self) -> bool {
        self.entries.iter().all(Option::is_none)
    }

    pub fn level_of(&self, kind: Enchantment) -> u8 {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.kind() == kind.kind())
            .map(|entry| entry.level())
            .unwrap_or(0)
    }

    pub fn add_or_upgrade(&mut self, enchantment: Enchantment) -> bool {
        if matches!(enchantment, Enchantment::Fortune(_))
            && self.level_of(Enchantment::SilkTouch) > 0
            || matches!(enchantment, Enchantment::SilkTouch)
                && self.level_of(Enchantment::Fortune(1)) > 0
        {
            return false;
        }
        if let Some(existing) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.kind() == enchantment.kind())
        {
            *existing = existing.with_level(existing.level().max(enchantment.level()));
            return true;
        }
        if let Some(empty) = self.entries.iter_mut().find(|entry| entry.is_none()) {
            *empty = Some(enchantment);
            return true;
        }
        false
    }

    pub fn merge(&mut self, other: &Self) {
        for enchantment in other.entries.iter().flatten().copied() {
            let next_level = self
                .level_of(enchantment)
                .max(enchantment.level())
                .saturating_add((self.level_of(enchantment) == enchantment.level()) as u8);
            self.add_or_upgrade(enchantment.with_level(next_level));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ItemName {
    bytes: [u8; 24],
    len: u8,
}

impl Default for ItemName {
    fn default() -> Self {
        Self {
            bytes: [0; 24],
            len: 0,
        }
    }
}

impl ItemName {
    pub fn set(&mut self, value: &str) {
        self.bytes = [0; 24];
        let clean = value.as_bytes();
        let len = clean.len().min(self.bytes.len());
        self.bytes[..len].copy_from_slice(&clean[..len]);
        self.len = len as u8;
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EnchantOption {
    pub cost: u8,
    pub lapis_cost: u8,
    pub enchantments: EnchantmentSet,
}

pub struct EnchantingState {
    pub input: Option<ItemStack>,
    pub lapis: Option<ItemStack>,
    pub options: [EnchantOption; 3],
    pub bookshelves: u8,
    pub seed: u32,
}

impl Default for EnchantingState {
    fn default() -> Self {
        Self {
            input: None,
            lapis: None,
            options: generate_options(Item::IronPickaxe, 0, 0),
            bookshelves: 0,
            seed: 0,
        }
    }
}

impl EnchantingState {
    pub fn refresh(&mut self) {
        if let Some(input) = self.input {
            self.options = generate_options(input.item, self.bookshelves, self.seed);
        }
    }
}

pub fn can_enchant(item: Item) -> bool {
    item.tool_properties().is_some() || item == Item::Bow || item.is_armor()
}

fn candidates(item: Item) -> &'static [Enchantment] {
    const TOOL: &[Enchantment] = &[
        Enchantment::Efficiency(1),
        Enchantment::Unbreaking(1),
        Enchantment::SilkTouch,
        Enchantment::Fortune(1),
    ];
    const SWORD: &[Enchantment] = &[
        Enchantment::Sharpness(1),
        Enchantment::Unbreaking(1),
        Enchantment::Knockback(1),
        Enchantment::FireAspect(1),
        Enchantment::Looting(1),
    ];
    const ARMOR: &[Enchantment] = &[
        Enchantment::Protection(1),
        Enchantment::Unbreaking(1),
        Enchantment::FeatherFalling(1),
        Enchantment::Respiration(1),
    ];
    const BOW: &[Enchantment] = &[
        Enchantment::Power(1),
        Enchantment::Unbreaking(1),
        Enchantment::Infinity,
    ];
    if item == Item::Bow {
        BOW
    } else if item.is_armor() {
        ARMOR
    } else if item
        .tool_properties()
        .is_some_and(|tool| tool.tool_type == ToolType::Sword)
    {
        SWORD
    } else {
        TOOL
    }
}

fn next_random(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *seed
}

pub fn generate_options(item: Item, bookshelves: u8, seed: u32) -> [EnchantOption; 3] {
    let shelves = bookshelves.min(15);
    let max_cost = (1 + shelves * 2).min(30);
    let mut result = [EnchantOption {
        cost: 1,
        lapis_cost: 1,
        enchantments: EnchantmentSet::default(),
    }; 3];
    let available = candidates(item);
    for (index, option) in result.iter_mut().enumerate() {
        let mut rng =
            seed ^ (item as u32).wrapping_mul(97) ^ (index as u32).wrapping_mul(0x9E37_79B9);
        let floor = match index {
            0 => 1,
            1 => (max_cost / 2).max(1),
            _ => max_cost.max(1),
        };
        let spread = (max_cost.saturating_sub(floor) + 1) as u32;
        option.cost = (floor as u32 + next_random(&mut rng) % spread).min(30) as u8;
        option.lapis_cost = index as u8 + 1;
        let enchantment_count = 1 + usize::from(option.cost >= 18);
        for _ in 0..enchantment_count {
            let base = available[next_random(&mut rng) as usize % available.len()];
            let max_level = match base {
                Enchantment::Efficiency(_) | Enchantment::Sharpness(_) | Enchantment::Power(_) => 5,
                Enchantment::Protection(_) | Enchantment::FeatherFalling(_) => 4,
                Enchantment::Unbreaking(_)
                | Enchantment::Fortune(_)
                | Enchantment::Looting(_)
                | Enchantment::Respiration(_) => 3,
                Enchantment::Knockback(_) | Enchantment::FireAspect(_) => 2,
                Enchantment::SilkTouch | Enchantment::Infinity => 1,
            };
            let level = (1 + option.cost / 7).clamp(1, max_level);
            option.enchantments.add_or_upgrade(base.with_level(level));
        }
    }
    result
}

pub fn mining_speed_multiplier(set: &EnchantmentSet) -> f32 {
    let level = set.level_of(Enchantment::Efficiency(1)) as f32;
    1.0 + level * level * 0.18
}

pub fn attack_damage_bonus(set: &EnchantmentSet) -> f32 {
    let level = set.level_of(Enchantment::Sharpness(1));
    if level == 0 {
        0.0
    } else {
        0.5 + level as f32 * 0.5
    }
}

pub fn protection_multiplier(armor: &[Option<ItemStack>; 4], is_fall: bool) -> f32 {
    let points: u8 = armor
        .iter()
        .flatten()
        .map(|stack| {
            stack.enchantments.level_of(Enchantment::Protection(1))
                + if is_fall {
                    stack.enchantments.level_of(Enchantment::FeatherFalling(1)) * 2
                } else {
                    0
                }
        })
        .sum();
    (1.0 - points.min(20) as f32 * 0.04).max(0.2)
}

pub fn should_consume_durability(set: &EnchantmentSet, seed: u32) -> bool {
    let level = set.level_of(Enchantment::Unbreaking(1)) as u32;
    level == 0 || seed.wrapping_mul(1_103_515_245).wrapping_add(12_345) % (level + 1) == 0
}

#[derive(Default)]
pub struct AnvilState {
    pub left: Option<ItemStack>,
    pub right: Option<ItemStack>,
    pub output: Option<ItemStack>,
    pub rename: String,
    pub cost: u8,
}

impl AnvilState {
    pub fn refresh(&mut self) {
        self.output = self.left.map(|mut left| {
            let mut cost = 0;
            if let Some(right) = self.right {
                if right.item == left.item {
                    let max = left
                        .item
                        .tool_properties()
                        .map(|tool| tool.durability)
                        .unwrap_or(0);
                    if max > 0 {
                        left.durability = (left.durability + right.durability + max / 20).min(max);
                        cost += 2;
                    }
                }
                left.enchantments.merge(&right.enchantments);
                cost += right.enchantments.entries.iter().flatten().count() as u8;
            }
            if !self.rename.trim().is_empty() {
                left.custom_name.set(self.rename.trim());
                cost += 1;
            }
            self.cost = cost.max(1);
            left
        });
        if self.left.is_none() {
            self.cost = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookshelf_options_scale_and_are_deterministic() {
        let low = generate_options(Item::DiamondPickaxe, 0, 42);
        let high = generate_options(Item::DiamondPickaxe, 15, 42);
        assert_eq!(
            low[0].cost,
            generate_options(Item::DiamondPickaxe, 0, 42)[0].cost
        );
        assert!(high[2].cost > low[2].cost);
        assert!(!high[2].enchantments.is_empty());
    }

    #[test]
    fn silk_touch_and_fortune_conflict() {
        let mut set = EnchantmentSet::default();
        assert!(set.add_or_upgrade(Enchantment::SilkTouch));
        assert!(!set.add_or_upgrade(Enchantment::Fortune(3)));
    }

    #[test]
    fn anvil_repairs_and_combines() {
        let mut left = ItemStack::new(Item::IronPickaxe, 1);
        left.durability = 10;
        left.enchantments.add_or_upgrade(Enchantment::Efficiency(2));
        let mut right = ItemStack::new(Item::IronPickaxe, 1);
        right.durability = 20;
        right
            .enchantments
            .add_or_upgrade(Enchantment::Efficiency(2));
        let mut anvil = AnvilState {
            left: Some(left),
            right: Some(right),
            rename: "Miner".into(),
            ..Default::default()
        };
        anvil.refresh();
        let output = anvil.output.unwrap();
        assert!(output.durability > 30);
        assert_eq!(output.enchantments.level_of(Enchantment::Efficiency(1)), 3);
        assert_eq!(output.custom_name.as_str(), "Miner");
    }

    #[test]
    fn efficiency_and_protection_change_gameplay_values() {
        let mut enchantments = EnchantmentSet::default();
        enchantments.add_or_upgrade(Enchantment::Efficiency(4));
        assert!(mining_speed_multiplier(&enchantments) > 3.0);

        let mut chestplate = ItemStack::new(Item::IronChestplate, 1);
        chestplate
            .enchantments
            .add_or_upgrade(Enchantment::Protection(4));
        let armor = [None, Some(chestplate), None, None];
        assert!(protection_multiplier(&armor, false) < 1.0);
    }
}
