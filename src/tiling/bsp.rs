use crate::{
    tiling::{SplitDirection, TilingMode, WindowNode},
    window::{WindowId, WindowPlacement, WindowRect},
};

/// Implements BSP algorithm as one of the available tiling modes, representing
/// all windows in the workspace as nodes, and splitting them at the given
/// window, allowing for efficient use of the screen space.
#[derive(Default)]
pub struct BspTilingMode {
    root: Option<WindowNode>,
}

/// Splits given window rectangle in the given direction, resulting in two new
/// window rectangles, representing left and right (or top and bottom) parts of
/// the layout respectively.
///
/// This function does not check for result size, so it is up to the caller to
/// validate that the returned values are appropriate (for example, not near
/// zero).
fn split_rect(rect: &WindowRect, direction: &SplitDirection) -> (WindowRect, WindowRect) {
    match direction {
        SplitDirection::Vertical => {
            let left_width = rect.width / 2;
            let right_width = rect.width - left_width;

            let left = WindowRect {
                x: rect.x,
                y: rect.y,
                width: left_width,
                height: rect.height,
            };
            let right = WindowRect {
                x: rect.x + left_width,
                y: rect.y,
                width: right_width,
                height: rect.height,
            };

            (left, right)
        }
        SplitDirection::Horizontal => {
            let top_height = rect.height / 2;
            let bottom_height = rect.height - top_height;

            let top = WindowRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: top_height,
            };
            let bottom = WindowRect {
                x: rect.x,
                y: rect.y + top_height,
                width: rect.width,
                height: bottom_height,
            };

            (top, bottom)
        }
    }
}

/// Counts total amount of window splits from the given node.
fn count_splits(root: &WindowNode) -> i32 {
    match root {
        WindowNode::Leaf { .. } => 0,
        WindowNode::Split { left, right, .. } => count_splits(left) + count_splits(right) + 1,
    }
}

/// Splits the current tree of windows at the given one. If root node is
/// already a split, finds neareast non-splitted window. Updates given root
/// node, rebuilding the tree. Returned value indicates whether it successfully
/// found the target window (active window).
fn split_active_window_leaf(
    root: &mut WindowNode,
    active_window_id: &WindowId,
    new_window_id: &WindowId,
    direction: SplitDirection,
) -> bool {
    match root {
        WindowNode::Leaf { window_id } => {
            if window_id != active_window_id {
                return false;
            }

            *root = WindowNode::Split {
                direction,
                left: Box::new(WindowNode::Leaf {
                    window_id: window_id.clone(),
                }),
                right: Box::new(WindowNode::Leaf {
                    window_id: new_window_id.clone(),
                }),
            };

            true
        }
        WindowNode::Split { left, right, .. } => {
            if split_active_window_leaf(left, active_window_id, new_window_id, direction) {
                return true;
            }
            if split_active_window_leaf(right, active_window_id, new_window_id, direction) {
                return true;
            }
            false
        }
    }
}

/// Calculates window placements for the given root node and a rect. For the
/// first iteration, root will be tiling tree root, and rect will be the whole
/// screen.
fn calculate_placements(
    root: &WindowNode,
    rect: &WindowRect,
    placements: &mut Vec<WindowPlacement>,
) {
    match root {
        WindowNode::Leaf { window_id } => {
            placements.push(WindowPlacement {
                window_id: window_id.clone(),
                rect: rect.clone(),
            });
        }
        WindowNode::Split {
            direction,
            left,
            right,
        } => {
            let (left_rect, right_rect) = split_rect(rect, direction);
            calculate_placements(left, &left_rect, placements);
            calculate_placements(right, &right_rect, placements);
        }
    }
}

/// Checks whether the given window ID is present in the tree, starting from
/// the root node.
fn is_window_in_tree(root: &WindowNode, target_window_id: &WindowId) -> bool {
    match root {
        WindowNode::Leaf { window_id } => window_id == target_window_id,
        WindowNode::Split { left, right, .. } => {
            if is_window_in_tree(left, target_window_id) {
                return true;
            }
            if is_window_in_tree(right, target_window_id) {
                return true;
            }
            false
        }
    }
}

/// Removes window with the given ID from the BSP tree.
fn remove_window(root: &WindowNode, target_window_id: &WindowId) -> Option<WindowNode> {
    match root {
        WindowNode::Leaf { window_id } => {
            if window_id != target_window_id {
                Some(WindowNode::Leaf {
                    window_id: window_id.clone(),
                })
            } else {
                None
            }
        }
        WindowNode::Split {
            direction,
            left,
            right,
        } => {
            let left = remove_window(left, target_window_id);
            let right = remove_window(right, target_window_id);

            match (left, right) {
                (None, None) => None,
                (None, Some(right)) => Some(right),
                (Some(left), None) => Some(left),
                (Some(left), Some(right)) => Some(WindowNode::Split {
                    direction: direction.clone(),
                    left: Box::new(left),
                    right: Box::new(right),
                }),
            }
        }
    }
}

impl TilingMode for BspTilingMode {
    fn accept_window(&mut self, window_id: &WindowId, active_window_id: Option<WindowId>) {
        // Disallow the same window from being registered multiple times.
        if let Some(ref root) = self.root
            && is_window_in_tree(root, window_id)
        {
            return;
        }

        match self.root.as_mut() {
            // If there is already an existing root node, we are splitting it
            // with the new one - for new window.
            Some(root) => {
                let direction = if count_splits(root) % 2 == 0 {
                    SplitDirection::Vertical
                } else {
                    SplitDirection::Horizontal
                };
                let inserted = match active_window_id {
                    Some(id) => split_active_window_leaf(root, &id, window_id, direction),
                    None => false,
                };

                if !inserted {
                    let old_root = self.root.take().unwrap();

                    self.root = Some(WindowNode::Split {
                        direction,
                        left: Box::new(old_root),
                        right: Box::new(WindowNode::Leaf {
                            window_id: window_id.clone(),
                        }),
                    });
                }
            }
            // Otherwise, we create a new empty root node - a leaf.
            None => {
                self.root = Some(WindowNode::Leaf {
                    window_id: window_id.clone(),
                })
            }
        }
    }

    fn calculate_placements(&self, rect: &WindowRect, placements: &mut Vec<WindowPlacement>) {
        if let Some(ref root) = self.root {
            calculate_placements(root, rect, placements);
        }
    }

    fn destroy_window(&mut self, window_id: &WindowId) {
        if let Some(ref root) = self.root {
            self.root = remove_window(root, window_id);
        }
    }
}
