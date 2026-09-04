//! The accept loop: admission control, per-connection tasks, and draining.
//!
//! This lives in `pyrite-net` rather than in the server binary so that it can
//! be tested. Shutdown arrives as an injected future rather than as a signal,
//! which means a test can trigger it directly instead of needing to deliver a
//! real Ctrl-C to a real process — something that is awkward on Unix and
//! genuinely difficult on Windows.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::config::ServerConfig;
use crate::connection::Connection;

/// How the accept loop is bounded.
#[derive(Debug, Clone)]
pub struct ServeOptions {
    /// Maximum connections serviced at once.
    ///
    /// Peers control how many sockets they open, so this is the ceiling on
    /// what an unauthenticated client population can make the server allocate.
    pub max_connections: usize,

    /// How long in-flight connections are given to finish after shutdown
    /// begins, before they are abandoned.
    pub shutdown_grace: Duration,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            shutdown_grace: Duration::from_secs(5),
        }
    }
}

/// What a completed [`serve`] run did.
///
/// Returned rather than only logged so that a test can assert on admission
/// control and draining without scraping log output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServeOutcome {
    /// Connections admitted and given a task.
    pub accepted: u64,
    /// Connections refused because the cap was already reached.
    pub refused: u64,
    /// Whether every in-flight connection finished within the grace period.
    pub drained: bool,
    /// Connections still running when the grace period expired.
    pub abandoned: usize,
}

/// Accepts connections until `shutdown` completes, then drains.
///
/// A permit is acquired before a task is spawned and released when that task
/// ends, so a new peer is admitted only once an existing one has finished.
/// At capacity the socket is dropped immediately rather than queued: making
/// the peer wait would let a backlog accumulate behind the cap, which is the
/// resource growth the cap exists to prevent.
pub async fn serve<S>(
    listener: TcpListener,
    config: Arc<ServerConfig>,
    options: ServeOptions,
    shutdown: S,
) -> ServeOutcome
where
    S: Future<Output = ()>,
{
    let permits = Arc::new(Semaphore::new(options.max_connections));
    let mut connections: JoinSet<()> = JoinSet::new();
    let mut accepted = 0u64;
    let mut refused = 0u64;

    // Pinned once, outside the loop. `select!` re-evaluates its branch
    // expressions every iteration, so a future built inline would be dropped
    // and rebuilt on each accept — and a shutdown signal landing in that
    // window would reach nothing.
    let mut shutdown = std::pin::pin!(shutdown);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        // Reap finished tasks so the JoinSet does not grow
                        // without bound over the server's lifetime.
                        while connections.try_join_next().is_some() {}

                        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                            refused += 1;
                            debug!(%peer, "refusing connection, at capacity");
                            drop(stream);
                            continue;
                        };

                        accepted += 1;
                        let config = Arc::clone(&config);
                        let span = info_span!("connection", %peer);
                        connections.spawn(
                            run_connection(stream, config, permit).instrument(span),
                        );
                    }
                    Err(error) => {
                        // A failed accept (a descriptor limit, for example)
                        // must never take the listener down with it.
                        error!(%error, "failed to accept a connection");
                    }
                }
            }
            () = &mut shutdown => {
                info!("shutdown signal received, stopping the listener");
                break;
            }
        }
    }

    let (drained, abandoned) = drain(&mut connections, options.shutdown_grace).await;

    ServeOutcome {
        accepted,
        refused,
        drained,
        abandoned,
    }
}

/// Services one connection and releases its permit when it ends.
async fn run_connection(
    stream: tokio::net::TcpStream,
    config: Arc<ServerConfig>,
    permit: OwnedSemaphorePermit,
) {
    if let Err(error) = Connection::new(stream, config).run().await {
        // Routine peer behaviour -- idle timeouts, resets, truncated frames
        // from a client that hung up -- is debug. Only genuine protocol
        // violations warrant a warning, or any peer could inflate the log with
        // a single SYN.
        if error.is_transport_noise() {
            debug!(%error, "connection closed");
        } else {
            warn!(%error, "connection closed with an error");
        }
    }
    drop(permit);
}

/// Waits for in-flight connections, bounded by `grace`.
///
/// Returns whether everything finished, and how many were abandoned. Dropping
/// the runtime instead would stop tasks at their next await point rather than
/// letting them finish, which is why this is explicit.
async fn drain(connections: &mut JoinSet<()>, grace: Duration) -> (bool, usize) {
    // Reap anything already finished so the count reported is live
    // connections, not stale handles left over since the last accept.
    while connections.try_join_next().is_some() {}

    let outstanding = connections.len();
    if outstanding == 0 {
        return (true, 0);
    }

    info!(outstanding, "waiting for in-flight connections to finish");

    let all_finished = tokio::time::timeout(grace, async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_ok();

    if all_finished {
        info!("all connections finished");
        (true, 0)
    } else {
        let remaining = connections.len();
        warn!(
            remaining,
            "shutdown grace period expired, abandoning connections"
        );
        (false, remaining)
    }
}
