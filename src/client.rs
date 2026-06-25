//! Per-client state for a Wayland connection.
//!
//! A client is a process/connection talking to the compositor. One client may
//! create multiple surfaces, toplevel windows, popups, or no windows at all.

use smithay::wayland::compositor::CompositorClientState;
use wayland_server::backend::{ClientData, ClientId, DisconnectReason};

/// Represents a single client's state in the compositor.
#[derive(Default)]
pub struct ClientState {
    /// Current compositor state of the client.
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, client_id: ClientId) {
        log::debug!("New client is initialized: {client_id:?}");
    }

    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        log::debug!("Client {client_id:?} has disconnected: {reason:?}");
    }
}
