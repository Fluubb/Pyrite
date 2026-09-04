//! Length-prefixed packet framing.

use std::io::{Read, Write};

use bytes::{Buf, Bytes, BytesMut};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use tokio_util::codec::{Decoder, Encoder};

use crate::error::ProtocolError;
use crate::packets::Packet;
use crate::varint::{read_varint, read_varint_slice, varint_len, write_varint};

/// The largest packet body Pyrite will accept, in bytes.
///
/// A frame's length prefix is a VarInt; three bytes carry 21 payload bits, so
/// `2^21 - 1` is the largest length expressible in the conventional prefix
/// width. Anything larger is treated as a malformed or hostile peer.
pub const MAX_PACKET_SIZE: usize = 2_097_151;

/// Upper bound on how much capacity a single incomplete frame may speculatively
/// reserve.
///
/// A frame's declared length is peer-controlled, so growing the buffer to the
/// declared size would let three bytes of traffic cost megabytes of resident
/// memory. Reserving in bounded steps means a peer must actually send the bytes
/// it promised before the server commits memory to them.
const MAX_SPECULATIVE_RESERVE: usize = 8 * 1024;

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

    /// Reused compression scratch buffer, so an encode that compresses does
    /// not allocate a fresh output vector each time.
    compressed: Vec<u8>,
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

    /// Enables or disables compression.
    ///
    /// A negative threshold disables compression; zero or above enables it and
    /// sets the size at or above which packets are compressed.
    ///
    /// The caller must flush the `Set Compression` packet **before** calling
    /// this. That packet is itself sent uncompressed and the new format
    /// applies only to what follows it, so flipping first would emit a
    /// compressed `Set Compression` that no peer can parse.
    pub fn set_compression(&mut self, threshold: i32) {
        self.compression_threshold = if threshold < 0 { None } else { Some(threshold) };
    }

    /// Turns a compressed-mode frame body into a raw packet.
    ///
    /// Every bound here is checked before any allocation or inflation happens.
    /// The declared inflated size is peer-controlled, so a tiny payload can
    /// otherwise claim to expand to megabytes -- the decompression form of the
    /// amplification the framing decoder already guards against.
    fn decompress(&self, mut frame: Bytes, threshold: i32) -> Result<Bytes, ProtocolError> {
        let declared = read_varint(&mut frame)?;

        // Guard 1: a negative length is always a violation.
        let data_length =
            usize::try_from(declared).map_err(|_| ProtocolError::NegativeLength(declared))?;

        // Zero means the payload is a raw packet and no inflation is needed.
        if data_length == 0 {
            return Ok(frame);
        }

        // Guard 2: refuse an inflated size we would never accept as a frame.
        if data_length > MAX_PACKET_SIZE {
            return Err(ProtocolError::FrameTooLarge {
                len: data_length,
                max: MAX_PACKET_SIZE,
            });
        }

        // Guard 3: below the threshold it should have been sent uncompressed.
        if data_length < threshold as usize {
            return Err(ProtocolError::CompressedBelowThreshold {
                data_length,
                threshold,
            });
        }

        // Guard 4: read at most one byte more than declared. If the payload
        // inflates further, the extra byte makes the length check below fail
        // instead of the output growing unbounded.
        let mut out = Vec::with_capacity(data_length.min(MAX_SPECULATIVE_RESERVE));
        let mut decoder = ZlibDecoder::new(frame.as_ref()).take(data_length as u64 + 1);
        decoder
            .read_to_end(&mut out)
            .map_err(|error| ProtocolError::Decompression {
                reason: error.to_string(),
            })?;

        if out.len() != data_length {
            return Err(ProtocolError::CompressedSizeMismatch {
                declared: data_length,
                actual: out.len(),
            });
        }

        Ok(Bytes::from(out))
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
            // Reserve towards the shortfall, but never more than
            // `MAX_SPECULATIVE_RESERVE` at a time: `frame_len` is only a claim
            // the peer has made, not bytes it has actually sent. Capping the
            // reserve means memory tracks bytes received, not bytes promised,
            // so three bytes of traffic can no longer commit megabytes of
            // resident memory. `BytesMut` still grows further as real data
            // keeps arriving, so a large legitimate frame decodes correctly —
            // it just costs one reservation per 8 KiB instead of one giant
            // upfront reservation.
            src.reserve((frame_len - src.len()).min(MAX_SPECULATIVE_RESERVE));
            return Ok(None);
        }

        let mut frame = src.split_to(frame_len).freeze();
        frame.advance(prefix_len);

        let mut body = match self.compression_threshold {
            None => frame,
            Some(threshold) => self.decompress(frame, threshold)?,
        };

        // `read_varint` advances `body`, so what remains is exactly the packet
        // body.
        let id = read_varint(&mut body)?;

        Ok(Some(RawPacket { id, body }))
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

        let Some(threshold) = self.compression_threshold else {
            dst.reserve(len + 3);
            write_varint(dst, len as i32);
            dst.extend_from_slice(&self.scratch);
            return Ok(());
        };

        if len < threshold as usize {
            // Below the threshold: a zero data-length marks the payload as a
            // raw packet, so the peer skips inflation entirely.
            let payload_len = 1 + len;
            dst.reserve(payload_len + 3);
            write_varint(dst, payload_len as i32);
            write_varint(dst, 0);
            dst.extend_from_slice(&self.scratch);
            return Ok(());
        }

        self.compressed.clear();
        let mut encoder =
            ZlibEncoder::new(std::mem::take(&mut self.compressed), Compression::default());
        encoder.write_all(&self.scratch)?;
        self.compressed = encoder.finish()?;

        // The data-length field carries the *uncompressed* size, so the peer
        // knows how much to expect after inflating.
        let payload_len = varint_len(len as i32) + self.compressed.len();
        dst.reserve(payload_len + 3);
        write_varint(dst, payload_len as i32);
        write_varint(dst, len as i32);
        dst.extend_from_slice(&self.compressed);
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

    use crate::packets::status::PongResponse;

    /// Builds a codec with compression active at `threshold`.
    fn compressed_codec(threshold: i32) -> PacketCodec {
        let mut codec = PacketCodec::new();
        codec.set_compression(threshold);
        codec
    }

    #[test]
    fn set_compression_treats_a_negative_threshold_as_disabled() {
        let mut codec = PacketCodec::new();
        codec.set_compression(256);
        assert_eq!(codec.compression_threshold(), Some(256));
        codec.set_compression(-1);
        assert_eq!(codec.compression_threshold(), None);
    }

    #[test]
    fn a_small_packet_is_sent_uncompressed_with_a_zero_data_length() {
        // PongResponse is 9 bytes (1 id + 8 payload), well under the threshold.
        let mut codec = compressed_codec(256);
        let mut buf = BytesMut::new();
        codec.encode(PongResponse { payload: 7 }, &mut buf).unwrap();

        assert_eq!(buf[0], 10, "packet length = 1 data-length byte + 9 payload");
        assert_eq!(buf[1], 0x00, "data length 0 means uncompressed");
        assert_eq!(buf[2], 0x01, "the raw packet id follows immediately");
        assert_eq!(buf.len(), 11);
    }

    #[test]
    fn a_large_packet_is_compressed_and_round_trips() {
        let json = "x".repeat(4096);
        let mut codec = compressed_codec(256);
        let mut buf = BytesMut::new();
        codec
            .encode(StatusResponse { json: json.clone() }, &mut buf)
            .unwrap();

        assert!(
            buf.len() < 512,
            "a 4 KiB repetitive payload must compress well, got {} bytes",
            buf.len()
        );

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.id, StatusResponse::ID);
        let decoded: StatusResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.json, json);
        assert!(buf.is_empty());
    }

    #[test]
    fn a_small_packet_round_trips_through_the_uncompressed_path() {
        let mut codec = compressed_codec(256);
        let mut buf = BytesMut::new();
        codec
            .encode(PongResponse { payload: -1 }, &mut buf)
            .unwrap();

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        let decoded: PongResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.payload, -1);
    }

    #[test]
    fn a_compressed_frame_fed_one_byte_at_a_time_yields_one_packet() {
        let json = "y".repeat(4096);
        let mut encoder = compressed_codec(256);
        let mut full = BytesMut::new();
        encoder
            .encode(StatusResponse { json: json.clone() }, &mut full)
            .unwrap();

        let mut decoder = compressed_codec(256);
        let mut buf = BytesMut::new();
        let mut frames = 0;
        for byte in full.iter() {
            buf.extend_from_slice(&[*byte]);
            if let Some(frame) = decoder.decode(&mut buf).unwrap() {
                let decoded: StatusResponse = frame.decode_as().unwrap();
                assert_eq!(decoded.json, json);
                frames += 1;
            }
        }
        assert_eq!(frames, 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn a_declared_inflated_size_above_the_maximum_is_rejected() {
        // Guard 2. Refused from the header alone, before any inflation.
        let mut codec = compressed_codec(256);
        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, (MAX_PACKET_SIZE + 1) as i32);
        body.extend_from_slice(&[0x78, 0x9c, 0x00]);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn a_compressed_packet_below_the_threshold_is_rejected() {
        // Guard 3. Honouring it would spend our cpu on inflation that was
        // never permitted.
        let mut codec = compressed_codec(256);
        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, 10);
        body.extend_from_slice(&[0x78, 0x9c, 0x00]);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::CompressedBelowThreshold {
                data_length: 10,
                threshold: 256
            })
        ));
    }

    #[test]
    fn a_payload_that_inflates_to_the_wrong_size_is_rejected() {
        // Guard 4. Compress 1000 bytes but claim 900.
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&vec![0x41; 1000]).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, 900);
        body.extend_from_slice(&compressed);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        let mut codec = compressed_codec(256);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::CompressedSizeMismatch {
                declared: 900,
                actual: _
            })
        ));
    }

    #[test]
    fn a_negative_data_length_is_rejected() {
        // Guard 1.
        let mut body = BytesMut::new();
        crate::varint::write_varint(&mut body, -1);
        body.extend_from_slice(&[0x78, 0x9c, 0x00]);

        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, body.len() as i32);
        buf.extend_from_slice(&body);

        let mut codec = compressed_codec(256);
        assert!(matches!(
            codec.decode(&mut buf),
            Err(ProtocolError::NegativeLength(-1))
        ));
    }

    #[test]
    fn a_zero_threshold_compresses_everything() {
        let mut codec = compressed_codec(0);
        let mut buf = BytesMut::new();
        codec.encode(PongResponse { payload: 1 }, &mut buf).unwrap();

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        let decoded: PongResponse = frame.decode_as().unwrap();
        assert_eq!(decoded.payload, 1);
    }
}
