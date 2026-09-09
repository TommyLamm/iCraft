//! Pure world-click and inventory-click hit resolution.
//!
//! Desktop binary-only (`mod` in `src/main.rs`). Not part of the `icraft`
//! library, so `icraft-server` does not compile this file.
//! `presentation_inventory_policy` is the thin public cut for integration tests.
//!
//! No GPU and no `State` fields. Join and Embedded share this resolver, then
//! map the intent separately. Topology-specific edges are locked here so they
//! are not "aligned" by accident:
//!
//! | | Join | Embedded |
//! |---|---|---|
//! | StartBreak | no `can_break` gate | reject if `!can_break` |
//! | Empty-hand Place | not sent | Place(Air) |
//! | OpenContainer set | automation + workstations | automation only |
//! | After resolve | `open_chest` / no `Action::*` | Container Open + `Action::Break`/`Place` |

use crate::inventory::Item;
use crate::presentation_inventory_policy::PresentationTopology;
use crate::world::BlockType;

/// Result of one world-click hit resolution. Callers map this to packets /
/// `submit_local_authority_block_action`; they must not re-branch on block
/// type for the live Join / Embedded paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldClickIntent {
    Miss,
    StartBreak {
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
    },
    IgnitePortal {
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
    },
    InsertEnderEye {
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
    },
    Sleep {
        x: i32,
        y: i32,
        z: i32,
    },
    OpenContainer {
        x: i32,
        y: i32,
        z: i32,
        block: BlockType,
    },
    Place {
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
        block: BlockType,
    },
    /// Collision / adventure / unbreakable gate rejected the click.
    Rejected,
}

/// Presentation-side hit already converted to integer block coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldClickHit {
    pub clicked: [i32; 3],
    pub place: [i32; 3],
    pub face: [i8; 3],
    pub clicked_block: BlockType,
}

/// Classify one world click. `can_break` / `can_place` are computed by the
/// caller from current inventory and collision; this function does not read
/// `State`.
pub fn resolve_world_click(
    topology: PresentationTopology,
    is_left_click: bool,
    hit: Option<WorldClickHit>,
    held_item: Item,
    selected_block: Option<BlockType>,
    can_break: bool,
    can_place: bool,
) -> WorldClickIntent {
    let Some(hit) = hit else {
        return WorldClickIntent::Miss;
    };
    if is_left_click {
        // Join historically sends StartBreak without the presentation
        // breakability gate. Embedded rejects first. Do not align.
        if topology.is_embedded() && !can_break {
            return WorldClickIntent::Rejected;
        }
        return WorldClickIntent::StartBreak {
            x: hit.clicked[0],
            y: hit.clicked[1],
            z: hit.clicked[2],
            face: hit.face,
        };
    }

    if hit.clicked_block == BlockType::Obsidian && held_item == Item::FlintAndSteel {
        return WorldClickIntent::IgnitePortal {
            x: hit.place[0],
            y: hit.place[1],
            z: hit.place[2],
            face: hit.face,
        };
    }
    if hit.clicked_block == BlockType::EndPortalFrame && held_item == Item::EyeOfEnder {
        return WorldClickIntent::InsertEnderEye {
            x: hit.clicked[0],
            y: hit.clicked[1],
            z: hit.clicked[2],
            face: hit.face,
        };
    }
    if hit.clicked_block == BlockType::Bed {
        return WorldClickIntent::Sleep {
            x: hit.clicked[0],
            y: hit.clicked[1],
            z: hit.clicked[2],
        };
    }
    if is_authority_container(topology, hit.clicked_block) {
        return WorldClickIntent::OpenContainer {
            x: hit.clicked[0],
            y: hit.clicked[1],
            z: hit.clicked[2],
            block: hit.clicked_block,
        };
    }

    if let Some(block) = selected_block {
        if !can_place {
            return WorldClickIntent::Rejected;
        }
        return WorldClickIntent::Place {
            x: hit.place[0],
            y: hit.place[1],
            z: hit.place[2],
            face: hit.face,
            block,
        };
    }

    // Empty-hand Place is Embedded-only. Join historically sends nothing.
    if topology.is_embedded() {
        return WorldClickIntent::Place {
            x: hit.place[0],
            y: hit.place[1],
            z: hit.place[2],
            face: hit.face,
            block: BlockType::Air,
        };
    }
    WorldClickIntent::Miss
}

/// Automation containers both topologies open. Join also treats workstations
/// as `open_chest` projections; Embedded lets those fall through to Place.
fn is_authority_container(topology: PresentationTopology, block: BlockType) -> bool {
    if matches!(
        block,
        BlockType::Chest
            | BlockType::EndCityChest
            | BlockType::Furnace
            | BlockType::FurnaceLit
            | BlockType::Hopper
            | BlockType::Dispenser
            | BlockType::Dropper
    ) {
        return true;
    }
    topology.is_join_client()
        && matches!(
            block,
            BlockType::CraftingTable
                | BlockType::EnchantingTable
                | BlockType::BrewingStand
                | BlockType::Anvil
        )
}

/// One inventory-screen region. Slot payload stays with the caller because
/// `SlotType` lives on the presentation `State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryHit<S> {
    Merchant { offer_index: usize },
    CreativeTab { index: usize },
    RecipeBookToggle,
    RecipeBook,
    Enchant { option_index: usize },
    Slot(S),
    Empty,
}

/// Every inventory region tested once. Callers pick authority-first or
/// UI-first so Join/Embedded vs leftover overlay order stays intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryHitProbe<S> {
    pub merchant: Option<usize>,
    pub creative_tab: Option<usize>,
    pub recipe_book_toggle: bool,
    pub recipe_book: bool,
    pub enchant: Option<usize>,
    pub slot: Option<S>,
}

impl<S: Copy> InventoryHitProbe<S> {
    /// Join / Embedded gate: merchant offer, then inventory slot.
    pub fn authority_hit(self) -> InventoryHit<S> {
        if let Some(offer_index) = self.merchant {
            InventoryHit::Merchant { offer_index }
        } else if let Some(slot) = self.slot {
            InventoryHit::Slot(slot)
        } else if let Some(index) = self.creative_tab {
            InventoryHit::CreativeTab { index }
        } else if self.recipe_book_toggle {
            InventoryHit::RecipeBookToggle
        } else if self.recipe_book {
            InventoryHit::RecipeBook
        } else if let Some(option_index) = self.enchant {
            InventoryHit::Enchant { option_index }
        } else {
            InventoryHit::Empty
        }
    }

    /// Leftover local mutate / Embedded player-inventory writeback apply.
    pub fn ui_hit(self) -> InventoryHit<S> {
        if let Some(index) = self.creative_tab {
            InventoryHit::CreativeTab { index }
        } else if self.recipe_book_toggle {
            InventoryHit::RecipeBookToggle
        } else if let Some(offer_index) = self.merchant {
            InventoryHit::Merchant { offer_index }
        } else if self.recipe_book {
            InventoryHit::RecipeBook
        } else if let Some(option_index) = self.enchant {
            InventoryHit::Enchant { option_index }
        } else if let Some(slot) = self.slot {
            InventoryHit::Slot(slot)
        } else {
            InventoryHit::Empty
        }
    }
}

/// Merchant / recipe-book / enchant regions. Slot and creative-tab rects stay
/// in `State` because they need live layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InventoryUiHits {
    pub merchant: Option<usize>,
    pub recipe_book_toggle: bool,
    pub recipe_book: bool,
    pub enchant: Option<usize>,
}

pub fn collect_inventory_ui_hits(
    mouse_x: f32,
    mouse_y: f32,
    is_left: bool,
    merchant_active: bool,
    merchant_offer_count: usize,
    recipe_book_open: bool,
    enchanting: bool,
) -> InventoryUiHits {
    let mut hits = InventoryUiHits::default();
    if is_left && merchant_active {
        hits.merchant = merchant_offer_at(mouse_x, mouse_y, merchant_offer_count);
    }
    if is_left {
        hits.recipe_book_toggle = is_recipe_book_toggle(mouse_x, mouse_y);
        if recipe_book_open {
            hits.recipe_book = is_recipe_book_panel(mouse_x, mouse_y);
        }
        if enchanting {
            hits.enchant = enchant_option_at(mouse_x, mouse_y);
        }
    }
    hits
}

pub fn merchant_offer_at(mouse_x: f32, mouse_y: f32, offer_count: usize) -> Option<usize> {
    let mut offer_y = 0.28;
    for idx in 0..offer_count {
        if mouse_x >= -0.35
            && mouse_x <= 0.35
            && mouse_y >= offer_y - 0.04
            && mouse_y <= offer_y + 0.03
        {
            return Some(idx);
        }
        offer_y -= 0.09;
        if offer_y < -0.30 {
            break;
        }
    }
    None
}

pub fn is_recipe_book_toggle(mouse_x: f32, mouse_y: f32) -> bool {
    mouse_x >= -0.45 && mouse_x <= -0.37 && mouse_y >= 0.35 && mouse_y <= 0.43
}

pub fn is_recipe_book_panel(mouse_x: f32, mouse_y: f32) -> bool {
    mouse_x >= -0.85 && mouse_x <= -0.48 && mouse_y >= -0.45 && mouse_y <= 0.45
}

pub fn enchant_option_at(mouse_x: f32, mouse_y: f32) -> Option<usize> {
    for index in 0..3 {
        let y1 = 0.28 - index as f32 * 0.12;
        let y0 = y1 - 0.09;
        if mouse_x >= 0.02 && mouse_x <= 0.62 && mouse_y >= y0 && mouse_y <= y1 {
            return Some(index);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stone_hit() -> WorldClickHit {
        WorldClickHit {
            clicked: [1, 2, 3],
            place: [1, 3, 3],
            face: [0, 1, 0],
            clicked_block: BlockType::Stone,
        }
    }

    fn hit(block: BlockType) -> WorldClickHit {
        WorldClickHit {
            clicked: [4, 5, 6],
            place: [4, 6, 6],
            face: [0, 1, 0],
            clicked_block: block,
        }
    }

    #[test]
    fn join_start_break_ignores_can_break() {
        let intent = resolve_world_click(
            PresentationTopology::JoinClient,
            true,
            Some(stone_hit()),
            Item::Air,
            None,
            false,
            true,
        );
        assert_eq!(
            intent,
            WorldClickIntent::StartBreak {
                x: 1,
                y: 2,
                z: 3,
                face: [0, 1, 0],
            }
        );
    }

    #[test]
    fn embedded_start_break_requires_can_break() {
        let rejected = resolve_world_click(
            PresentationTopology::Embedded,
            true,
            Some(stone_hit()),
            Item::Air,
            None,
            false,
            true,
        );
        let accepted = resolve_world_click(
            PresentationTopology::Embedded,
            true,
            Some(stone_hit()),
            Item::Air,
            None,
            true,
            true,
        );
        assert_eq!(rejected, WorldClickIntent::Rejected);
        assert_eq!(
            accepted,
            WorldClickIntent::StartBreak {
                x: 1,
                y: 2,
                z: 3,
                face: [0, 1, 0],
            }
        );
    }

    #[test]
    fn empty_hand_place_is_embedded_only() {
        let join = resolve_world_click(
            PresentationTopology::JoinClient,
            false,
            Some(stone_hit()),
            Item::Air,
            None,
            true,
            true,
        );
        let embedded = resolve_world_click(
            PresentationTopology::Embedded,
            false,
            Some(stone_hit()),
            Item::Air,
            None,
            true,
            true,
        );
        assert_eq!(join, WorldClickIntent::Miss);
        assert_eq!(
            embedded,
            WorldClickIntent::Place {
                x: 1,
                y: 3,
                z: 3,
                face: [0, 1, 0],
                block: BlockType::Air,
            }
        );
    }

    #[test]
    fn join_opens_workstations_embedded_does_not() {
        let join = resolve_world_click(
            PresentationTopology::JoinClient,
            false,
            Some(hit(BlockType::CraftingTable)),
            Item::Air,
            None,
            true,
            true,
        );
        let embedded = resolve_world_click(
            PresentationTopology::Embedded,
            false,
            Some(hit(BlockType::CraftingTable)),
            Item::Air,
            None,
            true,
            true,
        );
        assert_eq!(
            join,
            WorldClickIntent::OpenContainer {
                x: 4,
                y: 5,
                z: 6,
                block: BlockType::CraftingTable,
            }
        );
        // Embedded empty-hand: workstation is not a container, so Place(Air).
        assert_eq!(
            embedded,
            WorldClickIntent::Place {
                x: 4,
                y: 6,
                z: 6,
                face: [0, 1, 0],
                block: BlockType::Air,
            }
        );
    }

    #[test]
    fn both_open_automation_containers() {
        for topology in [
            PresentationTopology::JoinClient,
            PresentationTopology::Embedded,
        ] {
            let intent = resolve_world_click(
                topology,
                false,
                Some(hit(BlockType::Chest)),
                Item::Dirt,
                Some(BlockType::Dirt),
                true,
                true,
            );
            assert_eq!(
                intent,
                WorldClickIntent::OpenContainer {
                    x: 4,
                    y: 5,
                    z: 6,
                    block: BlockType::Chest,
                },
                "{topology:?}"
            );
        }
    }

    #[test]
    fn portal_and_eye_and_bed_shared() {
        let ignite = resolve_world_click(
            PresentationTopology::JoinClient,
            false,
            Some(hit(BlockType::Obsidian)),
            Item::FlintAndSteel,
            None,
            true,
            true,
        );
        let eye = resolve_world_click(
            PresentationTopology::Embedded,
            false,
            Some(hit(BlockType::EndPortalFrame)),
            Item::EyeOfEnder,
            None,
            true,
            true,
        );
        let sleep = resolve_world_click(
            PresentationTopology::JoinClient,
            false,
            Some(hit(BlockType::Bed)),
            Item::Air,
            None,
            true,
            true,
        );
        assert_eq!(
            ignite,
            WorldClickIntent::IgnitePortal {
                x: 4,
                y: 6,
                z: 6,
                face: [0, 1, 0],
            }
        );
        assert_eq!(
            eye,
            WorldClickIntent::InsertEnderEye {
                x: 4,
                y: 5,
                z: 6,
                face: [0, 1, 0],
            }
        );
        assert_eq!(sleep, WorldClickIntent::Sleep { x: 4, y: 5, z: 6 });
    }

    #[test]
    fn place_rejected_when_collision_blocks() {
        let intent = resolve_world_click(
            PresentationTopology::JoinClient,
            false,
            Some(stone_hit()),
            Item::Dirt,
            Some(BlockType::Dirt),
            true,
            false,
        );
        assert_eq!(intent, WorldClickIntent::Rejected);
    }

    #[test]
    fn authority_hit_prefers_merchant_over_slot() {
        let probe = InventoryHitProbe {
            merchant: Some(1),
            creative_tab: None,
            recipe_book_toggle: true,
            recipe_book: false,
            enchant: None,
            slot: Some(7u8),
        };
        assert_eq!(
            probe.authority_hit(),
            InventoryHit::Merchant { offer_index: 1 }
        );
        assert_eq!(probe.ui_hit(), InventoryHit::RecipeBookToggle);
    }

    #[test]
    fn merchant_offer_geometry_matches_legacy_rects() {
        assert_eq!(merchant_offer_at(0.0, 0.28, 3), Some(0));
        assert_eq!(merchant_offer_at(0.0, 0.19, 3), Some(1));
        assert_eq!(merchant_offer_at(0.9, 0.28, 3), None);
    }

    #[test]
    fn collect_inventory_ui_hits_covers_merchant_enchant_and_recipe_book() {
        let hits = collect_inventory_ui_hits(0.0, 0.28, true, true, 3, true, true);
        assert_eq!(hits.merchant, Some(0));
        assert!(!hits.recipe_book);
        assert_eq!(hits.enchant, None);

        let enchant = collect_inventory_ui_hits(0.3, 0.24, true, false, 0, false, true);
        assert_eq!(enchant.enchant, Some(0));
        assert!(enchant.merchant.is_none());
        assert!(!enchant.recipe_book);

        let recipe = collect_inventory_ui_hits(-0.7, 0.0, true, false, 0, true, false);
        assert!(recipe.recipe_book);
        assert!(recipe.merchant.is_none());
        assert!(recipe.enchant.is_none());

        let right_click = collect_inventory_ui_hits(0.0, 0.28, false, true, 3, true, true);
        assert!(right_click.merchant.is_none());
        assert!(!right_click.recipe_book);
        assert!(right_click.enchant.is_none());
    }
}
