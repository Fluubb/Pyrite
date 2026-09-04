//! Per-connection lifecycle: read frames, dispatch, respond.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use pyrite_protocol::codec::{PacketCodec, RawPacket};
use pyrite_protocol::packets::handshake::{Handshake, NextState};
use pyrite_protocol::packets::login::{
    GameProfile, LoginAcknowledged, LoginDisconnect, LoginStart, LoginSuccess, SetCompression,
};
use pyrite_protocol::packets::status::{
    PingRequest, PongResponse, StatusRequest, StatusResponse, StatusResponseJson,
};
use pyrite_protocol::text::TextComponent;
use pyrite_protocol::{Packet, State};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::timeout;
use tokio_util::codec::Framed;
use tracing::debug;

use crate::config::ServerConfig;
use crate::error::NetError;
use crate::offline::{offline_uuid, validate_username};
use crate::state::ConnectionState;

/// One client connection.
///
/// Generic over the transport rather than tied to `TcpStream` so that the same
/// handler serves a socket, an in-memory duplex pipe in tests, and — once the
/// embedded singleplayer server exists — a loopback pipe with no kernel
/// involvement at all.
#[derive(Debug)]
pub struct Connection<S> {
    framed: Framed<S, PacketCodec>,
    state: ConnectionState,
    config: Arc<ServerConfig>,
}

impl<S> Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// Wraps a transport in the packet codec and prepares its state machine.
    pub fn new(stream: S, config: Arc<ServerConfig>) -> Self {
        Self {
            framed: Framed::new(stream, PacketCodec::new()),
            state: ConnectionState::new(),
            config,
        }
    }

    /// Services the connection until it closes or errors.
    ///
    /// A peer hanging up cleanly is a normal outcome and returns `Ok(())`. Any
    /// protocol violation returns an error, which the caller logs; it never
    /// affects any other connection.
    pub async fn run(mut self) -> Result<(), NetError> {
        loop {
            let next = timeout(self.config.read_timeout, self.framed.next()).await;

            let frame = match next {
                Err(_elapsed) => return Err(NetError::Timeout),
                Ok(None) => {
                    debug!("peer closed the connection");
                    return Ok(());
                }
                Ok(Some(frame)) => frame?,
            };

            if self.handle(frame).await? == Flow::Close {
                return Ok(());
            }
        }
    }

    /// Dispatches one frame based on the current state and its packet ID.
    async fn handle(&mut self, frame: RawPacket) -> Result<Flow, NetError> {
        match (self.state.current(), frame.id) {
            (State::Handshaking, Handshake::ID) => self.handle_handshake(&frame).await,
            (State::Status, StatusRequest::ID) => self.handle_status_request().await,
            (State::Status, PingRequest::ID) => self.handle_ping(&frame).await,
            (State::Login, LoginStart::ID) => self.handle_login_start(&frame).await,
            (State::Login, LoginAcknowledged::ID) => self.handle_login_acknowledged().await,
            (state, id) => Err(NetError::UnexpectedPacket { state, id }),
        }
    }

    /// Handles the handshake, moving the state machine and, for a login or
    /// transfer request, sending the Milestone 1 login stub disconnect.
    async fn handle_handshake(&mut self, frame: &RawPacket) -> Result<Flow, NetError> {
        let handshake: Handshake = frame.decode_as()?;
        debug!(
            protocol_version = handshake.protocol_version,
            address = %handshake.server_address,
            next_state = ?handshake.next_state,
            "handshake received"
        );

        match handshake.next_state {
            NextState::Status => {
                self.state.transition(State::Status)?;
                Ok(Flow::Continue)
            }
            // A transferred client is mid-login from our point of view, so it
            // takes the same path as a fresh login until login is implemented.
            NextState::Login | NextState::Transfer => {
                self.state.transition(State::Login)?;
                Ok(Flow::Continue)
            }
        }
    }

    /// Answers a status request with the server's advertised status document.
    async fn handle_login_start(&mut self, frame: &RawPacket) -> Result<Flow, NetError> {
        let start: LoginStart = frame.decode_as()?;

        if let Err(error) = validate_username(&start.name) {
            debug!(name = %start.name, "rejecting invalid username");
            let packet = LoginDisconnect::from_component(&TextComponent::new(
                "Invalid username. Use 1-16 characters of A-Z, a-z, 0-9 or _.",
            ))?;
            self.framed.send(packet).await?;
            return Err(error);
        }

        // The uuid in Login Start is unauthenticated, so it is discarded and
        // the offline uuid derived from the name instead.
        let uuid = offline_uuid(&start.name);

        if let Some(threshold) = self.config.compression_threshold {
            // Send Set Compression in the OLD format, then switch. `send`
            // feeds and flushes, so the frame is on the wire before the codec
            // changes. Switching first would compress the very packet that
            // announces compression, which no client can read.
            self.framed.send(SetCompression { threshold }).await?;
            self.framed.codec_mut().set_compression(threshold);
        }

        self.framed
            .send(LoginSuccess {
                profile: GameProfile {
                    uuid,
                    username: start.name.clone(),
                    properties: Vec::new(),
                },
                session_id: uuid,
            })
            .await?;

        debug!(name = %start.name, %uuid, "login succeeded");
        Ok(Flow::Continue)
    }

    async fn handle_login_acknowledged(&mut self) -> Result<Flow, NetError> {
        self.state.transition(State::Configuration)?;
        debug!("entering configuration");
        // Configuration packets arrive in a later milestone. Until then the
        // connection idles here and the read timeout eventually closes it.
        Ok(Flow::Continue)
    }

    async fn handle_status_request(&mut self) -> Result<Flow, NetError> {
        self.state.mark_status_request_seen()?;

        let status = StatusResponseJson::new(
            self.config.motd.clone(),
            self.config.max_players,
            // Milestone 1 has no player list; every connection is transient.
            0,
        );
        let json = serde_json::to_string(&status)
            .map_err(|error| NetError::Protocol(pyrite_protocol::ProtocolError::Json(error)))?;

        self.framed.send(StatusResponse { json }).await?;
        Ok(Flow::Continue)
    }

    /// Echoes a ping payload and closes the connection, per convention.
    async fn handle_ping(&mut self, frame: &RawPacket) -> Result<Flow, NetError> {
        let ping: PingRequest = frame.decode_as()?;
        // Echo immediately and do no other work first: the client measures
        // latency as the round trip of this exact exchange.
        self.framed
            .send(PongResponse {
                payload: ping.payload,
            })
            .await?;
        // The status exchange ends here by convention.
        Ok(Flow::Close)
    }
}

/// Whether the read loop should continue or shut the connection down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Keep reading.
    Continue,
    /// Close the connection normally.
    Close,
}
