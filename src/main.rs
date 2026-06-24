pub mod backend;
pub mod client;
pub mod compositor;

use wayland_server::Display;

use crate::{backend::winit::WinitBackend, compositor::CompositorApp};

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let display = Display::new()?;
    let mut app = CompositorApp::new(display)?;
    let backend = WinitBackend::new(app.event_loop.handle()).expect("failed to create backend");

    app.bind_wayland_socket()?;
    app.register_display_event_sources()?;
    app.register_backend(backend)?;

    // Run an example application.
    std::process::Command::new("ghostty").spawn().ok();

    app.run_event_loop()?;
    Ok(())
}
