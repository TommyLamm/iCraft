use crate::inventory::Item;
use super::decode::PlayerId;
use super::wire_types::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]

pub enum GameplayOperation {
    /// Canonical player-authored block ingress.  `StartBreak` latches an
    /// authority-side mining session, `CancelBreak` clears it, and `Place`
    /// applies one inventory-backed placement.  The held slot is an exact
    /// metadata identity proof; it is never trusted as an inventory mutation.
    BlockAction {
        action: BlockActionKind,
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
        hand: u8,
        held: Option<SessionSlotWire>,
        block: u32,
        look_milli: [i16; 3],
    },
    Container {
        action: ContainerAction,
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
    },
    /// Canonical container click. Open/close stay on `Container`.
    ContainerClick {
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
        is_left: bool,
        dragged: Option<ItemWire>,
    },
    ItemUse {
        item: u32,
        count: u16,
    },
    Combat {
        target: u64,
        action: u8,
    },
    Sleep {
        x: i32,
        y: i32,
        z: i32,
    },
    Trade {
        villager_id: u64,
        offer_index: u16,
    },
    Mount {
        entity_id: u64,
    },
    Command {
        command: String,
    },
    Fishing {
        /// 0 = cast, 1 = reel, 2 = cancel.
        action: u8,
        /// 0 = main hand, 1 = off hand.
        hand: u8,
        /// Normalized look vector in thousandths of one block.
        look_milli: [i16; 3],
    },
    FurnaceTakeOutput {
        x: i32,
        y: i32,
        z: i32,
        count: u16,
    },
    Craft {
        /// Personal 2x2 grid or crafting-table 3x3 grid.
        grid: u8,
        sources: [Option<SlotRefWire>; 9],
        station: Option<[i32; 3]>,
    },
    Enchant {
        x: i32,
        y: i32,
        z: i32,
        source: SlotRefWire,
        /// Zero-based enchantment offer index.
        option: u8,
    },
    Brew {
        /// 0 = start, 1 = cancel, 2 = take ready output.
        action: u8,
        x: i32,
        y: i32,
        z: i32,
        ingredient: Option<SlotRefWire>,
        bottles: [Option<SlotRefWire>; 3],
    },
    Anvil {
        x: i32,
        y: i32,
        z: i32,
        left: SlotRefWire,
        right: Option<SlotRefWire>,
        rename: String,
    },
    UseState {
        /// 0 = main hand, 1 = off hand.
        hand: u8,
        active: bool,
    },
    /// Place or pick up a water source against a solid slab or adjacent cell.
    /// `source` is an exact compare-and-replace reference to the selected hand
    /// inventory slot; the authority validates the face and range from the
    /// authenticated pose before applying either world or inventory state.
    FluidUse {
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
        hand: u8,
        source: SlotRefWire,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockActionKind {
    StartBreak,
    CancelBreak,
    Place,
    /// Use an exact Flint and Steel slot against an obsidian portal frame.
    IgnitePortal,
    /// Consume one exact Eye of Ender slot into an empty portal frame.
    InsertEnderEye,
    /// Begin server-timed travel while the authenticated pose is in a portal.
    EnterPortal,
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameplayRequest {
    pub request_id: RequestId,
    pub client_sequence: u64,
    /// Filled by the server after authentication.  A client-supplied value is
    /// never trusted for routing or authorization.
    pub session_id: PlayerId,
    pub dimension: u8,
    pub client_revision: u64,
    pub operation: GameplayOperation,
}

impl GameplayRequest {
    pub fn encoded_len(&self) -> usize {
        bincode::serialized_size(self).unwrap_or(u64::MAX) as usize
    }

    pub fn validate_bounds(&self) -> Result<(), RejectReason> {
        if self.encoded_len() > MAX_REQUEST_BYTES {
            return Err(RejectReason::QueueFull);
        }
        if self.dimension > 2 {
            return Err(RejectReason::InvalidDimension);
        }

        match &self.operation {
            GameplayOperation::BlockAction { x, y, z, .. }
            | GameplayOperation::Sleep { x, y, z }
            | GameplayOperation::Container { x, y, z, .. }
            | GameplayOperation::ContainerClick { x, y, z, .. }
            | GameplayOperation::FurnaceTakeOutput { x, y, z, .. }
            | GameplayOperation::Enchant { x, y, z, .. }
            | GameplayOperation::Brew { x, y, z, .. }
            | GameplayOperation::Anvil { x, y, z, .. } => {
                validate_coordinate(*x, *y, *z)?;
            }
            GameplayOperation::FluidUse { x, y, z, .. } => {
                validate_coordinate(*x, *y, *z)?;
            }
            GameplayOperation::Craft {
                station: Some([x, y, z]),
                ..
            } => validate_coordinate(*x, *y, *z)?,
            _ => {}
        }

        match &self.operation {
            GameplayOperation::BlockAction {
                action,
                face,
                hand,
                held,
                block,
                look_milli,
                ..
            } => {
                if *hand > 1
                    || face.iter().any(|component| !matches!(*component, -1..=1))
                    || face
                        .iter()
                        .map(|component| i16::from(*component).abs())
                        .sum::<i16>()
                        > 1
                    || !valid_look(*look_milli)
                {
                    return Err(RejectReason::InvalidState);
                }
                match action {
                    BlockActionKind::StartBreak => {
                        if *block != crate::world::BlockType::Air.to_wire() {
                            return Err(RejectReason::InvalidState);
                        }
                    }
                    BlockActionKind::CancelBreak => {
                        if held.is_some() || *block != crate::world::BlockType::Air.to_wire() {
                            return Err(RejectReason::InvalidState);
                        }
                    }
                    BlockActionKind::Place => {
                        let Some(held) = held else {
                            return Err(RejectReason::InvalidState);
                        };
                        held.validate_bounds()?;
                        if crate::world::BlockType::from_wire(*block)
                            .map_or(true, |block| block == crate::world::BlockType::Air)
                        {
                            return Err(RejectReason::InvalidState);
                        }
                    }
                    BlockActionKind::IgnitePortal | BlockActionKind::InsertEnderEye => {
                        let Some(held) = held else {
                            return Err(RejectReason::InvalidState);
                        };
                        held.validate_bounds()?;
                        let expected = match action {
                            BlockActionKind::IgnitePortal => crate::world::BlockType::Fire,
                            BlockActionKind::InsertEnderEye => {
                                crate::world::BlockType::EndPortalFrame
                            }
                            _ => unreachable!(),
                        };
                        if crate::world::BlockType::from_wire(*block) != Some(expected) {
                            return Err(RejectReason::InvalidState);
                        }
                    }
                    BlockActionKind::EnterPortal => {
                        if held.is_some()
                            || !face.iter().all(|component| *component == 0)
                            || !matches!(
                                crate::world::BlockType::from_wire(*block),
                                Some(
                                    crate::world::BlockType::NetherPortal
                                        | crate::world::BlockType::EndPortal
                                        | crate::world::BlockType::EndGateway
                                )
                            )
                        {
                            return Err(RejectReason::InvalidState);
                        }
                    }
                }
                if let Some(held) = held {
                    held.validate_bounds()?;
                }
            }
            GameplayOperation::Container { slot, .. } => {
                if *slot >= MAX_CONTAINER_SLOTS {
                    return Err(RejectReason::InvalidState);
                }
            }
            GameplayOperation::ContainerClick { slot, dragged, .. } => {
                if *slot >= MAX_CONTAINER_SLOTS {
                    return Err(RejectReason::InvalidState);
                }
                if let Some(item) = dragged {
                    item.validate_rich_bounds(true)?;
                }
            }
            GameplayOperation::ItemUse { item, count } => {
                validate_item_count(*item, *count)?;
                // Only food ItemUse is implemented; reject tools/weapons here so
                // they never reach sequencing or dispatch Unsupported.
                match Item::from_u32(*item) {
                    Some(item_kind) if item_kind.food_properties().is_some() => {}
                    Some(_) => return Err(RejectReason::InvalidState),
                    None => return Err(RejectReason::Malformed),
                }
            }
            GameplayOperation::Command { command } => {
                if command.len() > MAX_COMMAND_BYTES {
                    return Err(RejectReason::StringTooLong);
                }
                // `/respawn` is a lifecycle string, not `commands::parse`.
                if command.trim().eq_ignore_ascii_case("/respawn") {
                    return Ok(());
                }
                let parsed =
                    crate::commands::parse(command).map_err(|_| RejectReason::InvalidState)?;
                if matches!(
                    parsed.surface(),
                    crate::commands::CommandSurface::ConsoleOnly
                ) {
                    return Err(RejectReason::Unsupported);
                }
            }
            GameplayOperation::Fishing {
                action,
                hand,
                look_milli,
            } => {
                if *action > 2 || *hand > 1 || !valid_look(*look_milli) {
                    return Err(RejectReason::InvalidState);
                }
            }
            GameplayOperation::FurnaceTakeOutput { count, .. } => {
                if *count == 0 || *count > 64 {
                    return Err(RejectReason::InvalidState);
                }
            }
            GameplayOperation::Craft {
                grid,
                sources,
                station,
            } => {
                if !matches!(*grid, 2 | 3) || (*grid == 3 && station.is_none()) {
                    return Err(RejectReason::InvalidState);
                }
                let active = usize::from(*grid) * usize::from(*grid);
                if sources[..active].iter().all(Option::is_none)
                    || sources[active..].iter().any(Option::is_some)
                {
                    return Err(RejectReason::InvalidState);
                }
                validate_slot_refs(sources.iter().flatten())?;
            }
            GameplayOperation::Enchant { source, option, .. } => {
                if *option > 2 {
                    return Err(RejectReason::InvalidState);
                }
                source.validate_bounds()?;
            }
            GameplayOperation::Brew {
                action,
                ingredient,
                bottles,
                ..
            } => {
                if *action > 2 {
                    return Err(RejectReason::InvalidState);
                }
                if *action == 0 && (ingredient.is_none() || bottles.iter().all(Option::is_none)) {
                    return Err(RejectReason::InvalidState);
                }
                if *action > 0 && (ingredient.is_some() || bottles.iter().any(Option::is_some)) {
                    return Err(RejectReason::InvalidState);
                }
                if let Some(source) = ingredient {
                    source.validate_bounds()?;
                }
                validate_slot_refs(bottles.iter().flatten())?;
            }
            GameplayOperation::Anvil {
                left,
                right,
                rename,
                ..
            } => {
                if rename.len() > MAX_ANVIL_RENAME_BYTES {
                    return Err(RejectReason::StringTooLong);
                }
                left.validate_bounds()?;
                if let Some(source) = right {
                    source.validate_bounds()?;
                }
            }
            GameplayOperation::UseState { hand, .. } => {
                if *hand > 1 {
                    return Err(RejectReason::InvalidState);
                }
            }
            GameplayOperation::FluidUse {
                face, hand, source, ..
            } => {
                if *hand > 1
                    || face.iter().any(|component| !matches!(*component, -1..=1))
                    || face
                        .iter()
                        .map(|component| i16::from(*component).abs())
                        .sum::<i16>()
                        != 1
                {
                    return Err(RejectReason::InvalidState);
                }
                source.validate_bounds()?;
            }
            GameplayOperation::Combat { .. }
            | GameplayOperation::Sleep { .. }
            | GameplayOperation::Trade { .. }
            | GameplayOperation::Mount { .. } => {}
        }
        Ok(())
    }
}
