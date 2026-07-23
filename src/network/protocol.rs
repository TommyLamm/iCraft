use serde::{Deserialize, Serialize};

pub type PlayerId = u64;

pub const PROTOCOL_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Place,
    Break,
    Use,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Packet {
    Handshake {
        protocol_version: u32,
        username: String,
    },
    LoginSuccess {
        protocol_version: u32,
        player_id: PlayerId,
        seed: u64,
        gamemode: u8,
    },
    Disconnect {
        protocol_version: u32,
        reason: String,
    },
    PlayerPosition {
        protocol_version: u32,
        id: PlayerId,
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    PlayerAction {
        protocol_version: u32,
        id: PlayerId,
        action: Action,
    },
    PlayerJoin {
        protocol_version: u32,
        id: PlayerId,
        username: String,
    },
    PlayerLeave {
        protocol_version: u32,
        id: PlayerId,
    },
    BlockChange {
        protocol_version: u32,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
    },
    ChunkData {
        protocol_version: u32,
        cx: i32,
        cz: i32,
        blocks: Vec<u8>,
    },
    TimeSync {
        protocol_version: u32,
        ticks: u64,
        weather: u8,
    },
    ChatMessage {
        protocol_version: u32,
        sender: String,
        message: String,
    },
    Keepalive {
        protocol_version: u32,
    },
}

impl Packet {
    pub fn protocol_version(&self) -> u32 {
        match self {
            Packet::Handshake {
                protocol_version, ..
            }
            | Packet::LoginSuccess {
                protocol_version, ..
            }
            | Packet::Disconnect {
                protocol_version, ..
            }
            | Packet::PlayerPosition {
                protocol_version, ..
            }
            | Packet::PlayerAction {
                protocol_version, ..
            }
            | Packet::PlayerJoin {
                protocol_version, ..
            }
            | Packet::PlayerLeave {
                protocol_version, ..
            }
            | Packet::BlockChange {
                protocol_version, ..
            }
            | Packet::ChunkData {
                protocol_version, ..
            }
            | Packet::TimeSync {
                protocol_version, ..
            }
            | Packet::ChatMessage {
                protocol_version, ..
            }
            | Packet::Keepalive { protocol_version } => *protocol_version,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("packet serialization is infallible")
    }

    pub fn decode(bytes: &[u8]) -> Result<Packet, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> u32 {
        PROTOCOL_VERSION
    }

    #[test]
    fn handshake_roundtrip() {
        let p = Packet::Handshake {
            protocol_version: v(),
            username: "steve".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn login_success_roundtrip() {
        let p = Packet::LoginSuccess {
            protocol_version: v(),
            player_id: 42,
            seed: 0xDEAD_BEEF_CAFE,
            gamemode: 1,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn disconnect_roundtrip() {
        let p = Packet::Disconnect {
            protocol_version: v(),
            reason: "kicked".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_position_roundtrip() {
        let p = Packet::PlayerPosition {
            protocol_version: v(),
            id: 7,
            sequence: 42,
            sender_time_millis: 12_345,
            x: 1.5,
            y: 64.0,
            z: -2.25,
            yaw: 90.0,
            pitch: -45.5,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_action_roundtrip() {
        let p = Packet::PlayerAction {
            protocol_version: v(),
            id: 7,
            action: Action::Place,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_join_roundtrip() {
        let p = Packet::PlayerJoin {
            protocol_version: v(),
            id: 99,
            username: "alex".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn player_leave_roundtrip() {
        let p = Packet::PlayerLeave {
            protocol_version: v(),
            id: 99,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn block_change_roundtrip() {
        let p = Packet::BlockChange {
            protocol_version: v(),
            x: -10,
            y: 64,
            z: 200,
            block: 12,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn chunk_data_roundtrip() {
        let p = Packet::ChunkData {
            protocol_version: v(),
            cx: -3,
            cz: 4,
            blocks: vec![0u8; 4096],
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn time_sync_roundtrip() {
        let p = Packet::TimeSync {
            protocol_version: v(),
            ticks: 18_500,
            weather: 2,
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn chat_message_roundtrip() {
        let p = Packet::ChatMessage {
            protocol_version: v(),
            sender: "steve".into(),
            message: "hi there".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn keepalive_roundtrip() {
        let p = Packet::Keepalive {
            protocol_version: v(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn version_mismatch_detectable() {
        let p = Packet::Handshake {
            protocol_version: 999,
            username: "old".into(),
        };
        let decoded = Packet::decode(&p.encode()).unwrap();
        assert_ne!(decoded.protocol_version(), PROTOCOL_VERSION);
    }

    #[test]
    fn invalid_bytes_rejected() {
        assert!(Packet::decode(&[0xFF; 3]).is_err());
    }
}
