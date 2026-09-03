//! The single pinned protocol version this build targets.
//!
//! Pyrite targets exactly one protocol version at a time (design decision D3).
//! Changing the version the engine speaks means changing these two constants
//! and auditing the packet ID tables in [`crate::packets`].
//!
//! Verified 2026-09-03 against public reverse-engineering documentation of the
//! Java Edition protocol.

/// The numeric protocol version sent in the handshake and advertised in the
/// status response. Java Edition 26.2.
pub const PROTOCOL_VERSION: i32 = 776;

/// The human-readable version name shown in a client's server list when the
/// client's own protocol version does not match [`PROTOCOL_VERSION`].
pub const VERSION_NAME: &str = "26.2";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_version_matches_documented_constants() {
        // Guards against an accidental edit; these two must always change
        // together and must match what the status response advertises.
        assert_eq!(PROTOCOL_VERSION, 776);
        assert_eq!(VERSION_NAME, "26.2");
    }
}
