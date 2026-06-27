use std::{os::unix::net::UnixStream, sync::Arc};

use smithay::{
    desktop::{PopupKind, PopupManager, Space, Window, WindowSurfaceType},
    output::Output,
    utils::{Logical, Point, Size},
    wayland::{
        compositor::with_states,
        seat::WaylandFocus,
        shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgToplevelSurfaceData},
    },
};
use wayland_server::{DisplayHandle, Resource, protocol::wl_surface::WlSurface};

use crate::{
    client::ClientState,
    tiling::{TilingMode, WindowId, WindowPlacement, WindowRect, bsp::BspTilingMode},
};

type ScreenSize = Size<i32, Logical>;

pub struct Workspace {
    space: Space<Window>,
    active_window: Option<Window>,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            space: Space::default(),
            active_window: None,
        }
    }

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

    pub fn accept_new_floating_window(&mut self, window: Window) {
        // TODO: We should calculate parent's position and insert this floating
        // window (which is, in our model, a popup/dialog) at the center of the
        // parent.
        println!("New floating window");
        self.space.map_element(window.clone(), (0, 0), true);
    }

    fn get_window_by_id(&self, window_id: &WindowId) -> Option<&Window> {
        for element in self.space.elements() {
            if let Some(surface) = element.wl_surface() {
                if &surface.id() == window_id {
                    return Some(element);
                }
            }
        }
        None
    }

    pub fn accept_new_tiling_window(&mut self, window: Window, placements: &Vec<WindowPlacement>) {
        println!("New tiling window");
        self.space.map_element(window.clone(), (0, 0), true);

        for placement in placements {
            if let Some(placement_window) = self.get_window_by_id(&placement.window_id) {
                if let Some(toplevel_surface) = placement_window.toplevel() {
                    toplevel_surface.with_pending_state(|state| {
                        state.size = Some((placement.rect.width, placement.rect.height).into());
                    });
                    toplevel_surface.send_configure();
                }
                self.space.map_element(
                    placement_window.clone(),
                    (placement.rect.x, placement.rect.y),
                    true,
                );
            }
        }
    }
}

pub struct LayoutManager {
    workspaces: Vec<Workspace>,
    active_workspace_id: usize,
    popups: PopupManager,
    start_time: std::time::Instant,
    screen_size: ScreenSize,

    pub display_handle: DisplayHandle,
    tiling_mode: Box<dyn TilingMode>,
}

impl LayoutManager {
    pub fn new(display_handle: DisplayHandle, screen_size: ScreenSize) -> Self {
        Self {
            workspaces: vec![Workspace::new()],
            active_workspace_id: 0,
            popups: PopupManager::default(),
            start_time: std::time::Instant::now(),
            screen_size,
            display_handle,
            tiling_mode: Box::new(BspTilingMode::default()), // TODO
        }
    }

    pub fn current_workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace_id]
    }

    pub fn current_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace_id]
    }

    pub fn active_window(&self) -> &Option<Window> {
        &self.current_workspace().active_window
    }

    pub fn set_active_window(&mut self, window: Window) {
        self.current_workspace_mut().active_window = Some(window);
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
        surface.send_configure();

        let screen_rect = WindowRect {
            x: 0,
            y: 0,
            width: self.screen_size.w,
            height: self.screen_size.h,
        };
        let window = Window::new_wayland_window(surface);

        let new_window_id: WindowId;
        if let Some(wl_surface) = window.wl_surface() {
            new_window_id = wl_surface.id();
        } else {
            self.current_workspace_mut()
                .accept_new_floating_window(window);
            return;
        }

        let active_window_id = {
            let active_workspace = self.current_workspace_mut();
            match active_workspace.active_window {
                Some(ref active_window) => {
                    if let Some(surface) = active_window.wl_surface() {
                        Some(surface.id())
                    } else {
                        None
                    }
                }
                None => None,
            }
        };

        self.tiling_mode
            .accept_window(&new_window_id, active_window_id);

        if let Some(toplevel) = window.toplevel() {
            if !toplevel.parent().is_some() {
                let mut placements = Vec::new();
                self.tiling_mode
                    .calculate_placements(&screen_rect, &mut placements);
                self.current_workspace_mut()
                    .accept_new_tiling_window(window, &placements);
                return;
            }
        }

        self.current_workspace_mut()
            .accept_new_floating_window(window);
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
