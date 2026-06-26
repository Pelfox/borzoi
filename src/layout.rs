use std::{os::unix::net::UnixStream, sync::Arc};

use smithay::{
    desktop::{PopupKind, PopupManager, Space, Window, WindowSurfaceType},
    output::Output,
    utils::{Logical, Point, Size},
    wayland::{
        compositor::with_states,
        shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgToplevelSurfaceData},
    },
};
use wayland_server::{DisplayHandle, protocol::wl_surface::WlSurface};

use crate::client::ClientState;

#[derive(Default)]
pub struct Workspace {
    space: Space<Window>,
}

impl Workspace {
    pub fn surface_under_location(
        &self,
        location: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let (window, window_location) = self.space.element_under(location)?;
        let relative_to_window = location - window_location.to_f64();
        let (surface, surface_location) =
            window.surface_under(relative_to_window, WindowSurfaceType::ALL)?;
        Some((
            surface,
            window_location.to_f64() + surface_location.to_f64(),
        ))
    }

    pub fn window_under_location(&self, location: Point<f64, Logical>) -> Option<&Window> {
        self.space.element_under(location).map(|(window, _)| window)
    }

    pub fn next_layout_size(&self) -> Option<Size<i32, Logical>> {
        Some((1000, 1000).into())
    }

    pub fn next_layout_point(&self) -> Point<i32, Logical> {
        let mut x = 0;
        for element in self.space.elements() {
            let size = element.geometry().size;
            if size.w == 0 && size.h == 0 {
                continue;
            }
            x += size.w;
        }
        (x, 0).into()
    }

    pub fn accept_new_window(&mut self, window: Window) {
        let window_spawn_point = self.next_layout_point();
        self.space.map_element(window, window_spawn_point, true);
    }
}

pub struct LayoutManager {
    workspaces: Vec<Workspace>,
    active_workspace_id: usize,
    popups: PopupManager,
    pub display_handle: DisplayHandle,
    start_time: std::time::Instant,

    pub active_window: Option<Window>,
}

impl LayoutManager {
    pub fn new(display_handle: DisplayHandle) -> Self {
        Self {
            workspaces: vec![Workspace::default()],
            active_workspace_id: 0,
            popups: PopupManager::default(),
            display_handle,
            start_time: std::time::Instant::now(),
            active_window: None,
        }
    }

    pub fn current_workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace_id]
    }

    pub fn current_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace_id]
    }

    pub fn insert_new_client(&mut self, stream: UnixStream) {
        self.display_handle
            .insert_client(stream, Arc::new(ClientState::default()))
            .expect("failed to insert new client");
    }

    pub fn window_needs_commit_redraw(&self, surface: &WlSurface) -> bool {
        for window in self.current_workspace().space.elements() {
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

            return true;
        }

        return false;
    }

    pub fn spawn_client_window(&mut self, surface: ToplevelSurface) {
        let active_workspace = self.current_workspace_mut();
        surface.with_pending_state(|state| state.size = active_workspace.next_layout_size());
        surface.send_configure();

        let window = Window::new_wayland_window(surface);
        active_workspace.accept_new_window(window);
    }

    pub fn track_window_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    pub fn reposition(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        // TODO: We should probably validate that the new position / geometry is correct.
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }

    pub fn map_output(&mut self, output: &Output) {
        let active_workspace = self.current_workspace_mut();
        active_workspace.space.map_output(output, (0, 0));
    }

    pub fn get_active_space(&self) -> &Space<Window> {
        &self.current_workspace().space
    }

    pub fn refresh_frame(&mut self, output: &Output) {
        let active_workspace = &mut self.workspaces[self.active_workspace_id];

        active_workspace.space.elements().for_each(|window| {
            window.send_frame(
                output,
                self.start_time.elapsed(),
                Some(std::time::Duration::ZERO),
                |_, _| Some(output.clone()),
            );
        });

        active_workspace.space.refresh();
        self.popups.cleanup();
    }
}
