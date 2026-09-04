//! Connection lifecycle management for the Pyrite server engine.

pub mod config;
pub mod connection;
pub mod error;
pub mod offline;
pub mod server;
pub mod state;

pub use config::ServerConfig;
pub use connection::Connection;
pub use error::NetError;
pub use offline::{offline_uuid, validate_username};
pub use server::{ServeOptions, ServeOutcome, serve};
pub use state::ConnectionState;
