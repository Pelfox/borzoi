pub mod bsp;

/// Direction of a windows split on workspace update.
#[derive(Debug, Clone, Copy)]
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
        window_id: crate::window::WindowId,
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

/// Describes tiling mode's capabilities. Target implementation should keep
/// track of the root node (if it follows tree-based approach).
pub trait TilingMode {
    /// Adds a new window, additionally recalculates current workspace tree.
    fn accept_window(
        &mut self,
        window_id: &crate::window::WindowId,
        active_window_id: Option<crate::window::WindowId>,
    );
    /// Calculates placements for all workspace's windows. Mutates given Vec.
    fn calculate_placements(
        &self,
        rect: &crate::window::WindowRect,
        placements: &mut Vec<crate::window::WindowPlacement>,
    );
}
