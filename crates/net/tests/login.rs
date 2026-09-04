//! End-to-end verification of the Milestone 2 login sequence.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pyrite_net::{Connection, NetError, ServerConfig, offline_uuid};
use pyrite_protocol::codec::PacketCodec;
use pyrite_protocol::packets::handshake::{Handshake, NextState};
use pyrite_protocol::packets::login::{
    LoginAcknowledged, LoginDisconnect, LoginStart, LoginSuccess, SetCompression,
};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{PROTOCOL_VERSION, Packet};
use tokio_util::codec::Framed;

fn config(compression_threshold: Option<i32>) -> Arc<ServerConfig> {
    Arc::new(ServerConfig {
        motd: TextComponent::new("A Pyrite Server"),
        max_players: 100,
        read_timeout: Duration::from_secs(5),
        compression_threshold,
    })
}

fn handshake() -> Handshake {
    Handshake {
        protocol_version: PROTOCOL_VERSION,
        server_address: "localhost".to_owned(),
        server_port: 25565,
        next_state: NextState::Login,
    }
}

// Paused time: the connection ends by idling out the read timeout, and
// tokio auto-advances the clock once every task is blocked on a timer, so
// this is instant and deterministic rather than a real five-second wait.
#[tokio::test(start_paused = true)]
async fn a_full_login_reaches_configuration() {
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(Some(256))).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "Notch".to_owned(),
            // A deliberately bogus claimed uuid; the server must ignore it.
            uuid: u128::MAX,
        })
        .await
        .unwrap();

    // Set Compression arrives first, still in the uncompressed format.
    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, SetCompression::ID);
    let set: SetCompression = frame.decode_as().unwrap();
    assert_eq!(set.threshold, 256);

    // From here the client must speak the compressed format too.
    client.codec_mut().set_compression(set.threshold);

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, LoginSuccess::ID);
    let success: LoginSuccess = frame.decode_as().unwrap();

    assert_eq!(success.profile.username, "Notch");
    assert_eq!(
        success.profile.uuid,
        offline_uuid("Notch"),
        "the server must derive the uuid, not echo the client's"
    );
    assert_ne!(success.profile.uuid, u128::MAX);
    assert!(success.profile.properties.is_empty());

    client.send(LoginAcknowledged).await.unwrap();

    // Nothing is sent in Configuration this milestone, so the connection
    // idles until the read timeout closes it. That is the expected end state.
    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(NetError::Timeout)),
        "expected the connection to idle in configuration, got {result:?}"
    );
}

// Paused time: the connection ends by idling out the read timeout, and
// tokio auto-advances the clock once every task is blocked on a timer, so
// this is instant and deterministic rather than a real five-second wait.
#[tokio::test(start_paused = true)]
async fn login_without_compression_skips_set_compression() {
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(None)).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "Steve".to_owned(),
            uuid: 0,
        })
        .await
        .unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(
        frame.id,
        LoginSuccess::ID,
        "with compression disabled, login success comes first"
    );

    client.send(LoginAcknowledged).await.unwrap();
    assert!(matches!(task.await.unwrap(), Err(NetError::Timeout)));
}

#[tokio::test]
async fn an_invalid_username_is_disconnected_without_a_login_success() {
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(Some(256))).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "has space".to_owned(),
            uuid: 0,
        })
        .await
        .unwrap();

    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, LoginDisconnect::ID);
    let disconnect: LoginDisconnect = frame.decode_as().unwrap();
    let reason: serde_json::Value = serde_json::from_str(&disconnect.reason).unwrap();
    assert!(
        reason["text"].as_str().unwrap().contains("username"),
        "the reason should name the problem, got {reason}"
    );

    assert!(client.next().await.is_none(), "the connection closes");
    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(NetError::InvalidUsername { .. })),
        "expected an invalid-username error, got {result:?}"
    );
}

#[tokio::test]
async fn set_compression_is_itself_sent_uncompressed() {
    // The switchover is the subtle part: Set Compression uses the old format
    // and everything after it uses the new one. A client that flips too early
    // cannot parse the packet that told it to flip.
    let (client, server) = tokio::io::duplex(8192);
    let task = tokio::spawn(Connection::new(server, config(Some(256))).run());
    let mut client = Framed::new(client, PacketCodec::new());

    client.send(handshake()).await.unwrap();
    client
        .send(LoginStart {
            name: "Notch".to_owned(),
            uuid: 0,
        })
        .await
        .unwrap();

    // Decoded with a codec that has NOT been switched: this only succeeds if
    // the server sent it in the uncompressed format.
    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, SetCompression::ID);

    // And Login Success is only readable after switching, proving the server
    // did change format immediately afterwards.
    client.codec_mut().set_compression(256);
    let frame = client.next().await.unwrap().unwrap();
    assert_eq!(frame.id, LoginSuccess::ID);

    drop(client);
    let _ = task.await.unwrap();
}
