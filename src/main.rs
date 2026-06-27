pub mod backend;
pub mod client;
pub mod compositor;
pub mod input_state;
pub mod state;

pub mod layout;
pub mod shortcut;
pub mod tiling;

use wayland_server::Display;

use crate::{backend::winit::WinitBackend, compositor::CompositorApp};

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let display = Display::new()?;
    let backend = WinitBackend::new().expect("failed to create backend");
    let mut app = CompositorApp::new(display, backend)?;

    app.bind_wayland_socket()?;
    app.register_display_event_sources()?;

    app.run_event_loop()?;
    Ok(())
}
