//! End-to-end verification of the Milestone 1 success criterion.
//!
//! These tests drive a real `Connection` over an in-memory duplex pipe rather
//! than a socket, so they are deterministic on every platform, need no ports,
//! and never sleep.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use pyrite_net::{Connection, NetError, ServerConfig};
use pyrite_protocol::codec::PacketCodec;
use pyrite_protocol::packets::handshake::{Handshake, NextState};
use pyrite_protocol::packets::status::{
    PingRequest, PongResponse, StatusRequest, StatusResponse, StatusResponseJson,
};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{PROTOCOL_VERSION, Packet, VERSION_NAME};
use tokio_util::codec::Framed;

fn config() -> Arc<ServerConfig> {
    Arc::new(ServerConfig {
        motd: TextComponent::new("A Pyrite Server"),
        max_players: 100,
        read_timeout: Duration::from_secs(5),
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

#[tokio::test]
async fn full_server_list_ping_exchange() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake(NextState::Status)).await.unwrap();
    client.send(StatusRequest).await.unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, StatusResponse::ID);
    let response: StatusResponse = frame.decode_as().unwrap();
    let status: StatusResponseJson = serde_json::from_str(&response.json).unwrap();

    assert_eq!(status.version.protocol, PROTOCOL_VERSION);
    assert_eq!(status.version.name, VERSION_NAME);
    assert_eq!(status.players.max, 100);
    assert_eq!(status.players.online, 0);
    assert_eq!(status.description, TextComponent::new("A Pyrite Server"));

    let payload = 0x0123_4567_89ab_cdefi64;
    client.send(PingRequest { payload }).await.unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, PongResponse::ID);
    let pong: PongResponse = frame.decode_as().unwrap();
    assert_eq!(pong.payload, payload, "the payload must be echoed verbatim");

    // The server closes the connection after the pong.
    assert!(client.next().await.is_none());
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn ping_without_a_status_request_is_accepted() {
    // Clients are permitted to skip the status request entirely.
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake(NextState::Status)).await.unwrap();
    client.send(PingRequest { payload: -1 }).await.unwrap();

    let frame = client.next().await.unwrap().unwrap();
    let pong: PongResponse = frame.decode_as().unwrap();
    assert_eq!(pong.payload, -1);
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn an_unknown_packet_id_closes_the_connection() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake(NextState::Status)).await.unwrap();

    // Hand-frame a status packet with an ID nothing defines.
    let mut raw = BytesMut::new();
    raw.extend_from_slice(&[0x01, 0x7f]); // length 1, packet id 0x7f
    use tokio::io::AsyncWriteExt;
    client.get_mut().write_all(&raw).await.unwrap();

    assert!(client.next().await.is_none());

    // Assert the specific rejection, not merely that something went wrong: a
    // bare `is_err()` would also pass if the connection died of a timeout or a
    // decode failure, which would not be the behaviour under test.
    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(NetError::UnexpectedPacket { id: 0x7f, .. })),
        "the dispatcher must reject the unknown id, got {result:?}"
    );
}

#[tokio::test]
async fn a_client_disconnecting_immediately_is_not_an_error() {
    let (client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(Connection::new(server, config()).run());
    drop(client);
    // A peer hanging up before saying anything is ordinary, not a failure.
    task.await.unwrap().unwrap();
}
