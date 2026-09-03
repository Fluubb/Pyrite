//! The Pyrite server executable.
//!
//! Binds a TCP listener, accepts connections, and hands each one to a
//! [`Connection`] on its own Tokio task.

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use pyrite_net::{Connection, ServerConfig};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{PROTOCOL_VERSION, VERSION_NAME};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{Instrument, debug, error, info, info_span, warn};
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
    ///
    /// Must be at least 1: a zero timeout elapses immediately and would kill
    /// every connection before its first frame arrived.
    #[arg(
        long,
        env = "PYRITE_READ_TIMEOUT",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    read_timeout: u64,

    /// Maximum number of connections serviced at once.
    ///
    /// Each connection costs a task and its buffers, and peers control how
    /// many they open, so this is the ceiling on what an unauthenticated
    /// client population can make the server allocate.
    ///
    /// Expressed as `NonZeroUsize` so a zero -- which would refuse every
    /// connection -- is rejected at parse time rather than becoming a silent
    /// outage.
    #[arg(long, env = "PYRITE_MAX_CONNECTIONS", default_value = "1000")]
    max_connections: NonZeroUsize,

    /// Seconds to let in-flight connections finish after a shutdown signal.
    #[arg(long, env = "PYRITE_SHUTDOWN_GRACE", default_value_t = 5)]
    shutdown_grace: u64,
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

    // Bounds concurrent connections. A permit is acquired before the task is
    // spawned and released when the task ends, so the accept loop can admit a
    // new peer only once an existing one has finished.
    let permits = Arc::new(Semaphore::new(args.max_connections.get()));
    let mut connections = JoinSet::new();

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
                        // Reap finished tasks so the JoinSet does not grow
                        // without bound over the server's lifetime.
                        while connections.try_join_next().is_some() {}

                        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                            // At capacity. Drop the socket immediately rather
                            // than queueing: making the peer wait would let a
                            // backlog accumulate behind the cap, which is the
                            // resource growth the cap exists to prevent.
                            debug!(%peer, "refusing connection, at capacity");
                            drop(stream);
                            continue;
                        };

                        let config = Arc::clone(&config);
                        let span = info_span!("connection", %peer);
                        connections.spawn(
                            async move {
                                if let Err(error) = Connection::new(stream, config).run().await {
                                    // Routine peer behaviour -- idle timeouts,
                                    // resets, truncated frames from a client
                                    // that hung up -- is debug. Only genuine
                                    // protocol violations warrant a warning,
                                    // or any peer could inflate the log with a
                                    // single SYN.
                                    if error.is_transport_noise() {
                                        debug!(%error, "connection closed");
                                    } else {
                                        warn!(%error, "connection closed with an error");
                                    }
                                }
                                drop(permit);
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

    // Returning from `main` drops the runtime, which stops tasks at their next
    // await point rather than letting them finish. Drain explicitly instead,
    // under a bounded grace period so a stuck connection cannot hang shutdown.
    let outstanding = connections.len();
    if outstanding > 0 {
        info!(outstanding, "waiting for in-flight connections to finish");
        let grace = Duration::from_secs(args.shutdown_grace);
        match tokio::time::timeout(grace, async {
            while connections.join_next().await.is_some() {}
        })
        .await
        {
            Ok(()) => info!("all connections finished"),
            Err(_elapsed) => {
                warn!(
                    remaining = connections.len(),
                    "shutdown grace period expired, abandoning connections"
                );
            }
        }
    }

    Ok(())
}
