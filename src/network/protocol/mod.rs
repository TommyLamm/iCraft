//! Wire protocol types, bounded decode helpers, and packet codec.

mod decode;
mod gameplay;
mod packet;
mod wire_types;

#[cfg(test)]
mod tests;

pub use decode::{PlayerId, MAX_PACKET_SIZE, PROTOCOL_VERSION};
pub use gameplay::{BlockActionKind, GameplayOperation, GameplayRequest};
pub use packet::{Action, EntityStateWire, LightningStrike, Packet, PlayerEffectWire};
pub use wire_types::{
    ContainerAction, GameplayOutcome, GameplayResponse, ItemWire, MiningProgressWire, PotionWire,
    RejectReason, RequestId, ServerSequence, SessionBrewWire, SessionFishingHookWire,
    SessionGameplayWire, SessionSlotWire, SlotRefWire, MAX_ANVIL_RENAME_BYTES, MAX_BLOCK_COORDINATE,
    MAX_BLOCK_Y, MAX_COMMAND_BYTES, MAX_CONTAINER_SLOTS, MAX_REQUEST_BYTES, MAX_REQUEST_STRING_BYTES,
    MAX_SESSION_VELOCITY_MILLI, MIN_BLOCK_Y, SESSION_SLOT_COUNT,
};
