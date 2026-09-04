//! Tests for the controls that keep a hostile or absent peer from costing the
//! server more than it costs the peer.
//!
//! The server list ping tests cover the happy path. These cover what a peer can
//! do when it is not trying to be a client: declare a frame it never sends, sit
//! idle holding a task open, or repeat a packet it already sent.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use futures_util::SinkExt;
use pyrite_net::{Connection, NetError, ServerConfig};
use pyrite_protocol::codec::PacketCodec;
use pyrite_protocol::packets::handshake::{Handshake, NextState};
use pyrite_protocol::packets::status::StatusRequest;
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{PROTOCOL_VERSION, varint};
use tokio_util::codec::{Decoder, Framed};

fn config() -> Arc<ServerConfig> {
    Arc::new(ServerConfig {
        motd: TextComponent::new("A Pyrite Server"),
        max_players: 100,
        read_timeout: Duration::from_secs(30),
        compression_threshold: None,
    })
}

fn handshake(next_state: NextState) -> Handshake {
    Handshake {
        protocol_version: PROTOCOL_VERSION,
        server_address: "localhost".to_owned(),
        server_port: 25565,
        next_state,
    }
}

/// A peer that connects, handshakes, and then goes silent must be dropped once
/// the read timeout elapses rather than holding its task indefinitely.
///
/// Time is paused, so this asserts the timeout actually fires without spending
/// thirty seconds of wall clock or depending on scheduling luck.
#[tokio::test(start_paused = true)]
async fn an_idle_peer_is_dropped_when_the_read_timeout_elapses() {
    let (client, server) = tokio::io::duplex(4096);
    let config = config();
    let timeout = config.read_timeout;
    let task = tokio::spawn(Connection::new(server, config).run());

    let mut client = Framed::new(client, PacketCodec::new());
    client.send(handshake(NextState::Status)).await.unwrap();

    // Say nothing further. Hold the client end open so the connection cannot
    // end by EOF -- only the timeout can close it.
    tokio::time::advance(timeout + Duration::from_secs(1)).await;

    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(NetError::Timeout)),
        "an idle peer must time out, got {result:?}"
    );
}

/// A peer that declares the largest permissible frame and then sends nothing
/// must not make the server allocate for it.
///
/// This is the regression test for the decoder's speculative reserve. Before
/// the fix, `src.reserve(frame_len - src.len())` grew the buffer to the full
/// declared length, so these three bytes took capacity from 3 to 2_097_154 --
/// roughly 699_000x amplification, held for the whole read timeout, on a
/// listener that binds 0.0.0.0 by default.
#[test]
fn a_declared_frame_the_peer_never_sends_does_not_allocate() {
    /// A VarInt declaring `MAX_PACKET_SIZE` (2_097_151), and nothing else.
    const LYING_PREFIX: &[u8] = &[0xff, 0xff, 0x7f];

    /// Generous enough not to be brittle, far below the ~2 MiB the unbounded
    /// reserve produced.
    const CEILING: usize = 16 * 1024;

    let mut codec = PacketCodec::new();
    let mut src = BytesMut::from(LYING_PREFIX);

    let frame = codec.decode(&mut src).unwrap();

    assert!(frame.is_none(), "an incomplete frame must not decode");
    assert_eq!(src.len(), LYING_PREFIX.len(), "no bytes may be consumed");
    assert!(
        src.capacity() <= CEILING,
        "three bytes must not commit {} bytes of capacity",
        src.capacity()
    );
}

/// Bounding the speculative reserve must not cap how large a legitimate frame
/// can be -- the buffer still grows as real bytes arrive, just in steps.
#[test]
fn a_large_frame_still_decodes_when_the_peer_actually_sends_it() {
    /// Comfortably larger than the 8 KiB reserve step, so the decoder has to
    /// grow the buffer several times to receive it.
    const BODY_LEN: usize = 64 * 1024;

    let mut body = BytesMut::new();
    varint::write_varint(&mut body, 0x00);
    body.extend_from_slice(&vec![0xab; BODY_LEN]);

    let mut wire = BytesMut::new();
    varint::write_varint(&mut wire, body.len() as i32);
    wire.extend_from_slice(&body);

    // Feed it in chunks, the way a socket would deliver it.
    let mut codec = PacketCodec::new();
    let mut src = BytesMut::new();
    let mut decoded = None;
    for chunk in wire.chunks(1024) {
        src.extend_from_slice(chunk);
        if let Some(frame) = codec.decode(&mut src).unwrap() {
            decoded = Some(frame);
        }
    }

    let frame = decoded.expect("the frame must decode once all bytes arrive");
    assert_eq!(frame.id, 0x00);
    assert_eq!(frame.body.len(), BODY_LEN);
    assert!(src.is_empty(), "the frame must be fully consumed");
}

/// A second status request on a connection that already answered one is a
/// protocol violation and closes the connection.
#[tokio::test]
async fn a_duplicate_status_request_is_rejected() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());

    let mut client = Framed::new(client, PacketCodec::new());
    client.send(handshake(NextState::Status)).await.unwrap();
    client.send(StatusRequest).await.unwrap();
    client.send(StatusRequest).await.unwrap();

    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(NetError::DuplicateStatusRequest)),
        "a repeated status request must be rejected as a duplicate, got {result:?}"
    );
}
