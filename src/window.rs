use smithay::{
    desktop::space::SpaceElement,
    wayland::{seat::WaylandFocus, shell::xdg::ToplevelState},
};
use wayland_server::{Resource, protocol::wl_surface::WlSurface};

/// Unique identifier for the single window in the compositor.
pub type WindowId = wayland_server::backend::ObjectId;

/// Represents window rectangle, including both relative (to the composer)
/// position and the width and height of the window to be drawn.
#[derive(Clone, Default)]
pub struct WindowRect {
    /// Window's relative position on X axis.
    pub x: i32,
    /// Window's relative position on Y axis.
    pub y: i32,
    /// Window's width.
    pub width: i32,
    /// Window's height.
    pub height: i32,
}

/// Represents placement for the given window.
#[derive(Clone)]
pub struct WindowPlacement {
    /// ID of the window that this placement represents.
    pub window_id: WindowId,
    /// Window's relative position and size.
    pub rect: WindowRect,
}

impl WindowPlacement {
    /// Creates an empty valued window placement.
    pub fn empty() -> WindowPlacement {
        WindowPlacement {
            window_id: WindowId::null(),
            rect: WindowRect::default(),
        }
    }
}

/// Represents a single unique compositor window.
#[derive(Clone)]
pub struct Window {
    inner: smithay::desktop::Window,
    placement: WindowPlacement,
}

impl PartialEq for Window {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Window {
    /// Creates a new [Window] from the given underlying Smithay's Window.
    pub fn new(inner: smithay::desktop::Window) -> Self {
        Self {
            inner,
            placement: WindowPlacement::empty(),
        }
    }

    /// Returns globally unique window's ID. Supports only Wayland.
    pub fn id(&self) -> WindowId {
        if self.inner.is_wayland() {
            if let Some(wl_surface) = self.inner.wl_surface() {
                return wl_surface.id().to_owned();
            }
        }
        WindowId::null()
    }

    /// Updates window's underlying toplevel state with the given one, and
    /// sends configuration request to the client.
    pub fn with_pending_state<F, T>(&self, f: F)
    where
        F: FnOnce(&mut ToplevelState) -> T,
    {
        if let Some(toplevel_surface) = self.inner.toplevel() {
            toplevel_surface.with_pending_state(f);
            toplevel_surface.send_configure();
        }
    }

    /// Updates window's placement. Returns the inner updated window.
    pub fn set_placement(&mut self, new_placement: &WindowPlacement) -> smithay::desktop::Window {
        self.placement = new_placement.clone();
        self.inner.clone()
    }

    /// Activates this window.
    pub fn activate(&mut self) {
        // self.inner.set_activate(true);
        self.inner.set_activated(true);
    }

    /// Retrieves the underlying surface of the window.
    pub fn surface(&self) -> Option<WlSurface> {
        self.inner.wl_surface().map(|surface| surface.into_owned())
    }
}
