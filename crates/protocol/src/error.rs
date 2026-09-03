//! Typed errors for every failure mode in protocol encoding and decoding.

use std::fmt;

/// The connection state a packet belongs to.
///
/// A connection begins in [`State::Handshaking`] and moves to exactly one of
/// [`State::Status`] or [`State::Login`] based on the handshake's next-state
/// field. `Configuration` and `Play` are declared because the state machine and
/// error type name them; no packets target them in Milestone 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// The initial state of every connection.
    Handshaking,
    /// Server list ping.
    Status,
    /// Authentication and connection setup.
    Login,
    /// Post-login negotiation of registries and resource packs.
    Configuration,
    /// In-world gameplay.
    Play,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Handshaking => "handshaking",
            Self::Status => "status",
            Self::Login => "login",
            Self::Configuration => "configuration",
            Self::Play => "play",
        };
        f.write_str(name)
    }
}

/// Which way a packet travels on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Client to server.
    Serverbound,
    /// Server to client.
    Clientbound,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Serverbound => "serverbound",
            Self::Clientbound => "clientbound",
        };
        f.write_str(name)
    }
}

/// Every way protocol encoding or decoding can fail.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// A VarInt or VarLong ran past its maximum encoded length, which means the
    /// peer sent a value that cannot fit the target integer type.
    #[error("varint exceeded its maximum encoded length of {max} bytes")]
    VarIntTooLong {
        /// The maximum number of bytes permitted for this integer width.
        max: usize,
    },

    /// The buffer ended in the middle of a field that was already committed to.
    #[error("unexpected end of buffer while decoding")]
    UnexpectedEof,

    /// A length prefix was negative. Lengths are VarInts and so can carry a
    /// negative bit pattern, which is always a protocol violation.
    #[error("negative length prefix: {0}")]
    NegativeLength(i32),

    /// A frame declared a length above the maximum a length VarInt can express.
    #[error("frame length {len} exceeds maximum {max}")]
    FrameTooLarge {
        /// The declared length.
        len: usize,
        /// The maximum permitted length.
        max: usize,
    },

    /// A string's byte length exceeded the cap for its field.
    #[error("string length {len} bytes exceeds maximum {max} bytes")]
    StringTooLong {
        /// The declared byte length.
        len: usize,
        /// The maximum permitted byte length.
        max: usize,
    },

    /// A string field did not contain valid UTF-8.
    #[error("string field was not valid utf-8")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    /// No packet is defined for this state, direction, and ID combination.
    #[error("unknown {direction} packet id {id:#04x} in state {state}")]
    UnknownPacket {
        /// The connection state the packet arrived in.
        state: State,
        /// The direction the packet travelled.
        direction: Direction,
        /// The packet ID that was not recognised.
        id: i32,
    },

    /// The handshake's next-state field was not 1, 2, or 3.
    #[error("invalid next state value {0}, expected 1 (status), 2 (login), or 3 (transfer)")]
    InvalidNextState(i32),

    /// A JSON payload failed to serialise or deserialise.
    #[error("json payload error")]
    Json(#[from] serde_json::Error),

    /// An underlying I/O error. Required because `tokio_util::codec::Decoder`
    /// mandates `From<std::io::Error>` on its error type.
    #[error("i/o error")]
    Io(#[from] std::io::Error),
}
