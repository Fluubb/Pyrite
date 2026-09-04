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

    /// A prefixed array declared more elements than its field permits.
    #[error("array length {len} exceeds maximum {max}")]
    ArrayTooLong {
        /// The declared element count.
        len: usize,
        /// The maximum permitted count.
        max: usize,
    },

    /// A boolean field held a byte other than 0 or 1.
    #[error("invalid boolean byte {0:#04x}, expected 0 or 1")]
    InvalidBoolean(u8),

    /// A compressed packet inflated to a different size than it declared.
    #[error("compressed packet declared {declared} bytes but inflated to {actual}")]
    CompressedSizeMismatch {
        /// The size the peer said the payload would inflate to.
        declared: usize,
        /// The size it actually inflated to.
        actual: usize,
    },

    /// A compressed payload could not be inflated.
    ///
    /// Distinct from [`ProtocolError::Io`] on purpose. A corrupt deflate
    /// stream is reported by the decompressor as an `io::Error`, but it is a
    /// protocol violation by the peer, not a transport failure -- and the
    /// networking layer classifies log severity on that distinction. Letting
    /// it reach `Io` would file hostile traffic as routine noise.
    #[error("decompression failed: {reason}")]
    Decompression {
        /// What the decompressor reported.
        reason: String,
    },

    /// An NBT document was malformed.
    ///
    /// Its own variant rather than folded into [`ProtocolError::Io`]: a
    /// malformed document is a protocol violation by the peer and must be
    /// logged as one. Filing it under a transport variant would classify it
    /// as routine noise and hide it at the default log level.
    #[error("nbt error: {0}")]
    Nbt(#[from] pyrite_nbt::NbtError),

    /// A packet was compressed even though it is below the threshold at which
    /// compression is permitted.
    #[error("compressed packet of {data_length} bytes is below the {threshold} byte threshold")]
    CompressedBelowThreshold {
        /// The declared uncompressed size.
        data_length: usize,
        /// The active compression threshold.
        threshold: i32,
    },

    /// A string field did not contain valid UTF-8.
    #[error("string field was not valid utf-8")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    /// The handshake's next-state field was not 1, 2, or 3.
    #[error("invalid next state value {0}, expected 1 (status), 2 (login), or 3 (transfer)")]
    InvalidNextState(i32),

    /// A JSON payload failed to serialise or deserialise.
    #[error("json payload error: {0}")]
    Json(#[from] serde_json::Error),

    /// An underlying I/O error. Required because `tokio_util::codec::Decoder`
    /// mandates `From<std::io::Error>` on its error type.
    ///
    /// **This variant is treated as routine transport noise and logged at
    /// `debug`.** Only genuine transport failures belong here. Anything that
    /// merely *reports itself* as an `io::Error` -- a decompressor, a parser,
    /// a future codec -- must get its own variant, or a peer's misbehaviour
    /// will be filed as a hung-up socket and never surface in the logs.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// A packet body contained more bytes than the packet's fields consume.
    #[error("packet body had {remaining} trailing bytes")]
    TrailingBytes {
        /// How many bytes were left over.
        remaining: usize,
    },
}
