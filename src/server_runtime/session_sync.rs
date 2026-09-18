//! Writers for the runtime session overlay beside authority `SessionContract`.
//!
//! Authority owns pose, dimension, game mode, username, and accepted sequence.
//! Runtime owns interest, Instant pose clocks, the save codec, and teleport
//! allowance. These helpers are the only production writers that keep interest
//! and the save codec aligned with the contract — they do not maintain a second
//! live copy of pose / name / dimension on `PlayerSessionState`.

use super::{apply_gameplay_to_player_data, ServerRuntime};
use crate::dimension::Dimension;

impl ServerRuntime {
    /// Write authoritative pose on the contract and refresh interest/clocks.
    /// Does not grant teleport allowance. Does not dual-write into `PlayerData`
    /// (save/projection overlays pose from the contract when needed).
    pub(super) fn write_pose(
        &mut self,
        id: u64,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
        refresh_interest: bool,
    ) -> bool {
        let had_runtime = if let Some(session) = self.players.get_mut(&id) {
            session.last_pose_position = position;
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
                session.player_dirty = true;
            }
            if let Some(dimension) = self
                .players
                .get(&id)
                .map(|session| session.interest.dimension)
            {
                self.update_interest_for(id, dimension, position);
            }
        } else if let Some(session) = self.players.get_mut(&id) {
            session.player_dirty = true;
        }
        true
    }

    /// Copy the authority pose onto the runtime pose clock and optionally
    /// refresh interest (still the single writer path for live pose).
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

    /// Write dimension onto the authority contract and the runtime interest set.
    /// Interest needs its own dimension key; this is not a third session mirror.
    pub(super) fn sync_dimension(&mut self, id: u64, dimension: Dimension) {
        if let Some(session) = self.players.get_mut(&id) {
            session.interest.dimension = dimension;
            session.player_dirty = true;
        }
        if let Some(authority_session) = self.authority.session_mut(id) {
            authority_session.dimension = dimension as u8;
        }
    }

    /// Copy `SessionContract.game_mode` onto the save-codec `PlayerData`.
    ///
    /// Authority remains the source of truth for `/gamemode`, hardcore→spectator,
    /// and save restore. This is a one-way overlay for persistence/projection.
    pub(super) fn sync_game_mode(&mut self, id: u64) {
        let Some(game_mode) = self.authority.session(id).map(|session| session.game_mode) else {
            return;
        };
        if let Some(session) = self.players.get_mut(&id) {
            session.data.game_mode = game_mode;
            session.player_dirty = true;
        }
    }

    /// Overlay authority gameplay (+ game_mode + pose) onto save-codec
    /// `PlayerData` for projection and persistence.
    pub(super) fn sync_gameplay_projection(&mut self, id: u64) {
        self.sync_game_mode(id);
        let Some((gameplay, position, yaw, pitch)) = self.authority.session(id).map(|session| {
            (
                session.gameplay,
                session.position,
                session.yaw,
                session.pitch,
            )
        }) else {
            return;
        };
        if let Some(session) = self.players.get_mut(&id) {
            apply_gameplay_to_player_data(&mut session.data, gameplay);
            session.data.position = position;
            session.data.yaw = yaw;
            session.data.pitch = pitch;
            session.last_pose_position = position;
            session.player_dirty = true;
        }
    }

    /// Set a view-distance override on the runtime session overlay and refresh
    /// interest if the effective distance changed. Remote sessions and default
    /// settings remain unaffected.
    pub fn set_session_view_distance(&mut self, id: u64, view_distance: u8) -> bool {
        let default_view = self.properties.view_distance;
        let view_distance = view_distance.clamp(2, 32);
        let Some(session) = self.players.get_mut(&id) else {
            return false;
        };
        let current_view = session.effective_view_distance(default_view);
        session.view_distance_override = Some(view_distance);
        if current_view == view_distance {
            return true;
        }
        let dimension = session.interest.dimension;
        let fallback_position = session.last_pose_position;
        let position = self
            .authority
            .session(id)
            .map(|authority| authority.position)
            .unwrap_or(fallback_position);
        self.update_interest_for(id, dimension, position);
        true
    }

    /// Set a view-distance override for the local embedded session.
    pub fn set_local_view_distance(&mut self, view_distance: u8) -> bool {
        let Some(local_id) = self.local_session_id else {
            return false;
        };
        self.set_session_view_distance(local_id, view_distance)
    }
}

