pub mod bsp;

/// Unique identifier for the single window in the compositor.
pub type WindowId = wayland_server::backend::ObjectId;

/// Direction of a windows split on workspace update.
pub enum SplitDirection {
    /// Indicates that the existing windows will be splitted vertically.
    Vertical,
    /// Indicates that the existing windows will be splitted horizontally.
    Horizontal,
}

/// Represents a single node in the workspace's tree.
pub enum WindowNode {
    /// Regular window - a tree's leaf.
    Leaf {
        /// ID of the assigned window to this node.
        window_id: WindowId,
    },
    /// Split between two windows in the given direction.
    Split {
        /// Direction of the split.
        direction: SplitDirection,
        /// Left-side (or top) window node.
        left: Box<Self>,
        /// Right-side (or bottom) window node.
        right: Box<Self>,
    },
}

/// Represents window rectangle, including both relative (to the composer)
/// position and the width and height of the window to be drawn.
#[derive(Clone)]
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
pub struct WindowPlacement {
    /// ID of the window that this placement represents.
    pub window_id: WindowId,
    /// Window's relative position and size.
    pub rect: WindowRect,
}

/// Describes tiling mode's capabilities. Target implementation should keep
/// track of the root node (if it follows tree-based approach).
pub trait TilingMode {
    /// Adds a new window, additionally recalculates current workspace tree.
    fn accept_window(&mut self, window_id: &WindowId, active_window_id: Option<WindowId>);
    /// Calculates placements for all workspace's windows. Mutates given Vec.
    fn calculate_placements(&self, rect: &WindowRect, placements: &mut Vec<WindowPlacement>);
}
