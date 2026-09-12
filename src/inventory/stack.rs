use super::catalog::Item;
use crate::world::BlockType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GameMode {
    Creative,
    Survival,
    Adventure,
    Spectator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CreativeDragOrigin {
    Catalog,
    Inventory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ItemStack {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
    pub enchantments: crate::enchantment::EnchantmentSet,
    pub potion: Option<crate::brewing::PotionData>,
    pub custom_name: crate::enchantment::ItemName,
    /// Adventure-mode permission tags encoded as a compact block-id bitset.
    /// Keeping this copyable preserves the inventory's fixed-size slot arrays;
    /// serde defaults keep old saves compatible.  Use the helpers below rather
    /// than depending on the representation.
    #[serde(default)]
    pub can_break: u128,
    #[serde(default)]
    pub can_place_on: u128,
}

impl ItemStack {
    pub fn new(item: Item, count: u32) -> Self {
        let durability = item
            .tool_properties()
            .map(|t| t.durability)
            .or_else(|| item.armor_properties().map(|a| a.durability))
            .or_else(|| {
                match item {
                    Item::Shield => Some(336),
                    // Fishing is durability-gated by the authority domain. New
                    // rods therefore need a real remaining-durability value;
                    // zero is reserved for legacy/corrupt broken rods.
                    Item::FishingRod => Some(64),
                    _ => None,
                }
            })
            .unwrap_or(0);
        let potion = match item {
            Item::Potion => Some(crate::brewing::PotionData::water()),
            Item::SplashPotion => {
                let mut potion = crate::brewing::PotionData::water();
                potion.splash = true;
                Some(potion)
            }
            _ => None,
        };
        Self {
            item,
            count,
            durability,
            enchantments: crate::enchantment::EnchantmentSet::default(),
            potion,
            custom_name: crate::enchantment::ItemName::default(),
            can_break: 0,
            can_place_on: 0,
        }
    }

    pub fn can_merge_with(&self, other: &Self) -> bool {
        self.item == other.item
            && self.durability == other.durability
            && self.enchantments == other.enchantments
            && self.potion == other.potion
            && self.custom_name == other.custom_name
            && self.can_break == other.can_break
            && self.can_place_on == other.can_place_on
    }

    pub fn with_can_break(mut self, block: BlockType) -> Self {
        self.can_break |= 1u128 << (block as u8);
        self
    }

    pub fn with_can_place_on(mut self, block: BlockType) -> Self {
        self.can_place_on |= 1u128 << (block as u8);
        self
    }

    pub fn can_break_block(&self, block: BlockType) -> bool {
        self.can_break & (1u128 << (block as u8)) != 0
    }

    pub fn can_place_on_block(&self, block: BlockType) -> bool {
        self.can_place_on & (1u128 << (block as u8)) != 0
    }
}
