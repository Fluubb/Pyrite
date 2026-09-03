//! Length-prefixed packet framing.

use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::ProtocolError;
use crate::packets::Packet;
use crate::varint::{read_varint, read_varint_slice, write_varint};

/// The largest packet body Pyrite will accept, in bytes.
///
/// A frame's length prefix is a VarInt; three bytes carry 21 payload bits, so
/// `2^21 - 1` is the largest length expressible in the conventional prefix
/// width. Anything larger is treated as a malformed or hostile peer.
pub const MAX_PACKET_SIZE: usize = 2_097_151;

/// A decoded frame: its packet ID and its still-encoded body.
///
/// `body` is a [`Bytes`] slice sharing the read buffer's allocation, so
/// framing does not copy the payload. Turn it into a typed packet with
/// [`RawPacket::decode_as`].
#[derive(Debug, Clone)]
pub struct RawPacket {
    /// The packet ID read from the front of the frame.
    pub id: i32,
    /// The remaining bytes of the frame, after the ID.
    pub body: Bytes,
}

impl RawPacket {
    /// Decodes this frame's body as packet type `P`.
    ///
    /// Returns [`ProtocolError::TrailingBytes`] if `P` does not consume the
    /// whole body, which indicates a malformed peer or a version mismatch
    /// rather than something safe to ignore.
    pub fn decode_as<P: Packet>(&self) -> Result<P, ProtocolError> {
        let mut body = self.body.clone();
        let packet = P::decode(&mut body)?;
        if body.has_remaining() {
            return Err(ProtocolError::TrailingBytes {
                remaining: body.remaining(),
            });
        }
        Ok(packet)
    }
}

/// Length-prefixed packet framing.
///
/// Uncompressed frame layout:
///
/// ```text
/// +------------------+------------------+------------------+
/// | VarInt length    | VarInt packet id | body             |
/// +------------------+------------------+------------------+
/// ```
///
/// `length` counts the packet ID plus the body, and does not count itself.
#[derive(Debug, Default)]
pub struct PacketCodec {
    /// Reused encode scratch buffer, so encoding a packet does not allocate
    /// once the connection has warmed up.
    scratch: BytesMut,

    /// Compression threshold, set once the server enables compression during
    /// login. `None` means every frame is sent uncompressed, which is the only
    /// mode Milestone 1 implements. This is a field rather than a stub so the
    /// codec is fully functional today.
    compression_threshold: Option<i32>,
}

impl PacketCodec {
    /// Creates a codec in uncompressed, unencrypted mode.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the active compression threshold, if compression is enabled.
    pub fn compression_threshold(&self) -> Option<i32> {
        self.compression_threshold
    }
}

impl Decoder for PacketCodec {
    type Item = RawPacket;
    type Error = ProtocolError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<RawPacket>, ProtocolError> {
        // Read the length prefix without consuming it: TCP delivers the prefix
        // in arbitrary fragments, and consuming a partial VarInt would
        // desynchronise the stream for good.
        let Some((declared, prefix_len)) = read_varint_slice(src)? else {
            return Ok(None);
        };

        let body_len =
            usize::try_from(declared).map_err(|_| ProtocolError::NegativeLength(declared))?;

        if body_len > MAX_PACKET_SIZE {
            return Err(ProtocolError::FrameTooLarge {
                len: body_len,
                max: MAX_PACKET_SIZE,
            });
        }

        let frame_len = prefix_len + body_len;
        if src.len() < frame_len {
            // Tell the buffer how much more we need so it grows once rather
            // than repeatedly as bytes trickle in.
            src.reserve(frame_len - src.len());
            return Ok(None);
        }

        let mut frame = src.split_to(frame_len).freeze();
        frame.advance(prefix_len);

        // `read_varint` advances `frame`, so what remains is exactly the body.
        let id = read_varint(&mut frame)?;

        Ok(Some(RawPacket { id, body: frame }))
    }
}

impl<P: Packet> Encoder<P> for PacketCodec {
    type Error = ProtocolError;

    fn encode(&mut self, item: P, dst: &mut BytesMut) -> Result<(), ProtocolError> {
        // The length prefix counts the ID and body, so both must be written
        // before the prefix can be known. The scratch buffer is reused across
        // calls to keep this allocation-free in steady state.
        self.scratch.clear();
        write_varint(&mut self.scratch, P::ID);
        item.encode(&mut self.scratch)?;

        let len = self.scratch.len();
        if len > MAX_PACKET_SIZE {
            return Err(ProtocolError::FrameTooLarge {
                len,
                max: MAX_PACKET_SIZE,
            });
        }

        dst.reserve(len + 3);
        write_varint(dst, len as i32);
        dst.extend_from_slice(&self.scratch);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::packets::handshake::{Handshake, NextState};
    use crate::packets::status::{PingRequest, StatusResponse};

    fn encoded_handshake() -> BytesMut {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        codec
            .encode(
                Handshake {
                    protocol_version: 776,
                    server_address: "localhost".to_owned(),
                    server_port: 25565,
                    next_state: NextState::Status,
                },
                &mut buf,
            )
            .unwrap();
        buf
    }

    #[test]
    fn encoded_frame_is_length_then_id_then_body() {
        let buf = encoded_handshake();
        // Body is 15 bytes (see the handshake byte-layout test) and the id
        // varint is 1 byte, so the length prefix is 16.
        assert_eq!(buf[0], 16);
        assert_eq!(buf[1], 0x00, "packet id");
        assert_eq!(buf.len(), 17);
    }

    #[test]
    fn decodes_a_complete_frame() {
        let mut codec = PacketCodec::new();
        let mut buf = encoded_handshake();
        let packet = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(packet.id, 0x00);
        assert!(buf.is_empty(), "the frame is fully consumed");

        let handshake: Handshake = packet.decode_as().unwrap();
        assert_eq!(handshake.server_address, "localhost");
        assert_eq!(handshake.next_state, NextState::Status);
    }

    #[test]
    fn returns_none_for_every_partial_prefix() {
        let full = encoded_handshake();
        let mut codec = PacketCodec::new();

        for split in 0..full.len() {
            let mut partial = BytesMut::from(&full[..split]);
            assert_eq!(
                codec.decode(&mut partial).unwrap().map(|p| p.id),
                None,
                "a {split}-byte prefix must decode to None"
            );
            assert_eq!(
                partial.len(),
                split,
                "an incomplete frame must not consume bytes"
            );
        }
    }

    #[test]
    fn feeding_one_byte_at_a_time_yields_exactly_one_frame() {
        let full = encoded_handshake();
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        let mut frames = 0;

        for byte in full.iter() {
            buf.extend_from_slice(&[*byte]);
            if codec.decode(&mut buf).unwrap().is_some() {
                frames += 1;
            }
        }

        assert_eq!(frames, 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn decodes_two_frames_from_one_buffer() {
        let mut codec = PacketCodec::new();
        let mut buf = encoded_handshake();
        let second = encoded_handshake();
        buf.extend_from_slice(&second);

        assert!(codec.decode(&mut buf).unwrap().is_some());
        assert!(codec.decode(&mut buf).unwrap().is_some());
        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert!(buf.is_empty());
    }

    #[test]
    fn rejects_a_length_prefix_above_the_maximum() {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, (MAX_PACKET_SIZE + 1) as i32);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn rejects_a_negative_length_prefix() {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, -1);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::NegativeLength(-1))
        ));
    }

    #[test]
    fn round_trips_a_clientbound_packet() {
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        codec
            .encode(
                StatusResponse {
                    json: r#"{"text":"x"}"#.to_owned(),
                },
                &mut buf,
            )
            .unwrap();

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.id, StatusResponse::ID);
        let decoded: StatusResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.json, r#"{"text":"x"}"#);
    }

    #[test]
    fn decode_as_rejects_trailing_bytes() {
        // A body longer than the packet's fields indicates a desynchronised
        // stream or a malformed peer; it must not be silently ignored.
        let mut codec = PacketCodec::new();
        let mut buf = BytesMut::new();
        codec.encode(PingRequest { payload: 7 }, &mut buf).unwrap();
        // Rewrite the frame with one extra body byte.
        let mut tampered = BytesMut::new();
        crate::varint::write_varint(&mut tampered, 10);
        tampered.extend_from_slice(&buf[1..]);
        tampered.extend_from_slice(&[0xaa]);

        let frame = codec.decode(&mut tampered).unwrap().unwrap();
        assert!(matches!(
            frame.decode_as::<PingRequest>(),
            Err(ProtocolError::TrailingBytes { remaining: 1 })
        ));
    }
}
