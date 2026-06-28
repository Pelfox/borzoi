use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    input::{SeatHandler, SeatState},
    reexports::calloop::LoopSignal,
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
    },
};
use wayland_server::{
    Client, DisplayHandle, Resource,
    protocol::{wl_buffer::WlBuffer, wl_seat::WlSeat, wl_surface::WlSurface},
};

use crate::{
    backend::Backend, client::ClientState, input_state::InputState, layout::LayoutManager,
    shortcut::ShortcutsComponent,
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
    /// Current input devices state.
    pub input_state: InputState<Self>,
    /// Initialized shortcuts component instance.
    pub shortcuts: ShortcutsComponent,
    /// Instance of the layout manager.
    pub layout_manager: LayoutManager,
}

impl CompositorAppState {
    pub fn new(
        display_handle: &DisplayHandle,
        loop_signal: LoopSignal,
        shortcuts: ShortcutsComponent,
        layout_manager: LayoutManager,
    ) -> Self {
        let xdg_shell_state = XdgShellState::new::<Self>(display_handle);
        let compositor_state = CompositorState::new::<Self>(display_handle);
        let shm_state = ShmState::new::<Self>(display_handle, vec![]);
        let data_device_state = DataDeviceState::new::<Self>(display_handle);

        let mut seat_state = SeatState::<Self>::new();
        let input_state = InputState::new(display_handle, &mut seat_state);

        Self {
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
