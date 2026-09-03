//! The connection finite state machine.

use pyrite_protocol::State;

use crate::error::NetError;

/// Tracks which protocol state a connection is in and enforces legal moves.
///
/// Transitions are explicit rather than implied by which packet arrived, so an
/// illegal move is a typed error that closes the connection instead of a
/// silently ignored no-op.
#[derive(Debug)]
pub struct ConnectionState {
    current: State,
    status_request_seen: bool,
}

impl ConnectionState {
    /// Creates a state machine for a freshly accepted connection.
    pub fn new() -> Self {
        Self {
            current: State::Handshaking,
            status_request_seen: false,
        }
    }

    /// The state the connection is currently in.
    pub fn current(&self) -> State {
        self.current
    }

    /// Whether a status request has already been serviced.
    ///
    /// This is informational only. A client may legally skip the status
    /// request and send a ping immediately, so nothing may gate on this being
    /// true.
    pub fn status_request_seen(&self) -> bool {
        self.status_request_seen
    }

    /// Moves to `to`, or rejects the move.
    ///
    /// The only legal moves in Milestone 1 are out of `Handshaking` into
    /// `Status` or `Login`. In particular `Status -> Login` is refused: a
    /// status connection is unauthenticated and must not be able to promote
    /// itself.
    pub fn transition(&mut self, to: State) -> Result<(), NetError> {
        let permitted = matches!(
            (self.current, to),
            (State::Handshaking, State::Status) | (State::Handshaking, State::Login)
        );

        if !permitted {
            return Err(NetError::IllegalTransition {
                from: self.current,
                to,
            });
        }

        self.current = to;
        Ok(())
    }

    /// Records that a status request was received, rejecting duplicates.
    pub fn mark_status_request_seen(&mut self) -> Result<(), NetError> {
        if self.status_request_seen {
            return Err(NetError::UnexpectedPacket {
                state: self.current,
                id: 0x00,
            });
        }
        self.status_request_seen = true;
        Ok(())
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn connections_begin_in_handshaking() {
        assert_eq!(ConnectionState::new().current(), State::Handshaking);
    }

    #[test]
    fn handshaking_moves_to_status_or_login() {
        for target in [State::Status, State::Login] {
            let mut fsm = ConnectionState::new();
            fsm.transition(target).unwrap();
            assert_eq!(fsm.current(), target);
        }
    }

    #[test]
    fn status_cannot_escalate_to_login() {
        // A status connection is unauthenticated and must never be able to
        // walk itself into login; this is a protocol violation, not a no-op.
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Status).unwrap();
        assert!(matches!(
            fsm.transition(State::Login),
            Err(NetError::IllegalTransition {
                from: State::Status,
                to: State::Login
            })
        ));
        assert_eq!(
            fsm.current(),
            State::Status,
            "a rejected transition must not mutate state"
        );
    }

    #[test]
    fn handshaking_cannot_skip_to_play_or_configuration() {
        for target in [State::Play, State::Configuration, State::Handshaking] {
            let mut fsm = ConnectionState::new();
            assert!(matches!(
                fsm.transition(target),
                Err(NetError::IllegalTransition { .. })
            ));
        }
    }

    #[test]
    fn status_request_may_be_skipped_entirely() {
        // A client is allowed to send the ping straight after the handshake.
        // Nothing in the state machine may require a status request first.
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Status).unwrap();
        assert!(!fsm.status_request_seen());
    }

    #[test]
    fn duplicate_status_requests_are_rejected() {
        let mut fsm = ConnectionState::new();
        fsm.transition(State::Status).unwrap();
        fsm.mark_status_request_seen().unwrap();
        assert!(fsm.status_request_seen());
        assert!(matches!(
            fsm.mark_status_request_seen(),
            Err(NetError::UnexpectedPacket { .. })
        ));
    }
}
