//! Shared rejected Place fixture for authority / TCP hardening tests.
//!
//! Empty-hand (or wrong held) Place of DiamondOre must reject with
//! `InvalidState` and leave the world untouched.

use icraft::dimension::Dimension;
use icraft::network::protocol::{
    BlockActionKind, GameplayOperation, GameplayRequest, SessionSlotWire,
};
use icraft::world::BlockType;

/// Empty-hand Place that authority rejects as `InvalidState`.
pub fn rejected_place(
    session_id: u64,
    request_id: u128,
    client_sequence: u64,
    client_revision: u64,
) -> GameplayRequest {
    rejected_place_with_held(session_id, request_id, client_sequence, client_revision, None)
}

/// Place DiamondOre that authority rejects (wrong/empty held vs block wire).
pub fn rejected_place_with_held(
    session_id: u64,
    request_id: u128,
    client_sequence: u64,
    client_revision: u64,
    held: Option<SessionSlotWire>,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence,
        session_id,
        dimension: Dimension::Overworld as u8,
        client_revision,
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::Place,
            x: 8,
            y: 80,
            z: 8,
            face: [0, 1, 0],
            hand: 0,
            held,
            block: BlockType::DiamondOre.to_wire(),
            look_milli: [0, 0, 1000],
        },
    }
}
