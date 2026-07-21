# Advancements System Design (任務 28)

This document details the architectural design and implementation specification for the **Advancements / Achievement System** (`src/advancements.rs`) in iCraft.

---

## 1. Overview

The **Advancements System** provides structured goals, progression tracking, visual toast notifications, an in-game interactive tree UI screen, and persistent progress saving across world loads.

### Key Features
1. **Advancement Tree Structure**: 5 categories (Story/Minecraft, Nether, End, Adventure, Husbandry) containing ~50 achievements organized hierarchically.
2. **Flexible Trigger Engine**: Listens to gameplay events (item obtaining, block mining, mob killing, crafting, brewing, enchanting, dimension changing, biome entering, and eating).
3. **Toast Notifications**: Top-right animated popups with category-themed banners, frame badges, icons, titles, and sound effects upon completion.
4. **Interactive Advancements GUI (Key 'L')**: A tabbed interactive menu displaying connected node graphs for each category with tooltips, pan/scroll, and locked/unlocked visual indicators.
5. **Persistence**: Saves player completed advancement IDs and criterion counters in `saves/<world>/` via `PlayerData` in `src/save.rs`.

---

## 2. Affected Modules & Exact Symbols

| Module / File | Status | Key Symbols & Responsibilities |
| --- | --- | --- |
| `src/advancements.rs` | **[NEW]** | `Advancement`, `AdvancementCategory`, `AdvancementFrameType`, `AdvancementTrigger`, `AdvancementReward`, `AdvancementTree`, `AdvancementManager`, `ToastNotification`, `AdvancementGui` |
| `src/save.rs` | **[MODIFY]** | `PlayerData` (add `advancements: AdvancementProgressData`), `AdvancementProgressData` |
| `src/state.rs` | **[MODIFY]** | `State` (`open_advancements_ui`, `close_advancements_ui`, `check_advancement_triggers`, UI rendering in `render()`, state updates in `update()`) |
| `src/app.rs` | **[MODIFY]** | `App::window_event` (handle `L` key binding to toggle Advancements GUI, route mouse clicks when GUI is open) |
| `src/inventory.rs` | **[MODIFY]** | `Inventory::add_item`, item craft/pickup trigger notifications |
| `src/crafting.rs` | **[MODIFY]** | Crafting completion event triggers |
| `src/mob.rs` / `src/boss.rs` | **[MODIFY]** | Hostile mob and boss kill event triggers |
| `src/enchantment.rs` / `src/brewing.rs` | **[MODIFY]** | Enchantment and potion brewing event triggers |

---

## 3. Data Architecture & Types (`src/advancements.rs`)

### 3.1. Enums and Structs

```rust
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use crate::inventory::Item;
use crate::world::BlockType;
use crate::entity::EntityType;
use crate::dimension::Dimension;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdvancementCategory {
    Minecraft, // Story / Main
    Nether,    // Nether Dimension
    TheEnd,    // End Dimension
    Adventure, // Combat & Exploration
    Husbandry, // Farming & Crafting
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvancementFrameType {
    Task,      // Square border (normal)
    Goal,      // Rounded border (intermediate goal)
    Challenge, // Spiked golden border (major challenge)
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
    Root, // Unlocked automatically when category opens
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
    pub x_pos: f32, // GUI tree layout coordinate X
    pub y_pos: f32, // GUI tree layout coordinate Y
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdvancementProgressData {
    pub completed_ids: HashSet<String>,
    pub criteria_progress: HashMap<String, u32>,
}

pub struct ToastNotification {
    pub title: String,
    pub description: String,
    pub frame: AdvancementFrameType,
    pub icon_item: Item,
    pub timer: f32, // Total 3.0 seconds (0.4s slide-in, 2.2s display, 0.4s slide-out)
}
```

---

## 4. Advancement Tree Definition (~50 Advancements)

The system registers 50 advancements across 5 categories:

### Category 1: Minecraft (Story - 10 Advancements)
1. `minecraft:root` — **Minecraft**: Mine wood (Root)
2. `minecraft:stone_age` — **Stone Age**: Craft a Stone Pickaxe
3. `minecraft:getting_hardware` — **Getting Hardware**: Mine Iron Ore or craft an Iron Ingot
4. `minecraft:suit_up` — **Suit Up**: Protect yourself with Iron Armor
5. `minecraft:hot_stuff` — **Hot Stuff**: Fill a bucket with lava
6. `minecraft:isn_it_iron_pick` — **Isn't It Iron Pick**: Craft an Iron Pickaxe
7. `minecraft:not_today_thank_you` — **Not Today, Thank You**: Craft a Shield
8. `minecraft:ice_bucket_challenge` — **Ice Bucket Challenge**: Obtain Obsidian
9. `minecraft:diamonds` — **Diamonds!**: Acquire diamonds
10. `minecraft:cover_me_with_diamonds` — **Cover Me with Diamonds**: Wear full Diamond Armor

### Category 2: Nether (10 Advancements)
11. `nether:root` — **Nether**: Enter the Nether dimension (Root)
12. `nether:into_fire` — **Into Fire**: Obtain a Blaze Rod
13. `nether:local_brewery` — **Local Brewery**: Brew a potion
14. `nether:withering_heights` — **Withering Heights**: Summon the Wither
15. `nether:bring_home_the_beacon` — **Bring Home the Beacon**: Obtain a Nether Star
16. `nether:spooky_scary_skeleton` — **Spooky Scary Skeleton**: Obtain a Wither Skeleton Skull
17. `nether:return_to_sender` — **Return to Sender**: Destroy a Ghast with a fireball
18. `nether:subspace_bubble` — **Subspace Bubble**: Use the Nether to travel 7 km in Overworld
19. `nether:uneasy_alliance` — **Uneasy Alliance**: Rescue a Ghast from the Nether to the Overworld
20. `nether:a_furious_cocktail` — **A Furious Cocktail**: Have every potion effect applied

### Category 3: The End (10 Advancements)
21. `end:root` — **The End?**: Enter the End Portal (Root)
22. `end:kill_dragon` — **Free the End**: Defeat the Ender Dragon
23. `end:dragon_egg` — **Next Generation**: Hold the Dragon Egg
24. `end:enter_end_city` — **The City at the End of the Game**: Discover an End City
25. `end:elytra` — **Sky's the Limit**: Find an Elytra
26. `end:great_escape` — **Great Escape**: Escape through the Exit Portal
27. `end:you_need_a_mint` — **You Need a Mint**: Collect Dragon's Breath in a glass bottle
28. `end:shulker_box` — **Remote Storage**: Craft a Shulker Box
29. `end:remote_travel` — **Zero Gravity**: Get levitated by a Shulker
30. `end:dragon_breath` — **Chemical Warfare**: Use Dragon Breath to brew a lingering potion

### Category 4: Adventure (10 Advancements)
31. `adventure:root` — **Adventure**: Kill a mob or explore (Root)
32. `adventure:monster_hunter` — **Monster Hunter**: Kill any hostile monster
33. `adventure:monsters_hunted` — **Monsters Hunted**: Defeat one of every monster species
34. `adventure:post_box` — **Bullseye**: Hit a target block
35. `adventure:sniper_duel` — **Sniper Duel**: Kill a Skeleton with an arrow from over 50 meters
36. `adventure:sticky_situation` — **Sticky Situation**: Slide on a honey/slime block
37. `adventure:trade` — **What a Deal!**: Successfully trade or barter
38. `adventure:light_as_a_feather` — **Light as a Feather**: Wear Leather Boots on powdered snow
39. `adventure:voluntary_exile` — **Voluntary Exile**: Defeat a raid captain / boss mob
40. `adventure:hero_of_the_village` — **Hero of the Village**: Successfully protect the realm

### Category 5: Husbandry (10 Advancements)
41. `husbandry:root` — **Husbandry**: Eat any food item (Root)
42. `husbandry:plant_seed` — **A Seedy Place**: Plant a seed and watch it grow
43. `husbandry:breed_an_animal` — **The Parrots and the Bats**: Breed two animals together
44. `husbandry:tame_an_animal` — **Best Friends Forever**: Tame an animal
45. `husbandry:balanced_diet` — **Balanced Diet**: Eat everything that is edible
46. `husbandry:serious_dedication` — **Serious Dedication**: Craft a Diamond Hoe
47. `husbandry:tactical_fishing` — **Fishy Business**: Catch a fish with a fishing rod
48. `husbandry:wax_on` — **Wax On**: Apply honeycomb to a copper block
49. `husbandry:two_by_two` — **Two by Two**: Breed all animal pairs
50. `husbandry:complete_catalogue` — **A Complete Catalogue**: Discover all passive animal variants

---

## 5. User Interface & Display

### 5.1. Toast Popup Notification
- Position: Top-right corner (`window_width - 240, 20`).
- Background: Translucent dark panel (`rgba(20, 20, 25, 0.85)`).
- Frame Badge: Yellow text for Challenge, Light Blue for Goal, White for Task.
- Text: Top line `"Advancement Made!"`, bottom line `Advancement Title`.
- Icon: Rendered small item icon matching `icon_item`.
- Animation: Smooth horizontal slide-in (0.4s), hold (2.2s), slide-out (0.4s).

### 5.2. Advancements Screen (Toggle Key: 'L')
- Background: Darkened blur overlay (`rgba(0, 0, 0, 0.6)`).
- Category Bar: Top tab selector with 5 category icons (`Minecraft`, `Nether`, `TheEnd`, `Adventure`, `Husbandry`).
- Graph View:
  - Lines drawn between parent and child advancement nodes.
  - Node Frames: Golden spiked border for Challenge, rounded border for Goal, square for Task.
  - Locked State: Greyed out node frame and semi-transparent icon.
  - Unlocked State: Bright frame with full-color icon and glow effect.
- Tooltip: Hovering over any node presents title, description, and completion status.
- Interaction: Mouse dragging pans the graph view; mouse wheel zooms or scrolls.

---

## 6. Persistence Model

`PlayerData` in `src/save.rs` will be extended with:
```rust
pub struct PlayerData {
    // Existing fields...
    pub advancements: AdvancementProgressData,
}
```
When loading a world save without `advancements` data (backward compatibility), it gracefully defaults to `AdvancementProgressData::default()`.

---

## 7. Verification Plan

### 7.1. Automated Unit Tests
- `test_advancement_tree_registration`: Verify all 50 advancements are correctly registered with valid parent-child relationships and unique IDs.
- `test_trigger_matching`: Verify `AdvancementManager::check_trigger` unlocks child nodes sequentially.
- `test_serialization`: Verify `AdvancementProgressData` serializes and deserializes cleanly with Bincode/Zlib.

### 7.2. Manual Verification
- Press `L` in game to toggle the Advancements GUI screen. Test tab switching and node hover tooltips.
- Pick up Wood / Stone / Iron in Survival mode; verify toast popups appear in the top-right corner.
- Save and quit, re-enter world, press `L` to confirm advancements remain unlocked.
