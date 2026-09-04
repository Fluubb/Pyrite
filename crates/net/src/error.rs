//! Errors raised while servicing a connection.

use pyrite_protocol::{ProtocolError, State};

/// Everything that can go wrong on a single connection.
///
/// Every variant terminates only the connection that produced it. None of them
/// is fatal to the server.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// The peer sent something the codec could not decode.
    ///
    /// Transport failures also arrive here, wrapped as
    /// [`ProtocolError::Io`], because the codec's error type is
    /// [`ProtocolError`] and it converts from [`std::io::Error`]. There is
    /// deliberately no separate `NetError::Io` variant: one would be
    /// unreachable, and having two spellings of the same condition is how a
    /// severity check ends up missing half its cases.
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    /// The peer tried to move between states in a way the protocol forbids.
    #[error("illegal state transition from {from} to {to}")]
    IllegalTransition {
        /// The state the connection was in.
        from: State,
        /// The state the peer tried to reach.
        to: State,
    },

    /// A packet arrived that is not valid in the current state, or arrived
    /// more times than permitted.
    #[error("unexpected packet id {id:#04x} in state {state}")]
    UnexpectedPacket {
        /// The state the connection was in.
        state: State,
        /// The offending packet ID.
        id: i32,
    },

    /// A second status request arrived on a connection that already answered
    /// one.
    ///
    /// Distinct from [`NetError::UnexpectedPacket`] because the packet ID is
    /// perfectly valid in this state — it simply may not arrive twice, and a
    /// log line saying "unexpected packet id 0x00 in state status" would
    /// misdescribe that.
    #[error("duplicate status request")]
    DuplicateStatusRequest,

    /// The client asked to log in under a name the server will not accept.
    #[error("invalid username {name:?}")]
    InvalidUsername {
        /// The rejected name.
        name: String,
    },

    /// The peer sent nothing for longer than the configured read timeout.
    #[error("connection timed out waiting for a packet")]
    Timeout,
}

impl NetError {
    /// Whether this error is ordinary transport noise rather than a protocol
    /// violation.
    ///
    /// An idle timeout, a reset, and a truncated frame from a peer that hung
    /// up mid-write are all routine on a public port: any host that connects
    /// and walks away produces one. Logging them at `warn` would let a single
    /// SYN buy a warning line, so they are logged at `debug` instead and only
    /// genuine protocol violations reach `warn`.
    pub fn is_transport_noise(&self) -> bool {
        matches!(self, Self::Timeout | Self::Protocol(ProtocolError::Io(_)))
    }
}
