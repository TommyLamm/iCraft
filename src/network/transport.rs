use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use super::protocol::Packet;

const MAX_PACKET_SIZE: u32 = 2 * 1024 * 1024;
const LEN_HEADER: usize = 4;

pub struct Connection {
    reader: ConnectionReader,
    writer: ConnectionWriter,
}

pub(super) struct ConnectionReader {
    stream: OwnedReadHalf,
    buf: Vec<u8>,
    frame_len: Option<usize>,
}

pub(super) struct ConnectionWriter {
    stream: OwnedWriteHalf,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Self {
        if let Err(error) = stream.set_nodelay(true) {
            eprintln!("[Network] Failed to enable TCP_NODELAY: {error}");
        }
        let (reader, writer) = stream.into_split();
        Self {
            reader: ConnectionReader {
                stream: reader,
                buf: Vec::new(),
                frame_len: None,
            },
            writer: ConnectionWriter { stream: writer },
        }
    }

    pub async fn recv(&mut self) -> io::Result<Packet> {
        self.reader.recv().await
    }

    pub async fn send(&mut self, packet: &Packet) -> io::Result<()> {
        self.writer.send(packet).await
    }

    pub(super) fn into_split(self) -> (ConnectionReader, ConnectionWriter) {
        (self.reader, self.writer)
    }
}

impl ConnectionReader {
    pub async fn recv(&mut self) -> io::Result<Packet> {
        loop {
            if self.frame_len.is_none() {
                while self.buf.len() < LEN_HEADER {
                    let mut tmp = [0u8; 4096];
                    let n = self.stream.read(&mut tmp).await?;
                    if n == 0 {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "early eof"));
                    }
                    self.buf.extend_from_slice(&tmp[..n]);
                }
                let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
                if len > MAX_PACKET_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("packet length {len} exceeds maximum {MAX_PACKET_SIZE}"),
                    ));
                }
                self.buf.drain(0..LEN_HEADER);
                self.frame_len = Some(len as usize);
            }

            let need = self.frame_len.unwrap();
            while self.buf.len() < need {
                let mut tmp = [0u8; 4096];
                let n = self.stream.read(&mut tmp).await?;
                if n == 0 {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "early eof"));
                }
                self.buf.extend_from_slice(&tmp[..n]);
            }

            let body: Vec<u8> = self.buf.drain(0..need).collect();
            self.frame_len = None;
            return Packet::decode(&body)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
}

impl ConnectionWriter {
    pub async fn send(&mut self, packet: &Packet) -> io::Result<()> {
        let payload = packet.encode();
        let len = u32::try_from(payload.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "packet payload exceeds u32 length",
            )
        })?;
        let mut frame = Vec::with_capacity(LEN_HEADER + payload.len());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(&payload);
        self.stream.write_all(&frame).await?;
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
        assert!(client.reader.stream.as_ref().nodelay().unwrap());

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
            sequence: 3,
            sender_time_millis: 150,
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

    #[tokio::test]
    async fn recv_survives_cancellation_between_header_and_body() {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();

        let packet = Packet::Handshake {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            username: "cancellation_test".into(),
        };
        let payload = packet.encode();
        let len = u32::try_from(payload.len()).unwrap();
        let len_bytes = len.to_be_bytes();

        let (reader_half, _writer_half) = client_stream.into_split();
        let mut reader = ConnectionReader {
            stream: reader_half,
            buf: Vec::new(),
            frame_len: None,
        };

        let mut server_stream = server_stream;
        server_stream.write_all(&len_bytes).await.unwrap();
        server_stream.flush().await.unwrap();

        tokio::select! {
            _ = reader.recv() => panic!("Should not complete yet"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }

        server_stream.write_all(&payload).await.unwrap();
        server_stream.flush().await.unwrap();

        let received = reader.recv().await.unwrap();
        assert_eq!(received, packet);
    }

    #[tokio::test]
    async fn recv_multiple_frames_in_one_segment() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let server_stream = listener.accept().await.unwrap().0;

        let mut client = Connection::new(client_stream);
        let mut server = Connection::new(server_stream);

        let p1 = Packet::Handshake {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            username: "p1".into(),
        };
        let p2 = Packet::Handshake {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            username: "p2".into(),
        };

        client.send(&p1).await.unwrap();
        client.send(&p2).await.unwrap();

        let r1 = server.recv().await.unwrap();
        let r2 = server.recv().await.unwrap();

        assert_eq!(r1, p1);
        assert_eq!(r2, p2);
    }
}
