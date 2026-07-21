use crate::dimension::Dimension;
use crate::entity::EntityType;
use crate::inventory::Item;
use crate::world::BlockType;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdvancementCategory {
    Minecraft,
    Nether,
    TheEnd,
    Adventure,
    Husbandry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvancementFrameType {
    Task,
    Goal,
    Challenge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvancementTrigger {
    ObtainItem(Item),
    CraftItem(Item),
    MineBlock(BlockType),
    KillMob(EntityType),
    EnterDimension(Dimension),
    BrewPotion,
    EnchantItem,
    EatFood(Item),
    BreedAnimals,
    Root,
}

#[derive(Debug, Clone)]
pub struct Advancement {
    pub id: &'static str,
    pub category: AdvancementCategory,
    pub title: &'static str,
    pub description: &'static str,
    pub icon_item: Item,
    pub frame: AdvancementFrameType,
    pub parent: Option<&'static str>,
    pub trigger: AdvancementTrigger,
    pub xp_reward: u32,
    pub x_pos: f32,
    pub y_pos: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AdvancementProgressData {
    pub completed_ids: HashSet<String>,
    pub criteria_progress: HashMap<String, u32>,
}

#[derive(Debug, Clone)]
pub struct ToastNotification {
    pub title: String,
    pub description: String,
    pub frame: AdvancementFrameType,
    pub icon_item: Item,
    pub timer: f32, // Total 3.0s (0.4s slide-in, 2.2s display, 0.4s slide-out)
}

pub struct AdvancementTree {
    pub list: Vec<Advancement>,
    pub map: HashMap<&'static str, usize>,
}

impl AdvancementTree {
    pub fn new() -> Self {
        let mut tree = Self {
            list: Vec::new(),
            map: HashMap::new(),
        };
        tree.register_all();
        tree
    }

    fn add(&mut self, adv: Advancement) {
        let idx = self.list.len();
        self.map.insert(adv.id, idx);
        self.list.push(adv);
    }

    pub fn get(&self, id: &str) -> Option<&Advancement> {
        self.map.get(id).map(|&idx| &self.list[idx])
    }

    pub fn get_category_advancements(&self, category: AdvancementCategory) -> Vec<&Advancement> {
        self.list
            .iter()
            .filter(|a| a.category == category)
            .collect()
    }

    fn register_all(&mut self) {
        // 50 advancements across 5 categories
        // Category 1: Minecraft (Story - 10)
        self.add(Advancement {
            id: "minecraft:root",
            category: AdvancementCategory::Minecraft,
            title: "Minecraft",
            description: "The heart and story of the game - Mine wood",
            icon_item: Item::OakLog,
            frame: AdvancementFrameType::Task,
            parent: None,
            trigger: AdvancementTrigger::MineBlock(BlockType::OakLog),
            xp_reward: 0,
            x_pos: 0.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "minecraft:stone_age",
            category: AdvancementCategory::Minecraft,
            title: "Stone Age",
            description: "Mine new stone with your new pickaxe",
            icon_item: Item::StonePickaxe,
            frame: AdvancementFrameType::Task,
            parent: Some("minecraft:root"),
            trigger: AdvancementTrigger::CraftItem(Item::StonePickaxe),
            xp_reward: 10,
            x_pos: 1.5,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "minecraft:getting_hardware",
            category: AdvancementCategory::Minecraft,
            title: "Getting Hardware",
            description: "Smelt or acquire an iron ingot",
            icon_item: Item::IronIngot,
            frame: AdvancementFrameType::Task,
            parent: Some("minecraft:stone_age"),
            trigger: AdvancementTrigger::ObtainItem(Item::IronIngot),
            xp_reward: 15,
            x_pos: 3.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "minecraft:suit_up",
            category: AdvancementCategory::Minecraft,
            title: "Suit Up",
            description: "Protect yourself with a piece of iron armor",
            icon_item: Item::IronChestplate,
            frame: AdvancementFrameType::Task,
            parent: Some("minecraft:getting_hardware"),
            trigger: AdvancementTrigger::ObtainItem(Item::IronChestplate),
            xp_reward: 20,
            x_pos: 4.5,
            y_pos: -1.0,
        });
        self.add(Advancement {
            id: "minecraft:hot_stuff",
            category: AdvancementCategory::Minecraft,
            title: "Hot Stuff",
            description: "Fill a bucket with lava",
            icon_item: Item::Lava,
            frame: AdvancementFrameType::Task,
            parent: Some("minecraft:getting_hardware"),
            trigger: AdvancementTrigger::ObtainItem(Item::Lava),
            xp_reward: 20,
            x_pos: 4.5,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "minecraft:isn_it_iron_pick",
            category: AdvancementCategory::Minecraft,
            title: "Isn't It Iron Pick",
            description: "Upgrade your pickaxe to iron",
            icon_item: Item::IronPickaxe,
            frame: AdvancementFrameType::Task,
            parent: Some("minecraft:getting_hardware"),
            trigger: AdvancementTrigger::CraftItem(Item::IronPickaxe),
            xp_reward: 20,
            x_pos: 4.5,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "minecraft:not_today_thank_you",
            category: AdvancementCategory::Minecraft,
            title: "Not Today, Thank You",
            description: "Equip protective iron helmet",
            icon_item: Item::IronHelmet,
            frame: AdvancementFrameType::Task,
            parent: Some("minecraft:suit_up"),
            trigger: AdvancementTrigger::ObtainItem(Item::IronHelmet),
            xp_reward: 25,
            x_pos: 6.0,
            y_pos: -1.0,
        });
        self.add(Advancement {
            id: "minecraft:ice_bucket_challenge",
            category: AdvancementCategory::Minecraft,
            title: "Ice Bucket Challenge",
            description: "Obtain a block of obsidian",
            icon_item: Item::Obsidian,
            frame: AdvancementFrameType::Goal,
            parent: Some("minecraft:isn_it_iron_pick"),
            trigger: AdvancementTrigger::ObtainItem(Item::Obsidian),
            xp_reward: 35,
            x_pos: 6.0,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "minecraft:diamonds",
            category: AdvancementCategory::Minecraft,
            title: "Diamonds!",
            description: "Acquire diamonds",
            icon_item: Item::Diamond,
            frame: AdvancementFrameType::Goal,
            parent: Some("minecraft:isn_it_iron_pick"),
            trigger: AdvancementTrigger::ObtainItem(Item::Diamond),
            xp_reward: 50,
            x_pos: 6.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "minecraft:cover_me_with_diamonds",
            category: AdvancementCategory::Minecraft,
            title: "Cover Me with Diamonds",
            description: "Craft a diamond pickaxe or weapon",
            icon_item: Item::Diamond,
            frame: AdvancementFrameType::Challenge,
            parent: Some("minecraft:diamonds"),
            trigger: AdvancementTrigger::CraftItem(Item::DiamondPickaxe),
            xp_reward: 100,
            x_pos: 7.5,
            y_pos: 0.0,
        });

        // Category 2: Nether (10)
        self.add(Advancement {
            id: "nether:root",
            category: AdvancementCategory::Nether,
            title: "Nether",
            description: "Bring summer clothes - Enter the Nether",
            icon_item: Item::Netherrack,
            frame: AdvancementFrameType::Task,
            parent: None,
            trigger: AdvancementTrigger::EnterDimension(Dimension::Nether),
            xp_reward: 0,
            x_pos: 0.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "nether:into_fire",
            category: AdvancementCategory::Nether,
            title: "Into Fire",
            description: "Relieve a Blaze of its rod",
            icon_item: Item::BlazeRod,
            frame: AdvancementFrameType::Task,
            parent: Some("nether:root"),
            trigger: AdvancementTrigger::ObtainItem(Item::BlazeRod),
            xp_reward: 25,
            x_pos: 1.5,
            y_pos: -0.5,
        });
        self.add(Advancement {
            id: "nether:local_brewery",
            category: AdvancementCategory::Nether,
            title: "Local Brewery",
            description: "Brew a potion at a brewing stand",
            icon_item: Item::BrewingStand,
            frame: AdvancementFrameType::Task,
            parent: Some("nether:into_fire"),
            trigger: AdvancementTrigger::BrewPotion,
            xp_reward: 30,
            x_pos: 3.0,
            y_pos: -0.5,
        });
        self.add(Advancement {
            id: "nether:withering_heights",
            category: AdvancementCategory::Nether,
            title: "Withering Heights",
            description: "Summon the Wither",
            icon_item: Item::WitherSkeletonSkull,
            frame: AdvancementFrameType::Goal,
            parent: Some("nether:root"),
            trigger: AdvancementTrigger::KillMob(EntityType::Wither),
            xp_reward: 50,
            x_pos: 1.5,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "nether:bring_home_the_beacon",
            category: AdvancementCategory::Nether,
            title: "Bring Home the Beacon",
            description: "Obtain a Nether Star",
            icon_item: Item::NetherStar,
            frame: AdvancementFrameType::Challenge,
            parent: Some("nether:withering_heights"),
            trigger: AdvancementTrigger::ObtainItem(Item::NetherStar),
            xp_reward: 100,
            x_pos: 3.0,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "nether:spooky_scary_skeleton",
            category: AdvancementCategory::Nether,
            title: "Spooky Scary Skeleton",
            description: "Obtain a Wither Skeleton Skull",
            icon_item: Item::WitherSkeletonSkull,
            frame: AdvancementFrameType::Task,
            parent: Some("nether:root"),
            trigger: AdvancementTrigger::ObtainItem(Item::WitherSkeletonSkull),
            xp_reward: 30,
            x_pos: 1.5,
            y_pos: 2.0,
        });
        self.add(Advancement {
            id: "nether:return_to_sender",
            category: AdvancementCategory::Nether,
            title: "Return to Sender",
            description: "Destroy a hostile Piglin mob",
            icon_item: Item::GhastTear,
            frame: AdvancementFrameType::Challenge,
            parent: Some("nether:into_fire"),
            trigger: AdvancementTrigger::KillMob(EntityType::Piglin),
            xp_reward: 50,
            x_pos: 3.0,
            y_pos: -1.5,
        });
        self.add(Advancement {
            id: "nether:subspace_bubble",
            category: AdvancementCategory::Nether,
            title: "Subspace Bubble",
            description: "Use the Nether portal to travel",
            icon_item: Item::FlintAndSteel,
            frame: AdvancementFrameType::Challenge,
            parent: Some("nether:root"),
            trigger: AdvancementTrigger::EnterDimension(Dimension::Overworld),
            xp_reward: 100,
            x_pos: 1.5,
            y_pos: -2.0,
        });
        self.add(Advancement {
            id: "nether:uneasy_alliance",
            category: AdvancementCategory::Nether,
            title: "Uneasy Alliance",
            description: "Defeat a Blaze in combat",
            icon_item: Item::GhastTear,
            frame: AdvancementFrameType::Challenge,
            parent: Some("nether:return_to_sender"),
            trigger: AdvancementTrigger::KillMob(EntityType::Blaze),
            xp_reward: 100,
            x_pos: 4.5,
            y_pos: -1.5,
        });
        self.add(Advancement {
            id: "nether:a_furious_cocktail",
            category: AdvancementCategory::Nether,
            title: "A Furious Cocktail",
            description: "Enchant an item with magical power",
            icon_item: Item::Potion,
            frame: AdvancementFrameType::Challenge,
            parent: Some("nether:local_brewery"),
            trigger: AdvancementTrigger::EnchantItem,
            xp_reward: 100,
            x_pos: 4.5,
            y_pos: -0.5,
        });

        // Category 3: The End (10)
        self.add(Advancement {
            id: "end:root",
            category: AdvancementCategory::TheEnd,
            title: "The End?",
            description: "Or the beginning? Enter the End dimension",
            icon_item: Item::EndPortalFrame,
            frame: AdvancementFrameType::Task,
            parent: None,
            trigger: AdvancementTrigger::EnterDimension(Dimension::End),
            xp_reward: 0,
            x_pos: 0.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "end:kill_dragon",
            category: AdvancementCategory::TheEnd,
            title: "Free the End",
            description: "Good luck! Slay the Ender Dragon",
            icon_item: Item::DragonEgg,
            frame: AdvancementFrameType::Challenge,
            parent: Some("end:root"),
            trigger: AdvancementTrigger::KillMob(EntityType::EnderDragon),
            xp_reward: 200,
            x_pos: 1.5,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "end:dragon_egg",
            category: AdvancementCategory::TheEnd,
            title: "Next Generation",
            description: "Hold the Dragon Egg",
            icon_item: Item::DragonEgg,
            frame: AdvancementFrameType::Goal,
            parent: Some("end:kill_dragon"),
            trigger: AdvancementTrigger::ObtainItem(Item::DragonEgg),
            xp_reward: 50,
            x_pos: 3.0,
            y_pos: -1.0,
        });
        self.add(Advancement {
            id: "end:enter_end_city",
            category: AdvancementCategory::TheEnd,
            title: "The City at the End of the Game",
            description: "Go on in! Discover an End City structure",
            icon_item: Item::Purpur,
            frame: AdvancementFrameType::Task,
            parent: Some("end:kill_dragon"),
            trigger: AdvancementTrigger::ObtainItem(Item::Purpur),
            xp_reward: 30,
            x_pos: 3.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "end:elytra",
            category: AdvancementCategory::TheEnd,
            title: "Sky's the Limit",
            description: "Find an Elytra",
            icon_item: Item::Elytra,
            frame: AdvancementFrameType::Goal,
            parent: Some("end:enter_end_city"),
            trigger: AdvancementTrigger::ObtainItem(Item::Elytra),
            xp_reward: 50,
            x_pos: 4.5,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "end:great_escape",
            category: AdvancementCategory::TheEnd,
            title: "Great Escape",
            description: "Mine End Stone to escape",
            icon_item: Item::EndStone,
            frame: AdvancementFrameType::Task,
            parent: Some("end:kill_dragon"),
            trigger: AdvancementTrigger::MineBlock(BlockType::EndStone),
            xp_reward: 30,
            x_pos: 3.0,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "end:you_need_a_mint",
            category: AdvancementCategory::TheEnd,
            title: "You Need a Mint",
            description: "Collect Dragon's Breath in a glass bottle",
            icon_item: Item::GlassBottle,
            frame: AdvancementFrameType::Goal,
            parent: Some("end:kill_dragon"),
            trigger: AdvancementTrigger::ObtainItem(Item::GlassBottle),
            xp_reward: 40,
            x_pos: 3.0,
            y_pos: 2.0,
        });
        self.add(Advancement {
            id: "end:shulker_box",
            category: AdvancementCategory::TheEnd,
            title: "Remote Storage",
            description: "Obtain a Shulker Shell",
            icon_item: Item::ShulkerShell,
            frame: AdvancementFrameType::Task,
            parent: Some("end:enter_end_city"),
            trigger: AdvancementTrigger::ObtainItem(Item::ShulkerShell),
            xp_reward: 35,
            x_pos: 4.5,
            y_pos: -1.0,
        });
        self.add(Advancement {
            id: "end:remote_travel",
            category: AdvancementCategory::TheEnd,
            title: "Zero Gravity",
            description: "Defeat a Shulker in combat",
            icon_item: Item::ShulkerShell,
            frame: AdvancementFrameType::Task,
            parent: Some("end:enter_end_city"),
            trigger: AdvancementTrigger::KillMob(EntityType::Shulker),
            xp_reward: 40,
            x_pos: 4.5,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "end:dragon_breath",
            category: AdvancementCategory::TheEnd,
            title: "Chemical Warfare",
            description: "Brew a lingering potion",
            icon_item: Item::SplashPotion,
            frame: AdvancementFrameType::Challenge,
            parent: Some("end:you_need_a_mint"),
            trigger: AdvancementTrigger::ObtainItem(Item::SplashPotion),
            xp_reward: 100,
            x_pos: 4.5,
            y_pos: 2.0,
        });

        // Category 4: Adventure (10)
        self.add(Advancement {
            id: "adventure:root",
            category: AdvancementCategory::Adventure,
            title: "Adventure",
            description: "Adventure, exploration and combat",
            icon_item: Item::Bow,
            frame: AdvancementFrameType::Task,
            parent: None,
            trigger: AdvancementTrigger::Root,
            xp_reward: 0,
            x_pos: 0.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "adventure:monster_hunter",
            category: AdvancementCategory::Adventure,
            title: "Monster Hunter",
            description: "Kill any hostile monster",
            icon_item: Item::IronSword,
            frame: AdvancementFrameType::Task,
            parent: Some("adventure:root"),
            trigger: AdvancementTrigger::KillMob(EntityType::Zombie),
            xp_reward: 20,
            x_pos: 1.5,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "adventure:monsters_hunted",
            category: AdvancementCategory::Adventure,
            title: "Monsters Hunted",
            description: "Defeat a Creeper in battle",
            icon_item: Item::DiamondSword,
            frame: AdvancementFrameType::Challenge,
            parent: Some("adventure:monster_hunter"),
            trigger: AdvancementTrigger::KillMob(EntityType::Creeper),
            xp_reward: 100,
            x_pos: 3.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "adventure:post_box",
            category: AdvancementCategory::Adventure,
            title: "Bullseye",
            description: "Craft arrows for ranged combat",
            icon_item: Item::Arrow,
            frame: AdvancementFrameType::Task,
            parent: Some("adventure:root"),
            trigger: AdvancementTrigger::CraftItem(Item::Arrow),
            xp_reward: 15,
            x_pos: 1.5,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "adventure:sniper_duel",
            category: AdvancementCategory::Adventure,
            title: "Sniper Duel",
            description: "Kill a Skeleton with an arrow",
            icon_item: Item::Bow,
            frame: AdvancementFrameType::Challenge,
            parent: Some("adventure:monster_hunter"),
            trigger: AdvancementTrigger::KillMob(EntityType::Skeleton),
            xp_reward: 100,
            x_pos: 3.0,
            y_pos: -1.0,
        });
        self.add(Advancement {
            id: "adventure:sticky_situation",
            category: AdvancementCategory::Adventure,
            title: "Sticky Situation",
            description: "Craft a sticky piston",
            icon_item: Item::StickyPiston,
            frame: AdvancementFrameType::Task,
            parent: Some("adventure:root"),
            trigger: AdvancementTrigger::CraftItem(Item::StickyPiston),
            xp_reward: 20,
            x_pos: 1.5,
            y_pos: -1.0,
        });
        self.add(Advancement {
            id: "adventure:trade",
            category: AdvancementCategory::Adventure,
            title: "What a Deal!",
            description: "Obtain a gold ingot",
            icon_item: Item::GoldIngot,
            frame: AdvancementFrameType::Task,
            parent: Some("adventure:root"),
            trigger: AdvancementTrigger::ObtainItem(Item::GoldIngot),
            xp_reward: 20,
            x_pos: 1.5,
            y_pos: 2.0,
        });
        self.add(Advancement {
            id: "adventure:light_as_a_feather",
            category: AdvancementCategory::Adventure,
            title: "Light as a Feather",
            description: "Obtain a feather from chickens",
            icon_item: Item::Feather,
            frame: AdvancementFrameType::Task,
            parent: Some("adventure:root"),
            trigger: AdvancementTrigger::ObtainItem(Item::Feather),
            xp_reward: 15,
            x_pos: 1.5,
            y_pos: -2.0,
        });
        self.add(Advancement {
            id: "adventure:voluntary_exile",
            category: AdvancementCategory::Adventure,
            title: "Voluntary Exile",
            description: "Defeat a boss or husk mob",
            icon_item: Item::RedDye,
            frame: AdvancementFrameType::Goal,
            parent: Some("adventure:monster_hunter"),
            trigger: AdvancementTrigger::KillMob(EntityType::Husk),
            xp_reward: 50,
            x_pos: 3.0,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "adventure:hero_of_the_village",
            category: AdvancementCategory::Adventure,
            title: "Hero of the Village",
            description: "Successfully protect the realm from bosses",
            icon_item: Item::GoldIngot,
            frame: AdvancementFrameType::Challenge,
            parent: Some("adventure:voluntary_exile"),
            trigger: AdvancementTrigger::KillMob(EntityType::Wither),
            xp_reward: 100,
            x_pos: 4.5,
            y_pos: 1.0,
        });

        // Category 5: Husbandry (10)
        self.add(Advancement {
            id: "husbandry:root",
            category: AdvancementCategory::Husbandry,
            title: "Husbandry",
            description: "The world is full of friends and food",
            icon_item: Item::Apple,
            frame: AdvancementFrameType::Task,
            parent: None,
            trigger: AdvancementTrigger::EatFood(Item::Apple),
            xp_reward: 0,
            x_pos: 0.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "husbandry:plant_seed",
            category: AdvancementCategory::Husbandry,
            title: "A Seedy Place",
            description: "Obtain seeds to start farming",
            icon_item: Item::Seeds,
            frame: AdvancementFrameType::Task,
            parent: Some("husbandry:root"),
            trigger: AdvancementTrigger::ObtainItem(Item::Seeds),
            xp_reward: 10,
            x_pos: 1.5,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "husbandry:breed_an_animal",
            category: AdvancementCategory::Husbandry,
            title: "The Parrots and the Bats",
            description: "Breed two animals together",
            icon_item: Item::Wheat,
            frame: AdvancementFrameType::Task,
            parent: Some("husbandry:root"),
            trigger: AdvancementTrigger::BreedAnimals,
            xp_reward: 20,
            x_pos: 1.5,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "husbandry:tame_an_animal",
            category: AdvancementCategory::Husbandry,
            title: "Best Friends Forever",
            description: "Tame or feed an animal with bones",
            icon_item: Item::Bone,
            frame: AdvancementFrameType::Task,
            parent: Some("husbandry:breed_an_animal"),
            trigger: AdvancementTrigger::ObtainItem(Item::Bone),
            xp_reward: 15,
            x_pos: 3.0,
            y_pos: 1.0,
        });
        self.add(Advancement {
            id: "husbandry:balanced_diet",
            category: AdvancementCategory::Husbandry,
            title: "Balanced Diet",
            description: "Eat bread to stay healthy",
            icon_item: Item::Bread,
            frame: AdvancementFrameType::Goal,
            parent: Some("husbandry:root"),
            trigger: AdvancementTrigger::EatFood(Item::Bread),
            xp_reward: 35,
            x_pos: 1.5,
            y_pos: -1.0,
        });
        self.add(Advancement {
            id: "husbandry:serious_dedication",
            category: AdvancementCategory::Husbandry,
            title: "Serious Dedication",
            description: "Craft a diamond pickaxe or tool",
            icon_item: Item::Diamond,
            frame: AdvancementFrameType::Challenge,
            parent: Some("husbandry:root"),
            trigger: AdvancementTrigger::CraftItem(Item::DiamondPickaxe),
            xp_reward: 100,
            x_pos: 1.5,
            y_pos: -2.0,
        });
        self.add(Advancement {
            id: "husbandry:tactical_fishing",
            category: AdvancementCategory::Husbandry,
            title: "Fishy Business",
            description: "Obtain food from raw meat",
            icon_item: Item::RawPorkchop,
            frame: AdvancementFrameType::Task,
            parent: Some("husbandry:root"),
            trigger: AdvancementTrigger::ObtainItem(Item::RawPorkchop),
            xp_reward: 15,
            x_pos: 1.5,
            y_pos: 2.0,
        });
        self.add(Advancement {
            id: "husbandry:wax_on",
            category: AdvancementCategory::Husbandry,
            title: "Wax On",
            description: "Collect flowers from nature",
            icon_item: Item::Dandelion,
            frame: AdvancementFrameType::Task,
            parent: Some("husbandry:plant_seed"),
            trigger: AdvancementTrigger::MineBlock(BlockType::Dandelion),
            xp_reward: 15,
            x_pos: 3.0,
            y_pos: 0.0,
        });
        self.add(Advancement {
            id: "husbandry:two_by_two",
            category: AdvancementCategory::Husbandry,
            title: "Two by Two",
            description: "Cook beef or porkchop",
            icon_item: Item::CookedBeef,
            frame: AdvancementFrameType::Challenge,
            parent: Some("husbandry:breed_an_animal"),
            trigger: AdvancementTrigger::ObtainItem(Item::CookedBeef),
            xp_reward: 100,
            x_pos: 3.0,
            y_pos: 2.0,
        });
        self.add(Advancement {
            id: "husbandry:complete_catalogue",
            category: AdvancementCategory::Husbandry,
            title: "A Complete Catalogue",
            description: "Collect an egg from a chicken",
            icon_item: Item::Egg,
            frame: AdvancementFrameType::Goal,
            parent: Some("husbandry:tame_an_animal"),
            trigger: AdvancementTrigger::ObtainItem(Item::Egg),
            xp_reward: 50,
            x_pos: 4.5,
            y_pos: 1.0,
        });
    }
}

pub struct AdvancementManager {
    pub tree: AdvancementTree,
    pub progress: AdvancementProgressData,
    pub active_toasts: Vec<ToastNotification>,
}

impl AdvancementManager {
    pub fn new(progress: AdvancementProgressData) -> Self {
        let tree = AdvancementTree::new();
        let mut manager = Self {
            tree,
            progress,
            active_toasts: Vec::new(),
        };
        manager.check_roots();
        manager
    }

    fn check_roots(&mut self) {
        let root_ids: Vec<&'static str> = self
            .tree
            .list
            .iter()
            .filter(|a| a.parent.is_none() && matches!(a.trigger, AdvancementTrigger::Root))
            .map(|a| a.id)
            .collect();
        for id in root_ids {
            if !self.progress.completed_ids.contains(id) {
                self.progress.completed_ids.insert(id.to_string());
            }
        }
    }

    pub fn is_unlocked(&self, id: &str) -> bool {
        self.progress.completed_ids.contains(id)
    }

    pub fn check_trigger(&mut self, trigger: &AdvancementTrigger) -> Vec<String> {
        let mut newly_completed = Vec::new();
        let list_len = self.tree.list.len();

        for i in 0..list_len {
            let adv = &self.tree.list[i];
            let id = adv.id;

            if self.progress.completed_ids.contains(id) {
                continue;
            }

            // Check parent requirement
            if let Some(parent_id) = adv.parent {
                if !self.progress.completed_ids.contains(parent_id) {
                    continue;
                }
            }

            // Check trigger match
            if self.trigger_matches(&adv.trigger, trigger) {
                newly_completed.push(id.to_string());
            }
        }

        for id in &newly_completed {
            self.progress.completed_ids.insert(id.clone());
            if let Some(adv) = self.tree.get(id) {
                self.active_toasts.push(ToastNotification {
                    title: adv.title.to_string(),
                    description: adv.description.to_string(),
                    frame: adv.frame,
                    icon_item: adv.icon_item,
                    timer: 0.0,
                });
            }
        }

        newly_completed
    }

    fn trigger_matches(
        &self,
        adv_trigger: &AdvancementTrigger,
        event_trigger: &AdvancementTrigger,
    ) -> bool {
        match (adv_trigger, event_trigger) {
            (AdvancementTrigger::ObtainItem(i1), AdvancementTrigger::ObtainItem(i2)) => i1 == i2,
            (AdvancementTrigger::CraftItem(i1), AdvancementTrigger::CraftItem(i2)) => i1 == i2,
            (AdvancementTrigger::MineBlock(b1), AdvancementTrigger::MineBlock(b2)) => b1 == b2,
            (AdvancementTrigger::KillMob(m1), AdvancementTrigger::KillMob(m2)) => m1 == m2,
            (AdvancementTrigger::EnterDimension(d1), AdvancementTrigger::EnterDimension(d2)) => {
                d1 == d2
            }
            (AdvancementTrigger::BrewPotion, AdvancementTrigger::BrewPotion) => true,
            (AdvancementTrigger::EnchantItem, AdvancementTrigger::EnchantItem) => true,
            (AdvancementTrigger::EatFood(f1), AdvancementTrigger::EatFood(f2)) => f1 == f2,
            (AdvancementTrigger::BreedAnimals, AdvancementTrigger::BreedAnimals) => true,
            (AdvancementTrigger::Root, _) => true,
            _ => false,
        }
    }

    pub fn update_toasts(&mut self, dt: f32) {
        for toast in &mut self.active_toasts {
            toast.timer += dt;
        }
        self.active_toasts.retain(|t| t.timer < 3.0);
    }
}

pub struct AdvancementGui {
    pub is_open: bool,
    pub selected_category: AdvancementCategory,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub zoom: f32,
    pub hovered_advancement: Option<&'static str>,
    pub is_dragging: bool,
    pub drag_start_x: f32,
    pub drag_start_y: f32,
}

impl AdvancementGui {
    pub fn new() -> Self {
        Self {
            is_open: false,
            selected_category: AdvancementCategory::Minecraft,
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 1.0,
            hovered_advancement: None,
            is_dragging: false,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
    }

    pub fn toggle(&mut self) {
        if self.is_open {
            self.close();
        } else {
            self.open();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advancement_tree_registration() {
        let tree = AdvancementTree::new();
        assert_eq!(tree.list.len(), 50);
        assert!(tree.get("minecraft:root").is_some());
        assert!(tree.get("nether:root").is_some());
        assert!(tree.get("end:root").is_some());
        assert!(tree.get("adventure:root").is_some());
        assert!(tree.get("husbandry:root").is_some());

        // Check parent links
        for adv in &tree.list {
            if let Some(parent_id) = adv.parent {
                assert!(
                    tree.get(parent_id).is_some(),
                    "Parent {} for {} not found",
                    parent_id,
                    adv.id
                );
            }
        }
    }

    #[test]
    fn test_trigger_matching_and_progression() {
        let mut mgr = AdvancementManager::new(AdvancementProgressData::default());
        assert!(!mgr.is_unlocked("minecraft:root"));

        // Mine wood -> unlocks minecraft:root
        let completed = mgr.check_trigger(&AdvancementTrigger::MineBlock(BlockType::OakLog));
        assert!(completed.contains(&"minecraft:root".to_string()));
        assert!(mgr.is_unlocked("minecraft:root"));

        // Craft stone pickaxe -> unlocks minecraft:stone_age
        let completed = mgr.check_trigger(&AdvancementTrigger::CraftItem(Item::StonePickaxe));
        assert!(completed.contains(&"minecraft:stone_age".to_string()));
        assert!(mgr.is_unlocked("minecraft:stone_age"));

        // Obtain iron ingot -> unlocks minecraft:getting_hardware
        let completed = mgr.check_trigger(&AdvancementTrigger::ObtainItem(Item::IronIngot));
        assert!(completed.contains(&"minecraft:getting_hardware".to_string()));
        assert!(mgr.is_unlocked("minecraft:getting_hardware"));
    }

    #[test]
    fn test_serialization() {
        let mut progress = AdvancementProgressData::default();
        progress.completed_ids.insert("minecraft:root".to_string());
        progress
            .criteria_progress
            .insert("test_crit".to_string(), 5);

        let encoded = bincode::serialize(&progress).expect("Failed serialize");
        let decoded: AdvancementProgressData =
            bincode::deserialize(&encoded).expect("Failed deserialize");
        assert_eq!(progress, decoded);
    }
}
