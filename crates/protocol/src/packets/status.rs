//! Server list ping packets.

use bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use crate::buf::{read_i64, read_string, write_i64, write_string};
use crate::error::{Direction, ProtocolError, State};
use crate::packets::Packet;
use crate::text::TextComponent;
use crate::version::{PROTOCOL_VERSION, VERSION_NAME};

/// Maximum length of the status response JSON string, in characters.
pub const MAX_STATUS_JSON_CHARS: usize = 32767;

/// Sent by the client to ask for the server's status.
///
/// Carries no fields. A client is permitted to skip this packet entirely and
/// send [`PingRequest`] straight after the handshake, so the server must not
/// require it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusRequest;

impl Packet for StatusRequest {
    const ID: i32 = 0x00;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, _dst: &mut B) -> Result<(), ProtocolError> {
        Ok(())
    }

    fn decode<B: Buf>(_src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self)
    }
}

/// The server's answer to [`StatusRequest`]: one JSON string.
///
/// Build the payload with [`StatusResponseJson`] rather than formatting the
/// string by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    /// The serialised [`StatusResponseJson`] document.
    pub json: String,
}

impl Packet for StatusResponse {
    const ID: i32 = 0x00;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_string(dst, &self.json);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            json: read_string(src, MAX_STATUS_JSON_CHARS)?,
        })
    }
}

/// A latency probe. The payload is opaque and must be echoed verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingRequest {
    /// Arbitrary client-chosen value, conventionally a millisecond timestamp.
    pub payload: i64,
}

impl Packet for PingRequest {
    const ID: i32 = 0x01;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_i64(dst, self.payload);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            payload: read_i64(src)?,
        })
    }
}

/// The echo of a [`PingRequest`].
///
/// The client subtracts its own send time from its receive time to display
/// latency, so the payload must be returned bit-for-bit and the response must
/// not be delayed by server-side work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PongResponse {
    /// The payload copied verbatim from the ping.
    pub payload: i64,
}

impl Packet for PongResponse {
    const ID: i32 = 0x01;
    const STATE: State = State::Status;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_i64(dst, self.payload);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            payload: read_i64(src)?,
        })
    }
}

/// The JSON document carried inside [`StatusResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponseJson {
    /// Version name and protocol number.
    pub version: VersionInfo,
    /// Player counts and the hover sample.
    pub players: PlayersInfo,
    /// The MOTD.
    pub description: TextComponent,
    /// Optional base64-encoded 64x64 PNG, prefixed `data:image/png;base64,`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    /// Whether the server requires cryptographically signed chat.
    #[serde(rename = "enforcesSecureChat")]
    pub enforces_secure_chat: bool,
}

impl StatusResponseJson {
    /// Builds a status document advertising this build's pinned version.
    pub fn new(description: TextComponent, max_players: i32, online_players: i32) -> Self {
        Self {
            version: VersionInfo {
                name: VERSION_NAME.to_owned(),
                protocol: PROTOCOL_VERSION,
            },
            players: PlayersInfo {
                max: max_players,
                online: online_players,
                sample: Vec::new(),
            },
            description,
            favicon: None,
            enforces_secure_chat: false,
        }
    }
}

/// The `version` object of the status document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Displayed to the client when its protocol number does not match.
    pub name: String,
    /// The protocol number the server speaks.
    pub protocol: i32,
}

/// The `players` object of the status document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayersInfo {
    /// Slot count shown to the client.
    pub max: i32,
    /// Currently connected players.
    pub online: i32,
    /// Names shown when hovering the player count. May be empty.
    #[serde(default)]
    pub sample: Vec<SamplePlayer>,
}

/// One entry in the hover sample list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplePlayer {
    /// The displayed name.
    pub name: String,
    /// The player's UUID in hyphenated string form.
    pub id: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    #[test]
    fn status_request_is_an_empty_body() {
        let mut buf = BytesMut::new();
        StatusRequest.encode(&mut buf).unwrap();
        assert!(buf.is_empty(), "status request carries no fields");

        let mut src = &buf[..];
        StatusRequest::decode(&mut src).unwrap();
    }

    #[test]
    fn packet_constants_match_the_specification() {
        assert_eq!(StatusRequest::ID, 0x00);
        assert_eq!(StatusRequest::DIRECTION, Direction::Serverbound);
        assert_eq!(StatusResponse::ID, 0x00);
        assert_eq!(StatusResponse::DIRECTION, Direction::Clientbound);
        assert_eq!(PingRequest::ID, 0x01);
        assert_eq!(PingRequest::DIRECTION, Direction::Serverbound);
        assert_eq!(PongResponse::ID, 0x01);
        assert_eq!(PongResponse::DIRECTION, Direction::Clientbound);

        for state in [
            StatusRequest::STATE,
            StatusResponse::STATE,
            PingRequest::STATE,
            PongResponse::STATE,
        ] {
            assert_eq!(state, State::Status);
        }
    }

    #[test]
    fn status_response_round_trips() {
        let original = StatusResponse {
            json: r#"{"version":{"name":"26.2","protocol":776}}"#.to_owned(),
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(StatusResponse::decode(&mut src).unwrap(), original);
        assert!(src.is_empty());
    }

    #[test]
    fn ping_and_pong_round_trip_every_payload_bit() {
        for payload in [0i64, 1, -1, i64::MAX, i64::MIN, 0x0123_4567_89ab_cdef] {
            let mut buf = BytesMut::new();
            PingRequest { payload }.encode(&mut buf).unwrap();
            assert_eq!(buf.len(), 8, "payload is a fixed-width long");

            let mut src = &buf[..];
            assert_eq!(PingRequest::decode(&mut src).unwrap().payload, payload);

            let mut buf = BytesMut::new();
            PongResponse { payload }.encode(&mut buf).unwrap();
            let mut src = &buf[..];
            assert_eq!(PongResponse::decode(&mut src).unwrap().payload, payload);
        }
    }

    #[test]
    fn status_json_serialises_the_documented_shape() {
        let status = StatusResponseJson::new(TextComponent::new("A Pyrite Server"), 100, 0);
        let value = serde_json::to_value(&status).unwrap();

        assert_eq!(value["version"]["name"], "26.2");
        assert_eq!(value["version"]["protocol"], 776);
        assert_eq!(value["players"]["max"], 100);
        assert_eq!(value["players"]["online"], 0);
        assert_eq!(value["description"]["text"], "A Pyrite Server");
        assert_eq!(value["enforcesSecureChat"], false);
        assert!(
            value.get("favicon").is_none(),
            "favicon is omitted when unset"
        );
    }

    #[test]
    fn status_json_round_trips() {
        let original = StatusResponseJson::new(TextComponent::new("Pyrite"), 20, 3);
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: StatusResponseJson = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.version.protocol, original.version.protocol);
        assert_eq!(decoded.players.max, 20);
        assert_eq!(decoded.players.online, 3);
        assert_eq!(decoded.description, original.description);
    }

    #[test]
    fn status_response_rejects_oversized_json_prefix() {
        let mut buf = BytesMut::new();
        // 32767 chars => 98301 byte cap; declare one byte more.
        crate::varint::write_varint(&mut buf, 98_302);
        let mut src = &buf[..];
        assert!(matches!(
            StatusResponse::decode(&mut src),
            Err(ProtocolError::StringTooLong { .. })
        ));
    }
}
