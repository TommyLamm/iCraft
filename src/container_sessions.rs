// Container session management for multiplayer chest interactions.
// Host-authoritative: each player has at most one open container session.
// Sessions track player_id, dimension, block position, and revision.
// Clicks are simulated on the host, committed atomically, and broadcast to viewers.

use crate::network::protocol::PlayerId;

#[derive(Debug, Clone)]
pub struct ContainerSession {
    pub player_id: PlayerId,
    pub dimension: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub revision: u64,
    pub is_double: bool,
}

impl ContainerSession {
    pub fn new(player_id: PlayerId, dimension: u8, x: i32, y: i32, z: i32) -> Self {
        Self {
            player_id,
            dimension,
            x,
            y,
            z,
            revision: 0,
            is_double: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct ContainerSessionManager {
    pub sessions: Vec<ContainerSession>,
}

impl ContainerSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }

    pub fn find_by_player(&self, player_id: PlayerId) -> Option<&ContainerSession> {
        self.sessions.iter().find(|s| s.player_id == player_id)
    }

    pub fn find_by_player_mut(&mut self, player_id: PlayerId) -> Option<&mut ContainerSession> {
        self.sessions.iter_mut().find(|s| s.player_id == player_id)
    }

    pub fn open(&mut self, player_id: PlayerId, dimension: u8, x: i32, y: i32, z: i32) -> bool {
        self.close_by_player(player_id);
        self.sessions
            .push(ContainerSession::new(player_id, dimension, x, y, z));
        true
    }

    /// Remove every session owned by `player_id` and return the removed
    /// records so callers can target lifecycle notifications precisely.
    pub fn close_by_player(&mut self, player_id: PlayerId) -> Vec<ContainerSession> {
        let affected: Vec<ContainerSession> = self
            .sessions
            .iter()
            .filter(|session| session.player_id == player_id)
            .cloned()
            .collect();
        self.sessions.retain(|s| s.player_id != player_id);
        affected
    }

    /// Remove one exact session. A stale close from an old coordinate or
    /// dimension must not terminate a newer session owned by the same player.
    pub fn close_exact(
        &mut self,
        player_id: PlayerId,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<ContainerSession> {
        let index = self.sessions.iter().position(|session| {
            session.player_id == player_id
                && session.dimension == dimension
                && session.x == x
                && session.y == y
                && session.z == z
        })?;
        Some(self.sessions.remove(index))
    }

    /// Remove sessions watching exactly this block in one dimension. Returning
    /// full records avoids accidentally sending a close for a same-coordinate
    /// session in another dimension. Callers that break a double chest must
    /// explicitly close both the primary and the verified partner coordinate.
    pub fn close_by_block(
        &mut self,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    ) -> Vec<ContainerSession> {
        let affected: Vec<ContainerSession> = self
            .sessions
            .iter()
            .filter(|s| s.dimension == dimension && s.x == x && s.y == y && s.z == z)
            .cloned()
            .collect();
        self.sessions
            .retain(|s| !(s.dimension == dimension && s.x == x && s.y == y && s.z == z));
        affected
    }

    /// Return the number of sessions watching a coordinate in one dimension.
    /// This is used for first-viewer/last-viewer chest state transitions.
    pub fn viewer_count(&self, dimension: u8, x: i32, y: i32, z: i32) -> usize {
        self.sessions
            .iter()
            .filter(|session| {
                session.dimension == dimension && session.x == x && session.y == y && session.z == z
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_session_open_close() {
        let mut manager = ContainerSessionManager::new();
        assert!(manager.open(1, 0, 10, 64, 20));
        assert!(manager.find_by_player(1).is_some());
        manager.close_by_player(1);
        assert!(manager.find_by_player(1).is_none());
    }

    #[test]
    fn container_session_close_by_block() {
        let mut manager = ContainerSessionManager::new();
        manager.open(1, 0, 10, 64, 20);
        manager.open(2, 0, 10, 64, 20);
        let affected = manager.close_by_block(0, 10, 64, 20);
        assert_eq!(affected.len(), 2);
        assert!(affected.iter().all(|session| session.dimension == 0));
        assert!(manager.find_by_player(1).is_none());
        assert!(manager.find_by_player(2).is_none());
    }

    #[test]
    fn container_session_close_by_block_is_dimension_scoped_and_exact() {
        let mut manager = ContainerSessionManager::new();
        manager.open(1, 0, 10, 64, 20);
        manager.open(2, 1, 10, 64, 20);
        let affected = manager.close_by_block(0, 10, 64, 20);
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].player_id, 1);
        assert!(manager.find_by_player(1).is_none());
        assert!(manager.find_by_player(2).is_some());

        assert!(manager.close_exact(2, 0, 10, 64, 20).is_none());
        assert!(manager.close_exact(2, 1, 10, 64, 20).is_some());
        assert!(manager.find_by_player(2).is_none());
    }

    #[test]
    fn container_session_close_by_block_does_not_close_adjacent_non_partner() {
        let mut manager = ContainerSessionManager::new();
        manager.open(1, 0, 10, 64, 20);
        manager.open(2, 0, 11, 64, 20);

        let affected = manager.close_by_block(0, 10, 64, 20);
        assert_eq!(
            affected
                .iter()
                .map(|session| session.player_id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert!(manager.find_by_player(1).is_none());
        assert!(manager.find_by_player(2).is_some());
    }

    #[test]
    fn container_session_double_chest_partner_requires_explicit_exact_close() {
        let mut manager = ContainerSessionManager::new();
        manager.open(1, 0, 10, 64, 20);
        manager.open(2, 0, 11, 64, 20);

        let primary = manager.close_by_block(0, 10, 64, 20);
        assert_eq!(primary.len(), 1);
        assert_eq!(primary[0].player_id, 1);
        assert!(manager.find_by_player(2).is_some());

        let partner = manager.close_by_block(0, 11, 64, 20);
        assert_eq!(partner.len(), 1);
        assert_eq!(partner[0].player_id, 2);
        assert!(manager.find_by_player(2).is_none());
    }
}
