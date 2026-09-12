use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::Deserialize;
use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;

pub type PlayerId = u64;

/// Protocol v21 embeds `ContainerAction` in `GameplayOperation::Container`
/// (Open=0, Close=1) and keeps `protocol_version` only on handshake /
/// login / server-list packets. Older clients are rejected during handshake.
pub const PROTOCOL_VERSION: u32 = 21;

/// Transport frame cap and decode budget. `ConnectionReader` rejects a
/// length header above this before allocating a body; `Packet::decode`
/// uses the same limit so a crafted `Vec` length cannot reserve more
/// than the remaining slice / this cap.
pub const MAX_PACKET_SIZE: usize = 2 * 1024 * 1024;

thread_local! {
    static DECODE_FRAME_LEN: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn set_decode_frame_len(len: usize) {
    DECODE_FRAME_LEN.with(|cell| cell.set(len));
}

pub(super) struct DecodeFrameGuard;

impl Drop for DecodeFrameGuard {
    fn drop(&mut self) {
        DECODE_FRAME_LEN.with(|cell| cell.set(0));
    }
}

pub(super) fn decode_frame_budget() -> usize {
    DECODE_FRAME_LEN.with(|cell| {
        let len = cell.get();
        if len == 0 {
            MAX_PACKET_SIZE
        } else {
            len.min(MAX_PACKET_SIZE)
        }
    })
}

/// Decode `Vec<u8>` through bincode's byte-buf path so a claimed length
/// larger than the remaining slice is rejected before allocation. Honest
/// `Vec<u8>` seq encoding is the same wire layout (`u64` length + bytes).
pub(super) fn deserialize_bounded_bytes<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoundedBytesVisitor;

    impl<'de> Visitor<'de> for BoundedBytesVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte buffer no larger than the remaining frame")
        }

        fn visit_borrowed_bytes<E: de::Error>(self, value: &'de [u8]) -> Result<Self::Value, E> {
            self.visit_bytes(value)
        }

        fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<Self::Value, E> {
            if value.len() > decode_frame_budget() {
                return Err(E::invalid_length(value.len(), &self));
            }
            Ok(value.to_vec())
        }

        fn visit_byte_buf<E: de::Error>(self, value: Vec<u8>) -> Result<Self::Value, E> {
            if value.len() > decode_frame_budget() {
                return Err(E::invalid_length(value.len(), &self));
            }
            Ok(value)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let hint = seq.size_hint().unwrap_or(0);
            let budget = decode_frame_budget();
            if hint > budget {
                return Err(de::Error::invalid_length(hint, &self));
            }
            let mut values = Vec::with_capacity(hint);
            while let Some(byte) = seq.next_element::<u8>()? {
                if values.len() >= budget {
                    return Err(de::Error::invalid_length(values.len() + 1, &self));
                }
                values.push(byte);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_byte_buf(BoundedBytesVisitor)
}

pub(super) fn deserialize_bounded_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedVecVisitor<T> {
        marker: PhantomData<T>,
    }

    impl<'de, T: Deserialize<'de>> Visitor<'de> for BoundedVecVisitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a sequence no longer than the remaining frame")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let hint = seq.size_hint().unwrap_or(0);
            let budget = decode_frame_budget();
            if hint > budget {
                return Err(de::Error::invalid_length(hint, &self));
            }
            let mut values = Vec::with_capacity(hint);
            while let Some(value) = seq.next_element()? {
                if values.len() >= budget {
                    return Err(de::Error::invalid_length(values.len() + 1, &self));
                }
                values.push(value);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_seq(BoundedVecVisitor {
        marker: PhantomData,
    })
}

