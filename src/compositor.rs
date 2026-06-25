//! This is the main entry for the whole compositor setup. This module contains
//! all core parts for the compositor.

use std::{ffi::OsString, sync::Arc};

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    desktop::{PopupKind, PopupManager, Space, Window},
    input::{SeatHandler, SeatState},
    reexports::calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
    utils::Serial,
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState, with_states},
        output::OutputHandler,
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            XdgToplevelSurfaceData,
        },
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
    },
};
use wayland_server::{
    Client, Display, DisplayHandle, Resource,
    protocol::{wl_buffer::WlBuffer, wl_seat::WlSeat, wl_surface::WlSurface},
};
use xkeysym::KeyCode;

use crate::{backend::Backend, client::ClientState, input_state::InputState};

/// Represents the compositor state at any given moment in time.
#[derive(Debug)]
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

    /// Handle to the Wayland server display, used to add clients.
    pub display_handle: DisplayHandle,
    /// Target rendering backend for the compositor.
    pub backend: Option<Box<dyn Backend>>,
    /// Signal, attached to the main event loop.
    pub loop_signal: LoopSignal,
    /// Two dimentional plane which maps all windows to a single output.
    pub space: Space<Window>,
    /// Tracker for windows' popups.
    pub popups: PopupManager,
    /// Current state of the input devices (mouse and keyboard).
    pub input_state: InputState,
    /// The name of the socket that the compositor is binded to.
    pub wayland_socket_name: Option<OsString>,
    /// When this compositor session has began.
    pub start_time: std::time::Instant,
}

impl CompositorAppState {
    /// Processes shortcuts from the current input state.
    pub fn process_shortcuts(&mut self) {
        let terminal_shortcut_keycodes = vec![KeyCode::new(50), KeyCode::new(28)];
        println!("Terminal shortcut keycodes: {terminal_shortcut_keycodes:?}");

        let is_terminal_shortcut_pressed = self
            .input_state
            .is_keyboard_combination_pressed(terminal_shortcut_keycodes);
        println!("Is terminal shotcut pressed: {is_terminal_shortcut_pressed:?}");
        if is_terminal_shortcut_pressed {
            let Some(socket_name) = &self.wayland_socket_name else {
                log::error!("cannot spawn terminal: WAYLAND_DISPLAY socket name is missing");
                return;
            };
            let _ = std::process::Command::new("ghostty")
                .env("WAYLAND_DISPLAY", socket_name)
                .spawn();
        }
    }

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

        let mut needs_redraw = false;
        for window in self.space.elements() {
            let is_this_window = window
                .toplevel()
                .map(|toplevel| toplevel.wl_surface() == surface)
                .unwrap_or(false);
            if !is_this_window {
                continue;
            }
            window.on_commit();
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });
            if !initial_configure_sent {
                window.toplevel().unwrap().send_pending_configure();
            }

            needs_redraw = true;
            break;
        }
        if needs_redraw {
            self.request_redraw();
        }
    }
}

impl XdgShellHandler for CompositorAppState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.size = Some((1000, 1000).into());
        });
        surface.send_configure();

        let window = Window::new_wayland_window(surface);
        self.space.map_element(window, (0, 0), false);
        self.request_redraw();
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
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
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
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
#[derive(Debug)]
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
        let compositor_state = CompositorState::new::<CompositorAppState>(&display_handle);
        let xdg_shell_state = XdgShellState::new::<CompositorAppState>(&display_handle);
        let shm_state = ShmState::new::<CompositorAppState>(&display_handle, vec![]);
        let data_device_state = DataDeviceState::new::<CompositorAppState>(&display_handle);

        let event_loop = EventLoop::<CompositorAppState>::try_new()?;
        let loop_signal = event_loop.get_signal();

        let mut seat_state = SeatState::<CompositorAppState>::new();
        let mut seat = seat_state.new_wl_seat(&display_handle, "seat-0");
        seat.add_keyboard(Default::default(), 200, 25)?;
        seat.add_pointer();

        let state = CompositorAppState {
            compositor_state,
            display_handle,
            backend: None,
            loop_signal,
            xdg_shell_state: xdg_shell_state,
            space: Space::default(),
            popups: PopupManager::default(),
            seat_state,
            input_state: InputState::default(),
            wayland_socket_name: None,
            start_time: std::time::Instant::now(),
            shm_state,
            data_device_state,
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

        self.state.wayland_socket_name = Some(socket_name.to_os_string());
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
