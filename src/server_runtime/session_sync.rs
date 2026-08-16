//! Dual-write helpers for the two live session records.
//!
//! Authority `SessionContract` and runtime `PlayerSessionState` stay separate
//! types (interest / save codec cannot enter the deterministic core). These
//! helpers are the only production writers of the mirrored pose, dimension,
//! and gameplay-projection fields.

use super::{apply_gameplay_to_player_data, ServerRuntime};
use crate::dimension::Dimension;

impl ServerRuntime {
    /// Write pose to both the authority session and runtime `PlayerData`.
    /// Does not grant teleport allowance. Rebuilds interest when requested.
    pub(super) fn write_pose(
        &mut self,
        id: u64,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
        refresh_interest: bool,
    ) -> bool {
        let had_runtime = if let Some(session) = self.players.get_mut(&id) {
            session.data.position = position;
            session.data.yaw = yaw;
            session.data.pitch = pitch;
            true
        } else {
            false
        };
        if let Some(authority_session) = self.authority.session_mut(id) {
            authority_session.position = position;
            authority_session.yaw = yaw;
            authority_session.pitch = pitch;
        }
        if !had_runtime {
            return false;
        }
        if refresh_interest {
            if let Some(dimension) = self.players.get(&id).map(|session| session.dimension) {
                self.update_interest_for(id, dimension, position);
            }
        }
        true
    }

    /// Copy the authority pose onto runtime `PlayerData` (and back onto the
    /// authority fields so this stays the single writer).
    pub(super) fn sync_pose_from_authority(&mut self, id: u64, refresh_interest: bool) -> bool {
        let Some((position, yaw, pitch)) = self
            .authority
            .session(id)
            .map(|session| (session.position, session.yaw, session.pitch))
        else {
            return false;
        };
        self.write_pose(id, position, yaw, pitch, refresh_interest)
    }

    /// Write dimension to both the runtime session and the authority contract.
    pub(super) fn sync_dimension(&mut self, id: u64, dimension: Dimension) {
        if let Some(session) = self.players.get_mut(&id) {
            session.dimension = dimension;
        }
        if let Some(authority_session) = self.authority.session_mut(id) {
            authority_session.dimension = dimension as u8;
        }
    }

    /// Overlay the authority `SessionGameplayState` onto runtime `PlayerData`.
    pub(super) fn sync_gameplay_projection(&mut self, id: u64) {
        let Some(gameplay) = self.authority.session(id).map(|session| session.gameplay) else {
            return;
        };
        if let Some(session) = self.players.get_mut(&id) {
            apply_gameplay_to_player_data(&mut session.data, gameplay);
        }
    }
}
