use crate::inventory::ItemStack;

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
    #[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Item;

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
