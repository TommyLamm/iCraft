use crate::inventory::{Item, ItemStack};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LootTableId {
    Dungeon,
    Mineshaft,
    Village,
    StrongholdCorridor,
    StrongholdLibrary,
    NetherBridge,
    EndCity,
}

impl LootTableId {
    pub fn as_str(&self) -> &'static str {
        match self {
            LootTableId::Dungeon => "chests/dungeon",
            LootTableId::Mineshaft => "chests/abandoned_mineshaft",
            LootTableId::Village => "chests/village/village_house",
            LootTableId::StrongholdCorridor => "chests/stronghold_corridor",
            LootTableId::StrongholdLibrary => "chests/stronghold_library",
            LootTableId::NetherBridge => "chests/nether_bridge",
            LootTableId::EndCity => "chests/end_city",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "chests/dungeon" => Some(LootTableId::Dungeon),
            "chests/abandoned_mineshaft" => Some(LootTableId::Mineshaft),
            "chests/village/village_house" | "chests/village" => Some(LootTableId::Village),
            "chests/stronghold_corridor" => Some(LootTableId::StrongholdCorridor),
            "chests/stronghold_library" => Some(LootTableId::StrongholdLibrary),
            "chests/nether_bridge" => Some(LootTableId::NetherBridge),
            "chests/end_city" => Some(LootTableId::EndCity),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LootEntry {
    pub item: Item,
    pub weight: u32,
    pub min_count: u32,
    pub max_count: u32,
    pub enchantment_chance: f32,
}

#[derive(Debug, Clone)]
pub struct LootPool {
    pub rolls_min: u32,
    pub rolls_max: u32,
    pub entries: Vec<LootEntry>,
}

#[derive(Debug, Clone)]
pub struct LootTable {
    pub pools: Vec<LootPool>,
}

pub fn get_loot_table(id: LootTableId) -> LootTable {
    match id {
        LootTableId::Dungeon => LootTable {
            pools: vec![LootPool {
                rolls_min: 1,
                rolls_max: 3,
                entries: vec![
                    LootEntry {
                        item: Item::Saddle,
                        weight: 20,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::IronIngot,
                        weight: 15,
                        min_count: 1,
                        max_count: 4,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::GoldIngot,
                        weight: 10,
                        min_count: 1,
                        max_count: 4,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Bread,
                        weight: 20,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::CoalOre,
                        weight: 15,
                        min_count: 1,
                        max_count: 4,
                        enchantment_chance: 0.0,
                    }, // placeholder coal item
                    LootEntry {
                        item: Item::RedstoneOre,
                        weight: 10,
                        min_count: 1,
                        max_count: 4,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::EnchantedBook,
                        weight: 5,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 1.0,
                    },
                    LootEntry {
                        item: Item::GoldenApple,
                        weight: 2,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 0.0,
                    },
                ],
            }],
        },
        LootTableId::Mineshaft => LootTable {
            pools: vec![LootPool {
                rolls_min: 2,
                rolls_max: 4,
                entries: vec![
                    LootEntry {
                        item: Item::IronIngot,
                        weight: 20,
                        min_count: 1,
                        max_count: 5,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::GoldIngot,
                        weight: 15,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::OakFence,
                        weight: 15,
                        min_count: 2,
                        max_count: 8,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Bread,
                        weight: 20,
                        min_count: 1,
                        max_count: 4,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::CoalOre,
                        weight: 20,
                        min_count: 2,
                        max_count: 6,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Diamond,
                        weight: 5,
                        min_count: 1,
                        max_count: 2,
                        enchantment_chance: 0.0,
                    },
                ],
            }],
        },
        LootTableId::Village => LootTable {
            pools: vec![LootPool {
                rolls_min: 3,
                rolls_max: 5,
                entries: vec![
                    LootEntry {
                        item: Item::Bread,
                        weight: 30,
                        min_count: 1,
                        max_count: 4,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Wheat,
                        weight: 25,
                        min_count: 1,
                        max_count: 5,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Apple,
                        weight: 20,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::IronIngot,
                        weight: 10,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Emerald,
                        weight: 8,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::IronPickaxe,
                        weight: 4,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 0.2,
                    },
                    LootEntry {
                        item: Item::IronChestplate,
                        weight: 3,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 0.2,
                    },
                ],
            }],
        },
        LootTableId::StrongholdCorridor => LootTable {
            pools: vec![LootPool {
                rolls_min: 2,
                rolls_max: 4,
                entries: vec![
                    LootEntry {
                        item: Item::EndPortalFrame,
                        weight: 5,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::EyeOfEnder,
                        weight: 15,
                        min_count: 1,
                        max_count: 2,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::IronIngot,
                        weight: 25,
                        min_count: 1,
                        max_count: 5,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::GoldIngot,
                        weight: 20,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::RedstoneOre,
                        weight: 15,
                        min_count: 2,
                        max_count: 6,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Bread,
                        weight: 15,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Diamond,
                        weight: 5,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                ],
            }],
        },
        LootTableId::StrongholdLibrary => LootTable {
            pools: vec![LootPool {
                rolls_min: 2,
                rolls_max: 5,
                entries: vec![
                    LootEntry {
                        item: Item::Book,
                        weight: 40,
                        min_count: 1,
                        max_count: 5,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::EnchantedBook,
                        weight: 30,
                        min_count: 1,
                        max_count: 2,
                        enchantment_chance: 1.0,
                    },
                    LootEntry {
                        item: Item::Paper,
                        weight: 20,
                        min_count: 2,
                        max_count: 7,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Compass,
                        weight: 10,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 0.0,
                    },
                ],
            }],
        },
        LootTableId::NetherBridge => LootTable {
            pools: vec![LootPool {
                rolls_min: 2,
                rolls_max: 4,
                entries: vec![
                    LootEntry {
                        item: Item::NetherWart,
                        weight: 25,
                        min_count: 1,
                        max_count: 4,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::BlazeRod,
                        weight: 20,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::GoldIngot,
                        weight: 25,
                        min_count: 1,
                        max_count: 6,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::IronIngot,
                        weight: 20,
                        min_count: 1,
                        max_count: 5,
                        enchantment_chance: 0.0,
                    },
                    LootEntry {
                        item: Item::Diamond,
                        weight: 10,
                        min_count: 1,
                        max_count: 3,
                        enchantment_chance: 0.0,
                    },
                ],
            }],
        },
        LootTableId::EndCity => LootTable {
            pools: vec![
                // Pool 1: Guaranteed Elytra (1 roll, 100% Elytra)
                LootPool {
                    rolls_min: 1,
                    rolls_max: 1,
                    entries: vec![LootEntry {
                        item: Item::Elytra,
                        weight: 100,
                        min_count: 1,
                        max_count: 1,
                        enchantment_chance: 0.0,
                    }],
                },
                // Pool 2: End City valuables
                LootPool {
                    rolls_min: 3,
                    rolls_max: 5,
                    entries: vec![
                        LootEntry {
                            item: Item::Diamond,
                            weight: 25,
                            min_count: 2,
                            max_count: 7,
                            enchantment_chance: 0.0,
                        },
                        LootEntry {
                            item: Item::GoldIngot,
                            weight: 25,
                            min_count: 3,
                            max_count: 9,
                            enchantment_chance: 0.0,
                        },
                        LootEntry {
                            item: Item::DiamondChestplate,
                            weight: 15,
                            min_count: 1,
                            max_count: 1,
                            enchantment_chance: 0.8,
                        },
                        LootEntry {
                            item: Item::DiamondSword,
                            weight: 15,
                            min_count: 1,
                            max_count: 1,
                            enchantment_chance: 0.8,
                        },
                        LootEntry {
                            item: Item::IronIngot,
                            weight: 20,
                            min_count: 4,
                            max_count: 10,
                            enchantment_chance: 0.0,
                        },
                    ],
                },
            ],
        },
    }
}

pub fn roll_loot_table(id: LootTableId, seed: u64) -> Vec<ItemStack> {
    let table = get_loot_table(id);
    let mut rng = SimpleLootRng::new(seed);
    let mut items = Vec::new();

    for pool in &table.pools {
        if pool.entries.is_empty() {
            continue;
        }
        let total_weight: u32 = pool.entries.iter().map(|e| e.weight).sum();
        if total_weight == 0 {
            continue;
        }

        let rolls = if pool.rolls_min >= pool.rolls_max {
            pool.rolls_min
        } else {
            pool.rolls_min + (rng.next_u32() % (pool.rolls_max - pool.rolls_min + 1))
        };

        // Cap rolls per pool to 32 to prevent unbounded generation
        let rolls = rolls.min(32);

        for _ in 0..rolls {
            let mut pick = rng.next_u32() % total_weight;
            for entry in &pool.entries {
                if pick < entry.weight {
                    let count = if entry.min_count >= entry.max_count {
                        entry.min_count
                    } else {
                        entry.min_count + (rng.next_u32() % (entry.max_count - entry.min_count + 1))
                    };
                    let count = count.clamp(1, 64) as u8;
                    let mut stack = ItemStack::new(entry.item, count.into());

                    if entry.enchantment_chance > 0.0 {
                        let roll_f = (rng.next_u32() % 1000) as f32 / 1000.0;
                        if roll_f < entry.enchantment_chance {
                            // Apply a basic enchantment for testing / loot progression
                            stack
                                .enchantments
                                .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(1));
                        }
                    }

                    items.push(stack);
                    break;
                } else {
                    pick -= entry.weight;
                }
            }
        }
    }

    items
}

struct SimpleLootRng {
    state: u64,
}

impl SimpleLootRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0xDEAD_BEEF } else { seed },
        }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loot_rolling_determinism() {
        let items1 = roll_loot_table(LootTableId::Dungeon, 12345);
        let items2 = roll_loot_table(LootTableId::Dungeon, 12345);
        assert_eq!(items1, items2);
        assert!(!items1.is_empty());

        let items3 = roll_loot_table(LootTableId::Dungeon, 54321);
        assert_ne!(items1, items3);
    }

    #[test]
    fn test_end_city_loot_contains_elytra() {
        let items = roll_loot_table(LootTableId::EndCity, 9999);
        assert!(items.iter().any(|st| st.item == Item::Elytra));
    }
}
