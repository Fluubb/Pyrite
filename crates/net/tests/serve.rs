//! Tests for admission control and shutdown draining.
//!
//! These are the controls that bound what an unauthenticated peer population
//! can make the server allocate, and until this file existed their only
//! evidence was that someone had read the code and believed it. Shutdown is
//! driven by an injected future rather than a signal, so none of this needs a
//! real Ctrl-C — which is awkward on Unix and genuinely difficult on Windows.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use pyrite_net::{ServeOptions, ServerConfig, serve};
use pyrite_protocol::text::TextComponent;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

fn config() -> Arc<ServerConfig> {
    Arc::new(ServerConfig {
        motd: TextComponent::new("A Pyrite Server"),
        max_players: 100,
        // Long enough that a connection never ends by timing out during a
        // test; the tests that need one to end close the client instead.
        read_timeout: Duration::from_secs(60),
        compression_threshold: None,
    })
}

/// Binds an ephemeral loopback port so tests never collide over a fixed one.
async fn listener() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (listener, addr)
}

/// Whether the server closed this socket without sending anything.
///
/// A refused connection is dropped rather than answered, so the peer sees an
/// immediate clean EOF.
async fn closed_immediately(stream: &mut TcpStream) -> bool {
    let mut byte = [0u8; 1];
    matches!(
        tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte)).await,
        Ok(Ok(0))
    )
}

#[tokio::test]
async fn connections_beyond_the_cap_are_refused() {
    let (listener, addr) = listener().await;
    let (stop, wait) = tokio::sync::oneshot::channel::<()>();

    let options = ServeOptions {
        max_connections: 1,
        shutdown_grace: Duration::from_secs(2),
    };
    let task = tokio::spawn(serve(listener, config(), options, async {
        let _ = wait.await;
    }));

    // First connection is admitted and held open, occupying the only permit.
    let first = TcpStream::connect(addr).await.unwrap();

    // Second is accepted at the TCP level but must be refused and dropped.
    let mut second = TcpStream::connect(addr).await.unwrap();
    assert!(
        closed_immediately(&mut second).await,
        "a connection beyond the cap must be dropped, not serviced"
    );

    drop(first);
    stop.send(()).unwrap();

    let outcome = task.await.unwrap();
    assert_eq!(outcome.accepted, 1, "only one connection fits the cap");
    assert_eq!(outcome.refused, 1, "the second must be counted as refused");
}

#[tokio::test]
async fn a_permit_is_reclaimed_when_a_connection_ends() {
    let (listener, addr) = listener().await;
    let (stop, wait) = tokio::sync::oneshot::channel::<()>();

    let options = ServeOptions {
        max_connections: 1,
        shutdown_grace: Duration::from_secs(2),
    };
    let task = tokio::spawn(serve(listener, config(), options, async {
        let _ = wait.await;
    }));

    // Occupy the only permit, then hang up. The connection task ends, which
    // is what releases the permit.
    let first = TcpStream::connect(addr).await.unwrap();
    drop(first);

    // The permit is released when the task completes, so retry briefly rather
    // than assuming an exact scheduling order.
    let mut admitted = false;
    for _ in 0..50 {
        let mut probe = TcpStream::connect(addr).await.unwrap();
        if closed_immediately(&mut probe).await {
            tokio::time::sleep(Duration::from_millis(20)).await;
            continue;
        }
        admitted = true;
        drop(probe);
        break;
    }

    stop.send(()).unwrap();
    let outcome = task.await.unwrap();

    assert!(
        admitted,
        "a permit must be reclaimed once its connection ends, or the cap \
         becomes a permanent ceiling rather than a concurrency limit"
    );
    assert!(outcome.accepted >= 2, "got {}", outcome.accepted);
}

#[tokio::test]
async fn shutdown_drains_connections_that_finish() {
    let (listener, addr) = listener().await;
    let (stop, wait) = tokio::sync::oneshot::channel::<()>();

    let task = tokio::spawn(serve(
        listener,
        config(),
        ServeOptions {
            max_connections: 10,
            shutdown_grace: Duration::from_secs(5),
        },
        async {
            let _ = wait.await;
        },
    ));

    // Connect and hang up, so the connection task is finished (or about to be)
    // by the time shutdown begins.
    let client = TcpStream::connect(addr).await.unwrap();
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;

    stop.send(()).unwrap();
    let outcome = task.await.unwrap();

    assert!(outcome.drained, "finished connections must drain");
    assert_eq!(outcome.abandoned, 0);
}

#[tokio::test]
async fn shutdown_abandons_connections_that_outlive_the_grace() {
    let (listener, addr) = listener().await;
    let (stop, wait) = tokio::sync::oneshot::channel::<()>();

    let task = tokio::spawn(serve(
        listener,
        config(),
        ServeOptions {
            max_connections: 10,
            // Deliberately short: the held connection sits on a 60s read
            // timeout, so it cannot possibly finish inside this window.
            shutdown_grace: Duration::from_millis(200),
        },
        async {
            let _ = wait.await;
        },
    ));

    // Hold a connection open and idle, so it is genuinely still in flight.
    let held = TcpStream::connect(addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    stop.send(()).unwrap();
    let outcome = task.await.unwrap();

    assert!(
        !outcome.drained,
        "a connection that cannot finish must not be reported as drained"
    );
    assert_eq!(
        outcome.abandoned, 1,
        "the abandoned count must reflect what was actually still running"
    );

    drop(held);
}
