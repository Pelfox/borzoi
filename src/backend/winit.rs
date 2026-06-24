//! Implements rendering backend, backed by winit.
use smithay::{
    backend::{
        renderer::gles::GlesRenderer,
        winit::{self, WinitEventLoop, WinitGraphicsBackend},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::LoopHandle,
    utils::{Physical, Size},
};
use wayland_server::backend::GlobalId;

use crate::{backend::Backend, compositor::CompositorAppState};

/// Implements [Backend] using winit (drawing the whole compositor in a window).
#[derive(Debug)]
pub struct WinitBackend {
    /// Holds graphics backend for the winit, using Gles (OpenGL ES) rendering.
    backend: WinitGraphicsBackend<GlesRenderer>,
    /// Holds winit's lifecycle-bound event loop.
    winit_event_loop: Option<WinitEventLoop>,
    /// Holds compositor main event loop's handle.
    event_loop_handle: LoopHandle<'static, CompositorAppState>,
    /// Holds an ID of the created winit window.
    global_id: Option<GlobalId>,
    /// Holds created output Wayland global.
    output: Option<Output>,
}

impl WinitBackend {
    /// Creates a new rendering backend, backed by winit.
    pub fn new(
        event_loop_handle: LoopHandle<'static, CompositorAppState>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (backend, winit_event_loop) = winit::init::<GlesRenderer>()?;
        Ok(Self {
            backend,
            winit_event_loop: Some(winit_event_loop),
            event_loop_handle,
            global_id: None,
            output: None,
        })
    }
}

impl Backend for WinitBackend {
    fn output_size(&self) -> Size<i32, Physical> {
        self.backend.window_size()
    }

    fn init_renderer(&mut self, app_state: &CompositorAppState) -> anyhow::Result<()> {
        let refresh_rate = self
            .backend
            .window()
            .primary_monitor()
            .and_then(|monitor| monitor.refresh_rate_millihertz())
            .map(|rate| rate as i32)
            .unwrap_or(60_000);

        let mode = Mode {
            size: self.backend.window_size(),
            refresh: refresh_rate,
        };
        let output = Output::new(
            "output-0".into(),
            PhysicalProperties {
                size: (255, 255).into(),
                subpixel: Subpixel::Unknown,
                make: "winit".into(),
                model: "unknown".into(),
            },
        );

        let global_id = output.create_global::<CompositorAppState>(&app_state.display_handle);
        self.global_id = Some(global_id);
        output.set_preferred(mode);
        self.output = Some(output);

        Ok(())
    }

    fn process_events(&mut self) -> anyhow::Result<()> {
        let winit_event_loop = match self.winit_event_loop.take() {
            Some(event_loop) => event_loop,
            None => anyhow::bail!("winit event loop was already registered"),
        };

        self.event_loop_handle
            .insert_source(winit_event_loop, move |event, _, state| {
                log::debug!("Received winit event: {event:?}, state: {state:?}");
            })
            .map_err(|err| anyhow::anyhow!("failed to register winit event source: {err:?}"))?;

        Ok(())
    }
}
