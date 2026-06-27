//! Implementation for different backends. In this context, a backend is the
//! gluing part between compositor and user input (keyboard, mouse), rendering
//! and so on.

use smithay::reexports::calloop::LoopHandle;

pub mod winit;

/// Describes renderer backend for the compositor.
pub trait Backend {
    /// Initializes renderer for the given compositor state.
    fn init_renderer(
        &mut self,
        app_state: &mut crate::state::CompositorAppState,
    ) -> anyhow::Result<()>;
    /// Processes incoming events from the renderer.
    fn process_events(
        &mut self,
        event_loop_handle: LoopHandle<'static, crate::state::CompositorAppState>,
    ) -> anyhow::Result<()>;
    /// Returns an output size of the compositor surface.
    fn output_size(&self) -> smithay::utils::Size<i32, smithay::utils::Logical>;

    fn request_redraw(&mut self);
}
