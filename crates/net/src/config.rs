//! Runtime configuration for the listening server.

use std::time::Duration;

use pyrite_protocol::text::TextComponent;

/// Settings shared by every connection.
///
/// Held behind an `Arc` and never mutated after startup, so it costs one
/// pointer clone per connection rather than a copy.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The message of the day shown in a client's server list.
    pub motd: TextComponent,
    /// The slot count advertised to clients.
    pub max_players: i32,
    /// How long a connection may sit idle before it is dropped.
    pub read_timeout: Duration,
    /// Size at or above which packets are compressed, or `None` to disable
    /// compression entirely.
    pub compression_threshold: Option<i32>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            motd: TextComponent::new("A Pyrite Server"),
            max_players: 20,
            // A status ping completes in milliseconds. Anything idle for this
            // long is a stalled or hostile peer holding a task open, so it is
            // dropped rather than allowed to accumulate.
            read_timeout: Duration::from_secs(30),
            // The conventional default. Small packets stay uncompressed, so
            // the ping path pays nothing, while chunk data later will.
            compression_threshold: Some(256),
        }
    }
}
