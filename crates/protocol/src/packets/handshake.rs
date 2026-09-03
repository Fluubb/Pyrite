//! The handshake packet, sent once at the start of every connection.

use bytes::{Buf, BufMut};

use crate::buf::{read_string, read_u16, write_string, write_u16};
use crate::error::{Direction, ProtocolError, State};
use crate::packets::Packet;
use crate::varint::{read_varint, write_varint};

/// Maximum length of the server address field, in characters.
pub const MAX_SERVER_ADDRESS_CHARS: usize = 255;

/// The state a client asks to move into after the handshake.
///
/// Encoded as a VarInt. Any value other than 1, 2, or 3 is a protocol
/// violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum NextState {
    /// Server list ping. The connection is closed afterwards.
    Status = 1,
    /// Begin authentication and join the server.
    Login = 2,
    /// The client was transferred here by another server.
    Transfer = 3,
}

impl TryFrom<i32> for NextState {
    type Error = ProtocolError;

    fn try_from(value: i32) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Status),
            2 => Ok(Self::Login),
            3 => Ok(Self::Transfer),
            other => Err(ProtocolError::InvalidNextState(other)),
        }
    }
}

/// The first packet of every connection.
///
/// It is the only packet in [`State::Handshaking`], and it determines which
/// state the connection moves into next.
///
/// `server_address` and `server_port` record what hostname the client dialled.
/// They are informational — clients routinely send values rewritten by proxies
/// or SRV lookups — and must never be trusted for authorisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    /// The protocol version the client speaks. `-1` means the client is
    /// probing to discover the server's version.
    pub protocol_version: i32,
    /// The hostname or IP the client used to reach this server.
    pub server_address: String,
    /// The port the client connected to.
    pub server_port: u16,
    /// The state the client wants to enter next.
    pub next_state: NextState,
}

impl Packet for Handshake {
    const ID: i32 = 0x00;
    const STATE: State = State::Handshaking;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_varint(dst, self.protocol_version);
        write_string(dst, &self.server_address);
        write_u16(dst, self.server_port);
        write_varint(dst, self.next_state as i32);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        let protocol_version = read_varint(src)?;
        let server_address = read_string(src, MAX_SERVER_ADDRESS_CHARS)?;
        let server_port = read_u16(src)?;
        let next_state = NextState::try_from(read_varint(src)?)?;

        Ok(Self {
            protocol_version,
            server_address,
            server_port,
            next_state,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    fn sample() -> Handshake {
        Handshake {
            protocol_version: 776,
            server_address: "localhost".to_owned(),
            server_port: 25565,
            next_state: NextState::Status,
        }
    }

    #[test]
    fn handshake_round_trips() {
        let original = sample();
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();

        let mut src = &buf[..];
        let decoded = Handshake::decode(&mut src).unwrap();

        assert_eq!(decoded, original);
        assert!(src.is_empty(), "decode must consume the whole body");
    }

    #[test]
    fn handshake_encodes_expected_byte_layout() {
        let mut buf = BytesMut::new();
        sample().encode(&mut buf).unwrap();

        let expected: &[u8] = &[
            0x88, 0x06, // protocol version 776 as varint
            0x09, // server address length 9
            b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', //
            0x63, 0xdd, // port 25565, big-endian
            0x01, // next state 1 (status)
        ];
        assert_eq!(&buf[..], expected);
    }

    #[test]
    fn handshake_constants_match_the_specification() {
        assert_eq!(Handshake::ID, 0x00);
        assert_eq!(Handshake::STATE, State::Handshaking);
        assert_eq!(Handshake::DIRECTION, Direction::Serverbound);
    }

    #[test]
    fn next_state_accepts_all_three_documented_values() {
        for (raw, expected) in [
            (1, NextState::Status),
            (2, NextState::Login),
            (3, NextState::Transfer),
        ] {
            assert_eq!(NextState::try_from(raw).unwrap(), expected);
        }
    }

    #[test]
    fn next_state_rejects_undefined_values() {
        for raw in [0, 4, -1, 999] {
            assert!(matches!(
                NextState::try_from(raw),
                Err(ProtocolError::InvalidNextState(value)) if value == raw
            ));
        }
    }

    #[test]
    fn handshake_rejects_oversized_server_address() {
        // Declares a 1000-byte address; the cap is 255 chars => 765 bytes.
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, 776);
        crate::varint::write_varint(&mut buf, 1000);
        let mut src = &buf[..];
        assert!(matches!(
            Handshake::decode(&mut src),
            Err(ProtocolError::StringTooLong {
                len: 1000,
                max: 765
            })
        ));
    }
}
