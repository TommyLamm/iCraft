use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::protocol::Packet;

const MAX_PACKET_SIZE: u32 = 2 * 1024 * 1024;
const LEN_HEADER: usize = 4;

pub struct Connection {
    stream: TcpStream,
    read_buf: Vec<u8>,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Self {
        Connection {
            stream,
            read_buf: Vec::new(),
        }
    }

    pub async fn recv(&mut self) -> io::Result<Packet> {
        let mut header = [0u8; LEN_HEADER];
        self.stream.read_exact(&mut header).await?;
        let len = u32::from_be_bytes(header);
        if len > MAX_PACKET_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("packet length {len} exceeds maximum {MAX_PACKET_SIZE}"),
            ));
        }
        let len = len as usize;
        self.read_buf.clear();
        self.read_buf.resize(len, 0);
        self.stream.read_exact(&mut self.read_buf).await?;
        Packet::decode(&self.read_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub async fn send(&mut self, packet: &Packet) -> io::Result<()> {
        let payload = packet.encode();
        let len = u32::try_from(payload.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "packet payload exceeds u32 length",
            )
        })?;
        self.stream.write_all(&len.to_be_bytes()).await?;
        self.stream.write_all(&payload).await?;
        self.stream.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::protocol::Action;

    #[tokio::test]
    async fn framed_roundtrip() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            conn.recv().await.unwrap()
        });

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let mut client = Connection::new(client_stream);

        let packet = Packet::Handshake {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            username: "steve".into(),
        };
        client.send(&packet).await.unwrap();

        let received = server_task.await.unwrap();
        assert_eq!(received, packet);
    }

    #[tokio::test]
    async fn bidirectional_multiple_packets() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            let first = conn.recv().await.unwrap();
            let second = conn.recv().await.unwrap();
            let echo = Packet::ChatMessage {
                protocol_version: crate::network::protocol::PROTOCOL_VERSION,
                sender: "server".into(),
                message: "pong".into(),
            };
            conn.send(&echo).await.unwrap();
            (first, second, echo)
        });

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let mut client = Connection::new(client_stream);

        let pos = Packet::PlayerPosition {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            id: 1,
            x: 12.5,
            y: 64.0,
            z: -7.25,
            yaw: 0.0,
            pitch: 30.0,
        };
        let act = Packet::PlayerAction {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            id: 1,
            action: Action::Break,
        };
        client.send(&pos).await.unwrap();
        client.send(&act).await.unwrap();
        let echoed = client.recv().await.unwrap();

        let (first, second, echo) = server_task.await.unwrap();
        assert_eq!(first, pos);
        assert_eq!(second, act);
        assert_eq!(echoed, echo);
    }
}
