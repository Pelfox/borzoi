//! Implements rendering backend, backed by winit.
use std::{cell::RefCell, rc::Rc};

use smithay::{
    backend::{
        input::{
            AbsolutePositionEvent, ButtonState, Device, Event, InputEvent, KeyState,
            KeyboardKeyEvent, PointerButtonEvent,
        },
        renderer::{
            Color32F, damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
        },
        winit::{
            self, WinitEvent, WinitEventLoop, WinitGraphicsBackend, WinitInput,
            WinitKeyboardInputEvent, WinitMouseInputEvent, WinitMouseMovedEvent,
        },
    },
    desktop::space::render_output,
    input::{
        keyboard::{FilterResult, KeyboardHandle},
        pointer::{ButtonEvent, MotionEvent, PointerHandle},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{calloop::LoopHandle, winit::dpi::PhysicalSize},
    utils::{Logical, Rectangle, SERIAL_COUNTER, Size, Transform},
};
use wayland_server::backend::GlobalId;

use crate::{backend::Backend, state::CompositorAppState};

/// Describes the current state of the backend renderer.
struct WinitBackendRenderer {
    /// Actual renderer reference for the backend.
    backend: WinitGraphicsBackend<GlesRenderer>,
    /// Created Wayland output for the renderer.
    output: Option<Output>,
    /// Tracker for framebuffer differences (damage).
    damage_tracker: Option<OutputDamageTracker>,
}

/// Implements [Backend] using winit (drawing the whole compositor in a window).
pub struct WinitBackend {
    /// Holds winit's lifecycle-bound event loop.
    winit_event_loop: Option<WinitEventLoop>,
    /// Holds an ID of the created winit window.
    global_id: Option<GlobalId>,
    /// References a shared backend renderer.
    renderer: Rc<RefCell<WinitBackendRenderer>>,
}

impl WinitBackend {
    /// Creates a new rendering backend, backed by winit.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (backend, winit_event_loop) = winit::init::<GlesRenderer>()?;
        let renderer = WinitBackendRenderer {
            backend,
            output: None,
            damage_tracker: None,
        };
        Ok(Self {
            winit_event_loop: Some(winit_event_loop),
            global_id: None,
            renderer: Rc::new(RefCell::new(renderer)),
        })
    }

    fn on_shutdown_event(state: &mut CompositorAppState) {
        state.loop_signal.stop();
        log::info!("Received close request, shutting down.");
    }

    fn on_keyboard_input(
        handle: &KeyboardHandle<CompositorAppState>,
        state: &mut CompositorAppState,
        event: &WinitKeyboardInputEvent,
    ) -> Option<anyhow::Result<()>> {
        let serial = SERIAL_COUNTER.next_serial();
        handle.input(
            state,
            event.key_code(),
            event.state(),
            serial,
            event.time_msec(),
            |state, modifiers, keysym_handle| {
                if event.state() != KeyState::Pressed {
                    return FilterResult::<anyhow::Result<()>>::Forward;
                }

                if let Some(shortcut) = state
                    .shortcuts
                    .find_shortcut(modifiers.into(), keysym_handle.raw_syms())
                {
                    log::debug!("Executing shortcut");
                    return FilterResult::Intercept(shortcut.execute());
                }

                FilterResult::Forward
            },
        )
    }

    fn on_pointer_absolute_motion(
        handle: &PointerHandle<CompositorAppState>,
        state: &mut CompositorAppState,
        event: &WinitMouseMovedEvent,
        renderer: Rc<RefCell<WinitBackendRenderer>>,
    ) {
        let renderer = renderer.borrow();
        let location = event.position_transformed(renderer.backend.window_size().to_logical(1));

        let surface_underneath_pointer = state
            .layout_manager
            .current_workspace()
            .surface_under_location(location);

        let serial = SERIAL_COUNTER.next_serial();
        handle.motion(
            state,
            surface_underneath_pointer,
            &MotionEvent {
                location,
                serial,
                time: event.time_msec(),
            },
        );
    }

    fn on_pointer_click(
        handle: &PointerHandle<CompositorAppState>,
        event: &WinitMouseInputEvent,
        state: &mut CompositorAppState,
    ) {
        let current_workspace = state.layout_manager.current_workspace();
        let window_underneath = current_workspace.window_under_location(handle.current_location());

        let mut should_activate_window = false;
        if current_workspace.active_window.is_none()
            || window_underneath != current_workspace.active_window
        {
            should_activate_window = true;
        }

        // Windows can be only activated on press.
        let can_activate_window = event.state() == ButtonState::Pressed;
        if let Some(window_underneath) = window_underneath
            && should_activate_window
            && can_activate_window
        {
            // Deactivate previous window.
            if let Some(ref previous_window) = current_workspace.active_window {
                previous_window.deactivate(true);
                log::debug!("Window {:?} deactivated", previous_window.id());
            }

            // Make keyboard focus this window as a primary.
            log::debug!("Focusing window {:?}", window_underneath.id());
            if let Some(keyboard_handle) = state.input_state.get_keyboard() {
                let serial = SERIAL_COUNTER.next_serial();
                keyboard_handle.set_focus(state, window_underneath.surface(), serial);
            }

            window_underneath.activate();
            state.layout_manager.set_active_window(window_underneath);
        }

        let serial = SERIAL_COUNTER.next_serial();
        handle.button(
            state,
            &ButtonEvent {
                serial,
                time: event.time_msec(),
                button: event.button_code(),
                state: event.state(),
            },
        );
        log::debug!("Tracked pointer button activation: {:?}", event.button());
    }

    fn on_input_event(
        event: &InputEvent<WinitInput>,
        state: &mut CompositorAppState,
        renderer: Rc<RefCell<WinitBackendRenderer>>,
    ) {
        match event {
            InputEvent::DeviceAdded { device } => {
                if let Err(e) = state.input_state.on_device_added(device) {
                    log::error!("Failed to register device {:?}: {e:?}", device.id());
                    return;
                }
                log::info!("Device {} was added and registered", device.id());
            }
            InputEvent::DeviceRemoved { device } => {
                state.input_state.on_device_removed(device);
                log::info!("Device {} was removed and unregistered", device.id());
            }
            InputEvent::Keyboard { event } => {
                println!("Keyboard event: {event:?}");
                match state.input_state.device_keyboard_handle(&event.device()) {
                    Ok(handle) => {
                        if let Some(Err(e)) = Self::on_keyboard_input(&handle, state, event) {
                            log::error!(
                                "Failed to process keyboard event for {}: {e:?}",
                                event.device().id()
                            );
                            return;
                        }
                        if let Some(ref mut backend) = state.backend {
                            backend.request_redraw();
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to retrieve keyboard handle for {}: {e:?}",
                            event.device().id()
                        );
                    }
                }
            }
            InputEvent::PointerMotionAbsolute { event } => {
                match state.input_state.pointer_handle_for_device(&event.device()) {
                    Ok(handle) => {
                        Self::on_pointer_absolute_motion(&handle, state, event, renderer);
                        if let Some(ref mut backend) = state.backend {
                            backend.request_redraw();
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to retrieve pointer handle for {}: {e:?}",
                            event.device().id()
                        );
                    }
                }
            }
            InputEvent::PointerButton { event } => {
                match state.input_state.pointer_handle_for_device(&event.device()) {
                    Ok(handle) => {
                        Self::on_pointer_click(&handle, event, state);
                        if let Some(ref mut backend) = state.backend {
                            backend.request_redraw();
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to retrieve pointer handle for {}: {e:?}",
                            event.device().id()
                        );
                    }
                }
            }
            // TODO: InputEvent::PointerAxis { event } => todo!(),
            event => log::debug!("Received an unhandled input event: {event:?}"),
        }
    }

    fn on_redraw_event(
        state: &mut CompositorAppState,
        renderer: Rc<RefCell<WinitBackendRenderer>>,
    ) {
        let backend_renderer = &mut *renderer.borrow_mut();
        let Some(ref output) = backend_renderer.output else {
            log::error!("Redraw was requested before output was initialized");
            return;
        };

        {
            let (renderer, mut framebuffer) = match backend_renderer.backend.bind() {
                Ok(values) => values,
                Err(e) => {
                    log::error!("Failed to acquire renderer and framebuffer from backend: {e:?}");
                    return;
                }
            };

            let Some(damage_tracker) = backend_renderer.damage_tracker.as_mut() else {
                log::error!("Redraw was requested before damage tracker was initialized");
                return;
            };

            let render_result = render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
                output,
                renderer,
                &mut framebuffer,
                1.0,                                       // Opacity for the drawn texture.
                0,                                         // How old the buffer is.
                [state.layout_manager.get_active_space()], // Space to draw the window in.
                &[],                                       // Cursors, decorations, and so on.
                damage_tracker,
                Color32F::new(0.0, 0.0, 0.0, 1.0), // Background color used to clear out the output.
            );

            if let Err(e) = render_result {
                log::error!("Failed to submit render: {e:?}");
            }
        }

        let damage = Rectangle::from_size(backend_renderer.backend.window_size());
        if let Err(e) = backend_renderer.backend.submit(Some(&[damage])) {
            log::error!("Failed to submit damage to renderer backend: {e:?}");
            return;
        }

        state.layout_manager.refresh_frame(output);
        if let Err(e) = state.layout_manager.display_handle.flush_clients() {
            log::error!("Failed to update display clients: {e:?}");
            return;
        }

        backend_renderer.backend.window().request_redraw();
    }

    fn on_event_dispatched(
        event: &WinitEvent,
        state: &mut CompositorAppState,
        renderer: Rc<RefCell<WinitBackendRenderer>>,
    ) {
        match event {
            WinitEvent::CloseRequested => Self::on_shutdown_event(state),
            WinitEvent::Input(event) => Self::on_input_event(event, state, renderer),
            WinitEvent::Redraw => Self::on_redraw_event(state, renderer),
            event => log::debug!("Received an unhandled winit event: {event:?}"),
        }
    }
}

impl Backend for WinitBackend {
    fn output_size(&self) -> Size<i32, Logical> {
        self.renderer.borrow().backend.window_size().to_logical(1) // TODO
    }

    fn init_renderer(&mut self, app_state: &mut CompositorAppState) -> anyhow::Result<()> {
        let mut renderer = self.renderer.borrow_mut();

        let (mut refresh_rate, mut monitor_size) = (60_000, PhysicalSize::new(512, 512));
        if let Some(monitor) = renderer.backend.window().primary_monitor() {
            if let Some(monitor_refresh_rate) = monitor.refresh_rate_millihertz() {
                refresh_rate = monitor_refresh_rate as i32;
            }
            monitor_size = monitor.size();
        }

        let mode = Mode {
            size: renderer.backend.window_size(),
            refresh: refresh_rate,
        };
        let output = Output::new(
            "output-0".into(),
            PhysicalProperties {
                size: (monitor_size.width as i32, monitor_size.height as i32).into(),
                subpixel: Subpixel::Unknown,
                make: "winit".into(),
                model: "unknown".into(),
            },
        );
        log::info!("Target refresh_rate: {refresh_rate:?}, monitor size: {monitor_size:?}");
        log::info!("Target window size: {:?}", renderer.backend.window_size());

        let global_id =
            output.create_global::<CompositorAppState>(&app_state.layout_manager.display_handle);
        self.global_id = Some(global_id);

        // Update output's mode for future drawing requests.
        output.change_current_state(
            Some(mode),
            Some(Transform::Flipped180),
            None,
            Some((0, 0).into()),
        );
        output.set_preferred(mode);
        app_state.layout_manager.map_output(&output);

        renderer.damage_tracker = Some(OutputDamageTracker::from_output(&output));
        renderer.output = Some(output);

        Ok(())
    }

    fn process_events(
        &mut self,
        event_loop_handle: LoopHandle<'static, CompositorAppState>,
    ) -> anyhow::Result<()> {
        let winit_event_loop = match self.winit_event_loop.take() {
            Some(event_loop) => event_loop,
            None => anyhow::bail!("winit event loop was already registered"),
        };

        let renderer_inner = Rc::clone(&self.renderer);
        event_loop_handle
            .insert_source(winit_event_loop, move |event, _, state| {
                Self::on_event_dispatched(&event, state, renderer_inner.clone());
            })
            .map_err(|err| anyhow::anyhow!("failed to register winit event source: {err:?}"))?;

        Ok(())
    }

    fn request_redraw(&mut self) {
        self.renderer.borrow().backend.window().request_redraw();
    }
}
