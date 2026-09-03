//! Strongly typed packet definitions.
//!
//! Every packet is its own struct implementing [`Packet`]. There is
//! deliberately no per-state enum: an enum is sized to its largest variant,
//! which would make every small packet pay for the largest one once Play state
//! arrives. Dispatch is a hand-written match over `(state, id)` in the
//! networking layer.
//!
//! Both directions are implemented for every packet even when only one is used
//! by the server today. This is what lets the future Pyrite client share this
//! crate unchanged, and it makes every packet round-trip testable.

use bytes::{Buf, BufMut};

use crate::error::{Direction, ProtocolError, State};

pub mod handshake;
pub mod login;
pub mod status;

/// A single protocol packet.
///
/// Implementations encode and decode only the packet **body**. The length
/// prefix and packet ID VarInt are written by
/// [`crate::codec::PacketCodec`], which is the only place framing rules live.
pub trait Packet: Sized {
    /// The packet's ID within its state and direction.
    const ID: i32;

    /// The connection state this packet is valid in.
    const STATE: State;

    /// The direction this packet travels.
    const DIRECTION: Direction;

    /// Writes the packet body to `dst`.
    fn encode<B: BufMut>(&self, dst: &mut B) -> Result<(), ProtocolError>;

    /// Reads a packet body from `src`.
    ///
    /// Implementations must not assume `src` contains only this packet; they
    /// read exactly their own fields and leave the rest.
    fn decode<B: Buf>(src: &mut B) -> Result<Self, ProtocolError>;
}
