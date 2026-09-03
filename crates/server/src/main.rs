//! The Pyrite server executable.
//!
//! Binds a TCP listener, accepts connections, and hands each one to a
//! [`Connection`] on its own Tokio task.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use pyrite_net::{Connection, ServerConfig};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{PROTOCOL_VERSION, VERSION_NAME};
use tokio::net::TcpListener;
use tracing::{Instrument, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

/// Command line options.
#[derive(Debug, Parser)]
#[command(name = "pyrite-server", version, about = "The Pyrite server engine")]
struct Args {
    /// Address to listen on.
    #[arg(long, env = "PYRITE_BIND", default_value = "0.0.0.0:25565")]
    bind: SocketAddr,

    /// Message of the day shown in the client's server list.
    #[arg(long, env = "PYRITE_MOTD", default_value = "A Pyrite Server")]
    motd: String,

    /// Slot count advertised to clients.
    #[arg(long, env = "PYRITE_MAX_PLAYERS", default_value_t = 20)]
    max_players: i32,

    /// Seconds a connection may sit idle before it is dropped.
    #[arg(long, env = "PYRITE_READ_TIMEOUT", default_value_t = 30)]
    read_timeout: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Default to `info`; `RUST_LOG=pyrite_net=debug` turns on per-packet tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Arc::new(ServerConfig {
        motd: TextComponent::new(args.motd),
        max_players: args.max_players,
        read_timeout: Duration::from_secs(args.read_timeout),
    });

    let listener = TcpListener::bind(args.bind).await?;
    info!(
        address = %args.bind,
        version = VERSION_NAME,
        protocol = PROTOCOL_VERSION,
        "pyrite-server listening"
    );

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let config = Arc::clone(&config);
                        let span = info_span!("connection", %peer);
                        tokio::spawn(
                            async move {
                                if let Err(error) = Connection::new(stream, config).run().await {
                                    warn!(%error, "connection closed with an error");
                                }
                            }
                            .instrument(span),
                        );
                    }
                    Err(error) => {
                        // A failed accept (a descriptor limit, for example) must
                        // never take the listener down with it.
                        error!(%error, "failed to accept a connection");
                    }
                }
            }
            result = tokio::signal::ctrl_c() => {
                match result {
                    Ok(()) => info!("shutdown signal received, stopping the listener"),
                    Err(error) => error!(%error, "failed to listen for the shutdown signal"),
                }
                break;
            }
        }
    }

    Ok(())
}
