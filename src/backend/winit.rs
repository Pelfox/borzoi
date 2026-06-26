//! Implements rendering backend, backed by winit.
use std::{cell::RefCell, rc::Rc};

use smithay::{
    backend::{
        input::{AbsolutePositionEvent, Event, KeyboardKeyEvent, PointerButtonEvent},
        renderer::{
            Color32F, damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
        },
        winit::{self, WinitEventLoop, WinitGraphicsBackend},
    },
    desktop::space::SpaceElement,
    input::pointer::{ButtonEvent, MotionEvent},
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{calloop::LoopHandle, winit::dpi::PhysicalSize},
    utils::{Physical, Rectangle, SERIAL_COUNTER, Size, Transform},
};
use wayland_server::backend::GlobalId;

use crate::{backend::Backend, compositor::CompositorAppState};

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
    /// Holds compositor main event loop's handle.
    event_loop_handle: LoopHandle<'static, CompositorAppState>,
    /// Holds an ID of the created winit window.
    global_id: Option<GlobalId>,
    /// References a shared backend renderer.
    renderer: Rc<RefCell<WinitBackendRenderer>>,
}

impl WinitBackend {
    /// Creates a new rendering backend, backed by winit.
    pub fn new(
        event_loop_handle: LoopHandle<'static, CompositorAppState>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (backend, winit_event_loop) = winit::init::<GlesRenderer>()?;
        let renderer = WinitBackendRenderer {
            backend,
            output: None,
            damage_tracker: None,
        };
        Ok(Self {
            winit_event_loop: Some(winit_event_loop),
            event_loop_handle,
            global_id: None,
            renderer: Rc::new(RefCell::new(renderer)),
        })
    }
}

impl Backend for WinitBackend {
    fn output_size(&self) -> Size<i32, Physical> {
        self.renderer.borrow().backend.window_size()
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

    fn process_events(&mut self) -> anyhow::Result<()> {
        let winit_event_loop = match self.winit_event_loop.take() {
            Some(event_loop) => event_loop,
            None => anyhow::bail!("winit event loop was already registered"),
        };
        let renderer_inner = Rc::clone(&self.renderer);

        self.event_loop_handle
            .insert_source(winit_event_loop, move |event, _, state| {
                match event {
                    winit::WinitEvent::Redraw => {
                        let WinitBackendRenderer {
                            backend,
                            output,
                            damage_tracker,
                        } = &mut *renderer_inner.borrow_mut();

                        let size = backend.window_size();
                        let damage = Rectangle::from_size(size);

                        let Some(output) = output.as_ref() else {
                            log::error!("Redrawn requested before output was initialized");
                            return;
                        };

                        {
                            let (renderer, mut framebuffer);
                            match backend.bind() {
                                Ok((result_renderer, result_framebuffer)) => {
                                    renderer = result_renderer;
                                    framebuffer = result_framebuffer;
                                }
                                Err(e) => {
                                    log::error!("failed to acquire renderer and framebuffer from backend: {e:?}");
                                    return;
                                }
                            }

                            let Some(damage_tracker) = damage_tracker.as_mut() else {
                                log::error!("Redrawn requested before damage tracker was initialized");
                                return;
                            };

                            let render_result = smithay::desktop::space::render_output::<
                                _,
                                WaylandSurfaceRenderElement<GlesRenderer>,
                                _,
                                _,
                            >(
                                output,
                                renderer,
                                &mut framebuffer,
                                1.0,            // Opacity for the drawn texture.
                                0,              // How old the buffer is.
                                [state.layout_manager.get_active_space()], // Space to draw the window in.
                                &[],            // Cursors, decorations, and so on.
                                damage_tracker,
                                Color32F::new(0.0, 0.0, 0.0, 1.0), // Background color used to clear out the output.
                            );
                            match render_result {
                                Ok(_) => log::debug!("Successfully rendered the output"),
                                Err(e) => log::error!("Failed to render the output: {e:?}"),
                            }
                        }

                        if let Err(e) = backend.submit(Some(&[damage])) {
                            log::error!("Failed to submit damage to the backend renderer: {e:?}");
                        }

                        state.layout_manager.refresh_frame(output);
                        if let Err(e) = state.layout_manager.display_handle.flush_clients() {
                            log::error!("Failed to flush display clients: {e:?}");
                        }

                        backend.window().request_redraw();
                    }
                    winit::WinitEvent::Input(input) => {
                        match input {
                            smithay::backend::input::InputEvent::Keyboard { event } => {
                                match state.input_state.keyboard_handle_for_device(event.device()) {
                                    Ok(handle) => {
                                        handle.input(state, event.key_code(), event.state(), SERIAL_COUNTER.next_serial(), event.time_msec(), |state, modifiers, keysym_handle| {
                                            let shortcut = state.shortcuts.shortcut_for_keystroke(modifiers.into(), keysym_handle.raw_syms());
                                            if let Some(shortcut) = shortcut {
                                                if let Err(e) = shortcut.execute() {
                                                    log::error!("Failed to process the shortcut: {e:?}");
                                                }
                                                smithay::input::keyboard::FilterResult::<()>::Intercept(())
                                            } else {
                                                smithay::input::keyboard::FilterResult::<()>::Forward
                                            }
                                        });
                                    },
                                    Err(e) => log::error!("Failed to acquire keyboard handle for device: {e:?}"),
                                };
                            }
                            smithay::backend::input::InputEvent::DeviceAdded { device } => {
                                if let Err(e) = state.input_state.on_device_added(device) {
                                    log::error!("Failed to register a new device: {e:?}");
                                }
                            }
                            smithay::backend::input::InputEvent::DeviceRemoved { device } => {
                                state.input_state.on_device_removed(device);
                            }
                            smithay::backend::input::InputEvent::PointerMotionAbsolute {
                                event,
                            } => {
                                let output_size = renderer_inner.borrow().backend.window_size().to_logical(1);
                                let location = event.position_transformed(output_size);
                                let surface_underneath = state.layout_manager.current_workspace().surface_under_location(location);

                                match state.input_state.pointer_handle_for_device(event.device()) {
                                    Ok(handle) => {
                                        let event = MotionEvent {
                                            location,
                                            serial: SERIAL_COUNTER.next_serial(),
                                            time: event.time_msec(),
                                        };
                                        handle.motion(state, surface_underneath, &event);
                                    },
                                    Err(e) => log::error!("Failed acquire pointer handle for device: {e:?}"),
                                };

                                state.request_redraw();
                            },
                            smithay::backend::input::InputEvent::PointerAxis { event } => {
                                todo!()
                            },
                            smithay::backend::input::InputEvent::PointerButton { event } => {
                                if let Some(mouse_button) = event.button() {
                                    match mouse_button {
                                        smithay::backend::input::MouseButton::Left => {
                                            match state.input_state.pointer_handle_for_device(event.device()) {
                                                Ok(handle) => {
                                                    let underlying_window = state.layout_manager.current_workspace().window_under_location(handle.current_location());
                                                    if let Some(underlying_window) = underlying_window {
                                                        let mut should_activate_window = false;

                                                        if let Some(ref window) = state.layout_manager.active_window {
                                                            if underlying_window != window {
                                                                should_activate_window = true;
                                                            }
                                                        } else {
                                                            should_activate_window = true;
                                                        }

                                                        if should_activate_window {
                                                            println!("Activating window: {underlying_window:?}");
                                                            underlying_window.set_activate(true);
                                                            state.layout_manager.active_window = Some(underlying_window.clone());
                                                        }
                                                    }

                                                    handle.button(state, &ButtonEvent{
                                                        serial: SERIAL_COUNTER.next_serial(),
                                                        time: event.time_msec(),
                                                        button: event.button_code(),
                                                        state: event.state(),
                                                    });
                                                },
                                                Err(e) => log::error!("Failed acquire pointer handle for device: {e:?}"),
                                            }
                                        },
                                        smithay::backend::input::MouseButton::Right => todo!(),
                                        _ => {
                                            match state.input_state.pointer_handle_for_device(event.device()) {
                                                Ok(handle) => {
                                                    handle.button(state, &ButtonEvent{
                                                        serial: SERIAL_COUNTER.next_serial(),
                                                        time: event.time_msec(),
                                                        button: event.button_code(),
                                                        state: event.state(),
                                                    });
                                                }
                                                Err(e) => log::error!("Failed acquire pointer handle for device: {e:?}"),
                                            };
                                        },
                                    }
                                }
                            },
                            // smithay::backend::input::InputEvent::PointerAxis { event } => todo!(),
                            event => log::debug!("Received input event from winit: {event:?}"),
                        }
                    }
                    winit::WinitEvent::CloseRequested => state.loop_signal.stop(),
                    winit::WinitEvent::Focus(is_focused) => {
                        log::info!("Focus state changed to: is_focused={is_focused}");
                    }
                    _ => {}
                };
            })
            .map_err(|err| anyhow::anyhow!("failed to register winit event source: {err:?}"))?;

        Ok(())
    }

    fn request_redraw(&mut self) {
        self.renderer.borrow().backend.window().request_redraw();
    }
}
