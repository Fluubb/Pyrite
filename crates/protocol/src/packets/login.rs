//! Login state packets.

use bytes::{Buf, BufMut};

use crate::buf::{
    read_prefixed_array, read_prefixed_optional, read_string, read_uuid, write_prefixed_array,
    write_prefixed_optional, write_string, write_uuid,
};
use crate::error::{Direction, ProtocolError, State};
use crate::packets::Packet;
use crate::text::TextComponent;
use crate::varint::{read_varint, write_varint};

/// Maximum length of the disconnect reason, in characters.
pub const MAX_DISCONNECT_REASON_CHARS: usize = 262_144;

/// Tells a client in [`State::Login`] why it is being refused, then the
/// connection closes.
///
/// The reason is a JSON text component encoded as a string. This differs from
/// the play-state disconnect packet, which carries a binary component instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginDisconnect {
    /// A JSON-encoded [`TextComponent`].
    pub reason: String,
}

impl LoginDisconnect {
    /// Builds a disconnect packet from a text component.
    pub fn from_component(reason: &TextComponent) -> Result<Self, ProtocolError> {
        Ok(Self {
            reason: serde_json::to_string(reason)?,
        })
    }
}

impl Packet for LoginDisconnect {
    const ID: i32 = 0x00;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_string(dst, &self.reason);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            reason: read_string(src, MAX_DISCONNECT_REASON_CHARS)?,
        })
    }
}

/// Maximum length of a username, in characters.
pub const MAX_USERNAME_CHARS: usize = 16;

/// Maximum length of a profile property's name, in characters.
const MAX_PROPERTY_NAME_CHARS: usize = 64;

/// Maximum length of a profile property's value, in characters.
const MAX_PROPERTY_VALUE_CHARS: usize = 32767;

/// Maximum length of a profile property's signature, in characters.
const MAX_SIGNATURE_CHARS: usize = 1024;

/// Maximum number of properties a game profile may carry.
///
/// Vanilla profiles carry at most a handful; the cap exists so a peer cannot
/// use the count to drive allocation.
const MAX_PROFILE_PROPERTIES: usize = 16;

/// One signed key/value pair attached to a player's profile, such as their
/// skin texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProperty {
    /// The property's name.
    pub name: String,
    /// The property's value.
    pub value: String,
    /// The signature over the value, when the property is signed.
    pub signature: Option<String>,
}

impl ProfileProperty {
    /// Writes this property's fields.
    fn encode_to<B: BufMut>(&self, dst: &mut B) {
        write_string(dst, &self.name);
        write_string(dst, &self.value);
        write_prefixed_optional(dst, self.signature.as_ref(), |dst, value| {
            write_string(dst, value)
        });
    }

    /// Reads one property's fields.
    fn decode_from<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            name: read_string(src, MAX_PROPERTY_NAME_CHARS)?,
            value: read_string(src, MAX_PROPERTY_VALUE_CHARS)?,
            signature: read_prefixed_optional(src, |src| read_string(src, MAX_SIGNATURE_CHARS))?,
        })
    }
}

/// A player's identity as the server reports it back to them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    /// The player's UUID.
    pub uuid: u128,
    /// The player's username.
    pub username: String,
    /// Signed profile properties. Empty in offline mode.
    pub properties: Vec<ProfileProperty>,
}

impl GameProfile {
    /// Writes this profile's fields.
    fn encode_to<B: BufMut>(&self, dst: &mut B) {
        write_uuid(dst, self.uuid);
        write_string(dst, &self.username);
        write_prefixed_array(dst, &self.properties, |dst, property| {
            property.encode_to(dst)
        });
    }

    /// Reads a profile's fields.
    fn decode_from<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            uuid: read_uuid(src)?,
            username: read_string(src, MAX_USERNAME_CHARS)?,
            properties: read_prefixed_array(
                src,
                MAX_PROFILE_PROPERTIES,
                ProfileProperty::decode_from,
            )?,
        })
    }
}

/// The first packet of the login sequence.
///
/// The UUID is supplied by the client and is **not** authenticated -- a client
/// may send any value -- so a server must never treat it as identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    /// The username the client wishes to log in as.
    pub name: String,
    /// The client's claimed UUID. Unauthenticated.
    pub uuid: u128,
}

impl Packet for LoginStart {
    const ID: i32 = 0x00;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_string(dst, &self.name);
        write_uuid(dst, self.uuid);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            name: read_string(src, MAX_USERNAME_CHARS)?,
            uuid: read_uuid(src)?,
        })
    }
}

/// Switches the connection to the compressed frame format.
///
/// This packet is itself sent uncompressed; the new format applies to
/// everything after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCompression {
    /// Size at or above which packets are compressed. Negative disables.
    pub threshold: i32,
}

impl Packet for SetCompression {
    const ID: i32 = 0x03;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        write_varint(dst, self.threshold);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            threshold: read_varint(src)?,
        })
    }
}

/// Confirms login and tells the client the identity it was granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    /// The profile the server assigned.
    pub profile: GameProfile,
    /// Session identifier for this login.
    pub session_id: u128,
}

impl Packet for LoginSuccess {
    const ID: i32 = 0x02;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Clientbound;

    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError> {
        self.profile.encode_to(dst);
        write_uuid(dst, self.session_id);
        Ok(())
    }

    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self {
            profile: GameProfile::decode_from(src)?,
            session_id: read_uuid(src)?,
        })
    }
}

/// The client's acknowledgement of [`LoginSuccess`].
///
/// Receiving it moves the connection into the configuration state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginAcknowledged;

impl Packet for LoginAcknowledged {
    const ID: i32 = 0x03;
    const STATE: State = State::Login;
    const DIRECTION: Direction = Direction::Serverbound;

    fn encode<B: BufMut>(&self, _dst: &mut B) -> Result<(), ProtocolError> {
        Ok(())
    }

    fn decode<B: Buf>(_src: &mut B) -> Result<Self, ProtocolError> {
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use bytes::BytesMut;

    #[test]
    fn login_disconnect_constants_match_the_specification() {
        assert_eq!(LoginDisconnect::ID, 0x00);
        assert_eq!(LoginDisconnect::STATE, State::Login);
        assert_eq!(LoginDisconnect::DIRECTION, Direction::Clientbound);
    }

    #[test]
    fn login_disconnect_round_trips() {
        let original = LoginDisconnect {
            reason: r#"{"text":"nope"}"#.to_owned(),
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginDisconnect::decode(&mut src).unwrap(), original);
    }

    #[test]
    fn from_component_produces_valid_json() {
        let packet =
            LoginDisconnect::from_component(&TextComponent::new("Login not implemented")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&packet.reason).unwrap();
        assert_eq!(value["text"], "Login not implemented");
    }

    fn sample_profile() -> GameProfile {
        GameProfile {
            uuid: 0x0123_4567_89ab_cdef_0123_4567_89ab_cdefu128,
            username: "Notch".to_owned(),
            properties: vec![ProfileProperty {
                name: "textures".to_owned(),
                value: "base64data".to_owned(),
                signature: Some("sig".to_owned()),
            }],
        }
    }

    #[test]
    fn login_packet_constants_match_the_specification() {
        assert_eq!(LoginStart::ID, 0x00);
        assert_eq!(LoginStart::DIRECTION, Direction::Serverbound);
        assert_eq!(LoginSuccess::ID, 0x02);
        assert_eq!(LoginSuccess::DIRECTION, Direction::Clientbound);
        assert_eq!(SetCompression::ID, 0x03);
        assert_eq!(SetCompression::DIRECTION, Direction::Clientbound);
        assert_eq!(LoginAcknowledged::ID, 0x03);
        assert_eq!(LoginAcknowledged::DIRECTION, Direction::Serverbound);

        for state in [
            LoginStart::STATE,
            LoginSuccess::STATE,
            SetCompression::STATE,
            LoginAcknowledged::STATE,
        ] {
            assert_eq!(state, State::Login);
        }
    }

    #[test]
    fn login_start_round_trips() {
        let original = LoginStart {
            name: "Notch".to_owned(),
            uuid: u128::MAX,
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginStart::decode(&mut src).unwrap(), original);
        assert!(src.is_empty());
    }

    #[test]
    fn login_start_encodes_name_then_uuid() {
        let mut buf = BytesMut::new();
        LoginStart {
            name: "ab".to_owned(),
            uuid: 1,
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf[0], 0x02, "string byte length");
        assert_eq!(&buf[1..3], b"ab");
        assert_eq!(buf.len(), 3 + 16, "name then a 16-byte uuid");
        assert_eq!(buf[buf.len() - 1], 0x01, "uuid is big-endian");
    }

    #[test]
    fn login_start_rejects_an_oversized_name() {
        let mut buf = BytesMut::new();
        crate::varint::write_varint(&mut buf, 49);
        let mut src = &buf[..];
        assert!(matches!(
            LoginStart::decode(&mut src),
            Err(ProtocolError::StringTooLong { len: 49, max: 48 })
        ));
    }

    #[test]
    fn set_compression_round_trips_including_negative() {
        for threshold in [-1i32, 0, 256, 2_097_151] {
            let mut buf = BytesMut::new();
            SetCompression { threshold }.encode(&mut buf).unwrap();
            let mut src = &buf[..];
            assert_eq!(
                SetCompression::decode(&mut src).unwrap(),
                SetCompression { threshold }
            );
        }
    }

    #[test]
    fn login_success_round_trips() {
        let original = LoginSuccess {
            profile: sample_profile(),
            session_id: 42,
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginSuccess::decode(&mut src).unwrap(), original);
        assert!(src.is_empty());
    }

    #[test]
    fn login_success_round_trips_with_no_properties_and_no_signature() {
        let original = LoginSuccess {
            profile: GameProfile {
                uuid: 7,
                username: "Steve".to_owned(),
                properties: Vec::new(),
            },
            session_id: 0,
        };
        let mut buf = BytesMut::new();
        original.encode(&mut buf).unwrap();
        let mut src = &buf[..];
        assert_eq!(LoginSuccess::decode(&mut src).unwrap(), original);
    }

    #[test]
    fn login_acknowledged_is_an_empty_body() {
        let mut buf = BytesMut::new();
        LoginAcknowledged.encode(&mut buf).unwrap();
        assert!(buf.is_empty());
        let mut src = &buf[..];
        LoginAcknowledged::decode(&mut src).unwrap();
    }
}
