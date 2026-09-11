//! Dual-write helpers for the two live session records.
//!
//! Authority `SessionContract` and runtime `PlayerSessionState` stay separate
//! types (interest / save codec cannot enter the deterministic core). These
//! helpers are the only production writers of pose (`PlayerData` + contract),
//! dimension (interest set + contract), game mode, and gameplay-projection
//! fields.

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
            if let Some(session) = self.players.get_mut(&id) {
                session.interest.invalidate_anchor();
            }
            if let Some(dimension) = self
                .players
                .get(&id)
                .map(|session| session.interest.dimension)
            {
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

    /// Write dimension to the authority contract and the runtime interest set.
    pub(super) fn sync_dimension(&mut self, id: u64, dimension: Dimension) {
        if let Some(session) = self.players.get_mut(&id) {
            session.interest.dimension = dimension;
        }
        if let Some(authority_session) = self.authority.session_mut(id) {
            authority_session.dimension = dimension as u8;
        }
    }

    /// Copy `SessionContract.game_mode` onto runtime `PlayerData`.
    ///
    /// Authority remains the source of truth for `/gamemode`, hardcore→spectator,
    /// and save restore. Join seeds the contract from `PlayerData`, then this
    /// helper keeps both records aligned on every later projection/save path.
    pub(super) fn sync_game_mode(&mut self, id: u64) {
        let Some(game_mode) = self.authority.session(id).map(|session| session.game_mode) else {
            return;
        };
        if let Some(session) = self.players.get_mut(&id) {
            session.data.game_mode = game_mode;
        }
    }

    /// Overlay the authority `SessionGameplayState` onto runtime `PlayerData`,
    /// including `game_mode` so mode changes never leave the save codec behind.
    pub(super) fn sync_gameplay_projection(&mut self, id: u64) {
        self.sync_game_mode(id);
        let Some(gameplay) = self.authority.session(id).map(|session| session.gameplay) else {
            return;
        };
        if let Some(session) = self.players.get_mut(&id) {
            apply_gameplay_to_player_data(&mut session.data, gameplay);
        }
    }
}
