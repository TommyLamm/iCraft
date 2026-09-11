//! Shared AuthorityCore request builders for headless integration tests.
//!
//! Stamps dimension + revision from the live session the same way
//! `tcp_harness::gameplay_request` does for ServerRuntime fixtures.

use icraft::authority::AuthorityCore;
use icraft::dimension::Dimension;
use icraft::network::protocol::{GameplayOperation, GameplayRequest};

/// Runtime-aware gameplay request using the live session dimension + revision.
pub fn authority_request(
    core: &AuthorityCore,
    session_id: u64,
    request_id: u128,
    client_sequence: u64,
    operation: GameplayOperation,
) -> GameplayRequest {
    let dimension = core
        .session(session_id)
        .and_then(|session| Dimension::from_wire(session.dimension))
        .expect("authority request session dimension");
    GameplayRequest {
        request_id,
        client_sequence,
        session_id,
        dimension: dimension as u8,
        client_revision: core.revision_for_dimension(dimension),
        operation,
    }
}
