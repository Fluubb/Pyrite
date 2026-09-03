//! Clean-room implementation of the Minecraft-compatible network protocol.
//!
//! This crate is deliberately runtime-free: it depends on no async executor so
//! that it can be used from blocking contexts (fuzz targets, the future
//! client's worker threads) as well as from Tokio.
//!
//! Everything here is derived solely from open, publicly documented
//! reverse-engineering of the wire format. No proprietary bytecode, decompiled
//! source, or private mapping was consulted.

pub mod error;
pub mod varint;
pub mod version;

pub use error::{Direction, ProtocolError, State};
pub use version::{PROTOCOL_VERSION, VERSION_NAME};
