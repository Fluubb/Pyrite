//! Login state packets.

use bytes::{Buf, BufMut};

use crate::buf::{read_string, write_string};
use crate::error::{Direction, ProtocolError, State};
use crate::packets::Packet;
use crate::text::TextComponent;

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
}
