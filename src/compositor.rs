//! This is the main entry for the whole compositor setup. This module contains
//! all core parts for the compositor.

use smithay::{
    input::keyboard::keysyms,
    reexports::calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic},
    wayland::socket::ListeningSocketSource,
};
use wayland_server::Display;

use crate::{
    backend::Backend,
    layout::LayoutManager,
    shortcut::{KeyboardModifiers, RegisteredShortcut, ShortcutAction, ShortcutsComponent},
    state::CompositorAppState,
};

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
    pub fn new<B>(display: Display<CompositorAppState>, mut backend: B) -> anyhow::Result<Self>
    where
        B: Backend + 'static,
    {
        let display_handle = display.handle();
        let event_loop = EventLoop::<CompositorAppState>::try_new()?;
        let loop_signal = event_loop.get_signal();

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
            action: ShortcutAction::Command("firefox".to_owned()),
        });

        let layout_manager = LayoutManager::new(display_handle.clone(), backend.output_size());
        let mut state =
            CompositorAppState::new(&display_handle, loop_signal, shortcuts, layout_manager);
        backend.init_renderer(&mut state)?;
        state.backend = Some(Box::new(backend));

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

    /// Blocks the main thread of the compositor and starts event processing
    /// from clients.
    pub fn run_event_loop(mut self) -> anyhow::Result<()> {
        if let Some(backend) = &mut self.state.backend {
            backend.process_events(self.event_loop.handle())?
        }
        self.event_loop.run(None, &mut self.state, move |_| {})?;
        Ok(())
    }
}
