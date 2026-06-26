//! This is the main entry for the whole compositor setup. This module contains
//! all core parts for the compositor.

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    input::{SeatHandler, SeatState, keyboard::keysyms},
    reexports::calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
    utils::Serial,
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        output::OutputHandler,
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
    },
};
use wayland_server::{
    Client, Display, Resource,
    protocol::{wl_buffer::WlBuffer, wl_seat::WlSeat, wl_surface::WlSurface},
};

use crate::{
    backend::Backend,
    client::ClientState,
    input_state::InputState,
    layout::LayoutManager,
    shortcut::{KeyboardModifiers, RegisteredShortcut, ShortcutAction, ShortcutsComponent},
};

/// Represents the compositor state at any given moment in time.
pub struct CompositorAppState {
    /// Internal XDG shell state for the compositor.
    xdg_shell_state: XdgShellState,
    /// Internal Wayland state for the compositor.
    compositor_state: CompositorState,
    /// Internal state for the current pair of keyboard and mouse.
    seat_state: SeatState<Self>,
    /// Internal state for teh shared memory for Wayland.
    shm_state: ShmState,
    /// Internal state for the shared device data.
    data_device_state: DataDeviceState,

    /// Target rendering backend for the compositor.
    pub backend: Option<Box<dyn Backend>>,

    /// Signal, attached to the main event loop.
    pub loop_signal: LoopSignal,

    pub input_state: InputState<CompositorAppState>,
    pub shortcuts: ShortcutsComponent,
    pub layout_manager: LayoutManager,
}

impl CompositorAppState {
    /// Requests redrawing from the rendering backend.
    pub fn request_redraw(&mut self) {
        if let Some(backend) = self.backend.as_mut() {
            backend.request_redraw();
        }
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
        // Accumulating all changes of the surface's buffer to be commited and
        // rendered afterwards by the renderer backend.
        on_commit_buffer_handler::<Self>(surface);

        if self.layout_manager.window_needs_commit_redraw(surface) {
            self.request_redraw();
        }
    }
}

impl XdgShellHandler for CompositorAppState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.layout_manager.spawn_client_window(surface);
        self.request_redraw();
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        self.layout_manager.track_window_popup(surface, positioner);
        self.request_redraw();
    }

    fn grab(&mut self, surface: PopupSurface, _seat: WlSeat, serial: Serial) {
        log::debug!(
            "popup grab requested for {:?} with serial {:?}",
            surface.wl_surface().id(),
            serial,
        );
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        self.layout_manager.reposition(surface, positioner, token);
        self.request_redraw();
    }
}

impl OutputHandler for CompositorAppState {}

impl SeatHandler for CompositorAppState {
    type KeyboardFocus = WlSurface;

    type PointerFocus = WlSurface;

    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

impl ShmHandler for CompositorAppState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl BufferHandler for CompositorAppState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl DataDeviceHandler for CompositorAppState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl SelectionHandler for CompositorAppState {
    type SelectionUserData = ();
}
impl ClientDndGrabHandler for CompositorAppState {}
impl ServerDndGrabHandler for CompositorAppState {}

smithay::delegate_compositor!(CompositorAppState);
smithay::delegate_output!(CompositorAppState);
smithay::delegate_xdg_shell!(CompositorAppState);
smithay::delegate_seat!(CompositorAppState);
smithay::delegate_shm!(CompositorAppState);
smithay::delegate_data_device!(CompositorAppState);

/// Represents the core part of the compositor - the application itself.
pub struct CompositorApp {
    /// Wayland server display. Moved into the event loop once event sources
    /// are registered.
    display: Option<Display<CompositorAppState>>,
    /// Wayland socket for the compositor. Moved into the event loop once event
    /// sources are registered.
    wayland_socket: Option<ListeningSocketSource>,
    /// The current state of the compositor.
    state: CompositorAppState,
    /// Central event loop for the compositor.
    pub event_loop: EventLoop<'static, CompositorAppState>,
}

impl CompositorApp {
    /// Creates a new instance of the compositor state, acquiring Wayland state.
    pub fn new(display: Display<CompositorAppState>) -> anyhow::Result<Self> {
        let display_handle = display.handle();

        let xdg_shell_state = XdgShellState::new::<CompositorAppState>(&display_handle);
        let compositor_state = CompositorState::new::<CompositorAppState>(&display_handle);
        let mut seat_state = SeatState::<CompositorAppState>::new();
        let shm_state = ShmState::new::<CompositorAppState>(&display_handle, vec![]);
        let data_device_state = DataDeviceState::new::<CompositorAppState>(&display_handle);

        let event_loop = EventLoop::<CompositorAppState>::try_new()?;
        let loop_signal = event_loop.get_signal();

        let input_state = InputState::new(&display_handle, &mut seat_state);
        let mut shortcuts = ShortcutsComponent::default();
        shortcuts.register(RegisteredShortcut {
            modifiers: KeyboardModifiers {
                ctrl: true,
                ..Default::default()
            },
            keysyms: vec![keysyms::KEY_t],
            action: ShortcutAction::Command("ghostty".to_owned()),
        });
        shortcuts.register(RegisteredShortcut {
            modifiers: KeyboardModifiers {
                shift: true,
                ..Default::default()
            },
            keysyms: vec![keysyms::KEY_b],
            action: ShortcutAction::Command("helium-browser".to_owned()),
        });

        let layout_manager = LayoutManager::new(display_handle);
        let state = CompositorAppState {
            xdg_shell_state,
            compositor_state,
            seat_state,
            shm_state,
            data_device_state,
            backend: None,
            loop_signal,
            input_state,
            shortcuts,
            layout_manager,
        };

        Ok(Self {
            display: Some(display),
            wayland_socket: None,
            event_loop,
            state,
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
            state.layout_manager.insert_new_client(stream);
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
        backend.init_renderer(&mut self.state)?;
        backend.process_events()?;
        self.state.backend = Some(Box::new(backend));
        Ok(())
    }

    /// Blocks the main thread of the compositor and starts event processing
    /// from clients.
    pub fn run_event_loop(mut self) -> anyhow::Result<()> {
        self.event_loop.run(None, &mut self.state, move |_| {})?;
        Ok(())
    }
}
