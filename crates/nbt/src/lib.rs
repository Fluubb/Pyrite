//! Clean-room implementation of NBT, the binary tree format Minecraft uses
//! for structured data.
//!
//! This crate deliberately knows nothing about the network protocol. NBT is
//! used both on the wire and in save files, which are unrelated domains, so
//! the dependency runs `pyrite-protocol -> pyrite-nbt` and never the reverse.
//!
//! Two root forms exist and they are not interchangeable. Since protocol 764
//! the network form omits the root compound's name; the file form keeps it.
//! Reading one as the other misaligns the whole document by the length of the
//! name, with no error at the point of the mistake, so the entry points are
//! separate functions rather than a flag.

pub mod error;
pub mod tag;

pub use error::{MAX_DEPTH, NbtError};
pub use tag::TagId;
