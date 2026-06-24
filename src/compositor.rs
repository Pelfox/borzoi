//! This is the main entry for the whole compositor setup. This module contains
//! all core parts for the compositor.

use std::sync::Arc;

use smithay::{
    reexports::calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic},
    wayland::{
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        output::OutputHandler,
        socket::ListeningSocketSource,
    },
};
use wayland_server::{Client, Display, DisplayHandle, Resource, protocol::wl_surface::WlSurface};

use crate::{backend::Backend, client::ClientState};

/// Represents the compositor state at any given moment in time.
#[derive(Debug)]
pub struct CompositorAppState {
    /// Internal Wayland state for the compositor.
    compositor_state: CompositorState,
    /// Handle to the Wayland server display, used to create globals and add
    /// clients.
    pub display_handle: DisplayHandle,
}

/// Represents the core part of the compositor - the application itself.
#[derive(Debug)]
pub struct CompositorApp {
    /// Wayland server display. Moved into the event loop once event sources
    /// are registered.
    display: Option<Display<CompositorAppState>>,
    /// Wayland socket for the compositor. Moved into the event loop once event
    /// sources are registered.
    wayland_socket: Option<ListeningSocketSource>,
    /// Central event loop for the compositor.
    pub event_loop: EventLoop<'static, CompositorAppState>,
    /// The current state of the compositor.
    state: CompositorAppState,
    /// Target rendering backend for the compositor.
    pub backend: Option<Box<dyn Backend>>,
}

impl CompositorApp {
    /// Creates a new instance of the compositor state, acquiring Wayland state.
    pub fn new(display: Display<CompositorAppState>) -> anyhow::Result<Self> {
        let display_handle = display.handle();
        let compositor_state = CompositorState::new::<CompositorAppState>(&display_handle);

        let state = CompositorAppState {
            compositor_state,
            display_handle,
        };

        Ok(Self {
            display: Some(display),
            wayland_socket: None,
            event_loop: EventLoop::<CompositorAppState>::try_new()?,
            state,
            backend: None,
        })
    }

    /// Binds compositor to the next available Wayland socket.
    pub fn bind_wayland_socket(&mut self) -> anyhow::Result<()> {
        let wayland_socket = ListeningSocketSource::new_auto()?;
        let socket_name = wayland_socket.socket_name();

        log::debug!("Listening on Wayland socket: {:?}", socket_name);
        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", socket_name);
        }

        self.wayland_socket = Some(wayland_socket);
        Ok(())
    }

    /// Registers display event sources for the given Wayland socket and
    /// display, so new clients are populated with the client data.
    pub fn register_display_event_sources(&mut self) -> anyhow::Result<()> {
        let listener = match self.wayland_socket.take() {
            Some(listener) => listener,
            None => anyhow::bail!("display event sources were already registered"),
        };
        let display = match self.display.take() {
            Some(listener) => listener,
            None => anyhow::bail!("display event sources were already registered"),
        };
        let loop_handle = self.event_loop.handle();

        // Adding new clients into the display.
        loop_handle.insert_source(listener, move |stream, _, state| {
            state
                .display_handle
                .insert_client(stream, Arc::new(ClientState::default()))
                .expect("failed to insert new client");
        })?;

        // Adding the display itself, so new events can be processed.
        let source = Generic::new(display, Interest::READ, Mode::Level);
        loop_handle.insert_source(source, |_, display, state| {
            unsafe {
                display
                    .get_mut()
                    .dispatch_clients(state)
                    .expect("failed to dispatch clients");
            }
            Ok(PostAction::Continue)
        })?;

        Ok(())
    }

    /// Registers the provided backend to use for compositing windows.
    pub fn register_backend<B>(&mut self, mut backend: B) -> anyhow::Result<()>
    where
        B: Backend + 'static,
    {
        backend.init_renderer(&self.state)?;
        backend.process_events()?;
        self.backend = Some(Box::new(backend));
        Ok(())
    }

    /// Blocks the main thread of the compositor and starts event processing
    /// from clients.
    pub fn run_event_loop(mut self) -> anyhow::Result<()> {
        self.event_loop.run(None, &mut self.state, move |_| {})?;
        Ok(())
    }
}

impl CompositorHandler for CompositorAppState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<ClientState>()
            .expect("client does not have a ClientState attached to it")
            .compositor_state
    }

    // This function is called when wl_surface state is ready to be changed. We
    // should check whether the surface belongs to one of our known
    // applications, and if yes, then update its state and schedule a redraw.
    fn commit(&mut self, surface: &WlSurface) {
        log::debug!("Surface {:?} committed its state change", surface.id());
    }
}

impl OutputHandler for CompositorAppState {}

smithay::delegate_compositor!(CompositorAppState);
smithay::delegate_output!(CompositorAppState);
