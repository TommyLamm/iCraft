use crate::inventory::{Item, ItemStack};
use crate::village::poi::VillagerProfession;
use std::collections::HashMap;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum VillagerLevel {
    Novice = 1,
    Apprentice = 2,
    Journeyman = 3,
    Expert = 4,
    Master = 5,
}

impl VillagerLevel {
    pub const fn from_u8(val: u8) -> Self {
        match val {
            2 => Self::Apprentice,
            3 => Self::Journeyman,
            4 => Self::Expert,
            5 => Self::Master,
            _ => Self::Novice,
        }
    }

    pub fn xp_threshold(self) -> u32 {
        match self {
            Self::Novice => 0,
            Self::Apprentice => 10,
            Self::Journeyman => 70,
            Self::Expert => 150,
            Self::Master => 250,
        }
    }

    pub fn next_level(self) -> Option<Self> {
        match self {
            Self::Novice => Some(Self::Apprentice),
            Self::Apprentice => Some(Self::Journeyman),
            Self::Journeyman => Some(Self::Expert),
            Self::Expert => Some(Self::Master),
            Self::Master => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TradeOffer {
    pub buy_a: ItemStack,
    pub buy_b: Option<ItemStack>,
    pub sell: ItemStack,
    pub uses: u32,
    pub max_uses: u32,
    pub xp_reward: u32,
    pub price_multiplier: f32,
}

impl TradeOffer {
    pub fn new(
        buy_a: ItemStack,
        buy_b: Option<ItemStack>,
        sell: ItemStack,
        max_uses: u32,
        xp_reward: u32,
    ) -> Self {
        Self {
            buy_a,
            buy_b,
            sell,
            uses: 0,
            max_uses,
            xp_reward,
            price_multiplier: 1.0,
        }
    }

    pub fn effective_cost_a(&self, discount: f32) -> u32 {
        if discount <= 0.0 {
            self.buy_a.count
        } else {
            let mult = (1.0 - discount).max(0.3);
            ((self.buy_a.count as f32) * mult).max(1.0).round() as u32
        }
    }

    pub fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }
}

pub fn generate_offers_for_level(
    profession: VillagerProfession,
    level: VillagerLevel,
) -> Vec<TradeOffer> {
    let mut offers = Vec::new();

    match profession {
        VillagerProfession::Farmer => match level {
            VillagerLevel::Novice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Wheat, 20),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    16,
                    2,
                ));
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 1),
                    None,
                    ItemStack::new(Item::Bread, 6),
                    16,
                    1,
                ));
            }
            VillagerLevel::Apprentice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Carrot, 15),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    16,
                    5,
                ));
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Potato, 15),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    16,
                    5,
                ));
            }
            VillagerLevel::Journeyman => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 1),
                    None,
                    ItemStack::new(Item::Pumpkin, 4),
                    12,
                    10,
                ));
            }
            VillagerLevel::Expert => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 3),
                    None,
                    ItemStack::new(Item::GoldenApple, 1),
                    12,
                    15,
                ));
            }
            VillagerLevel::Master => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 4),
                    None,
                    ItemStack::new(Item::GoldenCarrot, 3),
                    12,
                    20,
                ));
            }
        },
        VillagerProfession::Librarian => match level {
            VillagerLevel::Novice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Paper, 24),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    16,
                    2,
                ));
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 5),
                    Some(ItemStack::new(Item::Book, 1)),
                    ItemStack::new(Item::Book, 1),
                    12,
                    1,
                ));
            }
            VillagerLevel::Apprentice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Bookshelf, 4),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    12,
                    5,
                ));
            }
            VillagerLevel::Journeyman => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 1),
                    None,
                    ItemStack::new(Item::Glass, 4),
                    12,
                    10,
                ));
            }
            VillagerLevel::Expert => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 5),
                    None,
                    ItemStack::new(Item::Compass, 1),
                    12,
                    15,
                ));
            }
            VillagerLevel::Master => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 20),
                    None,
                    ItemStack::new(Item::Paper, 1),
                    12,
                    20,
                ));
            }
        },
        VillagerProfession::Armorer => match level {
            VillagerLevel::Novice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Coal, 15),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    16,
                    2,
                ));
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 5),
                    None,
                    ItemStack::new(Item::IronHelmet, 1),
                    12,
                    1,
                ));
            }
            VillagerLevel::Apprentice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::IronIngot, 4),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    12,
                    5,
                ));
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 9),
                    None,
                    ItemStack::new(Item::IronChestplate, 1),
                    12,
                    5,
                ));
            }
            VillagerLevel::Journeyman => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 7),
                    None,
                    ItemStack::new(Item::IronLeggings, 1),
                    12,
                    10,
                ));
            }
            VillagerLevel::Expert => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Diamond, 1),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    12,
                    15,
                ));
            }
            VillagerLevel::Master => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 8),
                    Some(ItemStack::new(Item::Diamond, 1)),
                    ItemStack::new(Item::DiamondChestplate, 1),
                    12,
                    20,
                ));
            }
        },
        VillagerProfession::Cleric => match level {
            VillagerLevel::Novice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::RottenFlesh, 32),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    16,
                    2,
                ));
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 1),
                    None,
                    ItemStack::new(Item::Redstone, 2),
                    12,
                    1,
                ));
            }
            VillagerLevel::Apprentice => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::GoldIngot, 3),
                    None,
                    ItemStack::new(Item::Emerald, 1),
                    12,
                    5,
                ));
            }
            VillagerLevel::Journeyman => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 4),
                    None,
                    ItemStack::new(Item::RedstoneDust, 1),
                    12,
                    10,
                ));
            }
            VillagerLevel::Expert => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 3),
                    None,
                    ItemStack::new(Item::GlassBottle, 1),
                    12,
                    15,
                ));
            }
            VillagerLevel::Master => {
                offers.push(TradeOffer::new(
                    ItemStack::new(Item::Emerald, 3),
                    None,
                    ItemStack::new(Item::NetherWart, 1),
                    12,
                    20,
                ));
            }
        },
        VillagerProfession::Unemployed => {}
    }

    offers
}

#[derive(Debug, Clone)]
pub struct ActiveMerchantSession {
    pub player_id: u64,
    pub villager_id: u64,
    pub villager_pos: glam::Vec3,
}

#[derive(Debug, Clone, Default)]
pub struct MerchantSessionManager {
    pub sessions: HashMap<u64, ActiveMerchantSession>,
}

impl MerchantSessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_session(&mut self, player_id: u64, villager_id: u64, villager_pos: glam::Vec3) {
        self.sessions.insert(
            player_id,
            ActiveMerchantSession {
                player_id,
                villager_id,
                villager_pos,
            },
        );
    }

    pub fn close_session(&mut self, player_id: u64) -> Option<ActiveMerchantSession> {
        self.sessions.remove(&player_id)
    }

    pub fn get_session(&self, player_id: u64) -> Option<&ActiveMerchantSession> {
        self.sessions.get(&player_id)
    }

    pub fn close_sessions_for_villager(&mut self, villager_id: u64) {
        self.sessions
            .retain(|_, session| session.villager_id != villager_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_villager_level_thresholds() {
        assert_eq!(VillagerLevel::Novice.xp_threshold(), 0);
        assert_eq!(VillagerLevel::Apprentice.xp_threshold(), 10);
        assert_eq!(VillagerLevel::Master.xp_threshold(), 250);
        assert_eq!(
            VillagerLevel::Novice.next_level(),
            Some(VillagerLevel::Apprentice)
        );
    }

    #[test]
    fn test_offer_generation() {
        let offers = generate_offers_for_level(VillagerProfession::Farmer, VillagerLevel::Novice);
        assert!(!offers.is_empty());
        assert_eq!(offers[0].buy_a.item, Item::Wheat);
        assert_eq!(offers[0].sell.item, Item::Emerald);
    }

    #[test]
    fn test_trade_discount() {
        let offer = TradeOffer::new(
            ItemStack::new(Item::Emerald, 10),
            None,
            ItemStack::new(Item::Bread, 1),
            12,
            1,
        );
        let cost = offer.effective_cost_a(0.3); // 30% discount
        assert_eq!(cost, 7);
    }
}
