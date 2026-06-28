use std::{collections::HashMap, os::unix::net::UnixStream, sync::Arc};

use smithay::{
    desktop::{PopupKind, PopupManager, Space, WindowSurfaceType},
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
    tiling::{TilingMode, bsp::BspTilingMode},
    window::{Window, WindowId, WindowPlacement, WindowRect},
};

type ScreenSize = Size<i32, Logical>;

pub struct Workspace {
    space: Space<smithay::desktop::Window>,
    windows: HashMap<WindowId, Window>,
    pub active_window: Option<Window>,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            space: Space::default(),
            windows: HashMap::new(),
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

    pub fn window_under_location(&self, location: Point<f64, Logical>) -> Option<Window> {
        if let Some(window) = self.space.element_under(location).map(|(window, _)| window) {
            if let Some(surface) = window.wl_surface() {
                return self.windows.get(&surface.id()).map(|window| window.clone());
            }
        }
        None
    }

    pub fn accept_new_floating_window(&mut self, floating_window: smithay::desktop::Window) {
        // TODO: We should calculate parent's position and insert this floating
        // window (which is, in our model, a popup/dialog) at the center of the
        // parent.
        self.space
            .map_element(floating_window.clone(), (0, 0), true);
        let window = Window::new(floating_window);
        self.windows.insert(window.id(), window.clone());
        self.active_window = Some(window);
    }

    fn submit_windows_placements(&mut self, placements: Vec<WindowPlacement>) {
        for placement in placements {
            let Some(placement_window) = self.windows.get_mut(&placement.window_id) else {
                continue;
            };

            placement_window.with_pending_state(|state| {
                state.size = Some((placement.rect.width, placement.rect.height).into());
            });

            let element = placement_window.set_placement(&placement);
            self.space
                .map_element(element, (placement.rect.x, placement.rect.y), true);
        }
    }

    pub fn accept_new_tiling_window(
        &mut self,
        tiling_window: smithay::desktop::Window,
        placements: Vec<WindowPlacement>,
    ) {
        if let Some(ref old_window) = self.active_window {
            old_window.deactivate(true);
        }

        let window = Window::new(tiling_window);
        self.windows.insert(window.id(), window.clone());
        self.active_window = Some(window.clone());
        self.submit_windows_placements(placements);
        window.activate();
    }

    pub fn delete_window(&mut self, window_id: &WindowId, placements: Vec<WindowPlacement>) {
        if let Some(window) = self.windows.remove(window_id) {
            log::debug!("Window {:?} is no longer tracked", window.id());
            self.space.unmap_elem(&window.into());
        }
        if let Some(ref active_window) = self.active_window {
            if &active_window.id() == window_id {
                self.active_window = None;
                // TODO: We should find the next nearest window and make it active.
            }
        }
        self.submit_windows_placements(placements);
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

    pub fn spawn_client_window(&mut self, surface: &ToplevelSurface) {
        let screen_rect = WindowRect {
            x: 0,
            y: 0,
            width: self.screen_size.w,
            height: self.screen_size.h,
        };
        let window = smithay::desktop::Window::new_wayland_window(surface.clone());

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
                Some(ref active_window) => Some(active_window.id()),
                None => None,
            }
        };

        if let Some(toplevel) = window.toplevel() {
            if !toplevel.parent().is_some() {
                self.tiling_mode
                    .accept_window(&new_window_id, active_window_id);
                let mut placements = Vec::new();
                self.tiling_mode
                    .calculate_placements(&screen_rect, &mut placements);
                self.current_workspace_mut()
                    .accept_new_tiling_window(window, placements);
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

    pub fn get_active_space(&self) -> &Space<smithay::desktop::Window> {
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

    pub fn on_toplevel_destroy(&mut self, surface: &ToplevelSurface) {
        let window_id = surface.wl_surface().id();
        self.tiling_mode.destroy_window(&window_id);

        let screen_rect = WindowRect {
            x: 0,
            y: 0,
            width: self.screen_size.w,
            height: self.screen_size.h,
        };
        let mut placements = Vec::new();
        self.tiling_mode
            .calculate_placements(&screen_rect, &mut placements);
        self.current_workspace_mut()
            .delete_window(&window_id, placements);
    }

    pub fn on_popup_destroy(&mut self, _: PopupSurface) {
        self.popups.cleanup();
    }
}
