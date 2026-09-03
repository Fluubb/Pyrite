//! Errors raised while servicing a connection.

use pyrite_protocol::{ProtocolError, State};

/// Everything that can go wrong on a single connection.
///
/// Every variant terminates only the connection that produced it. None of them
/// is fatal to the server.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// The peer sent something the codec could not decode.
    #[error("protocol error")]
    Protocol(#[from] ProtocolError),

    /// The underlying transport failed.
    #[error("i/o error")]
    Io(#[from] std::io::Error),

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

    /// The peer sent nothing for longer than the configured read timeout.
    #[error("connection timed out waiting for a packet")]
    Timeout,

    /// The peer closed the connection cleanly.
    #[error("connection closed by peer")]
    Closed,
}
