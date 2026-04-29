use super::*;

/// Split direction for layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

/// Binary tree node for pane layout.
#[derive(Debug)]
pub enum LayoutNode {
    Leaf {
        pane_id: usize,
    },
    Split {
        direction: SplitDirection,
        ratio: f32, // 0.0..1.0, portion allocated to first child
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn collect_pane_ids(&self) -> Vec<usize> {
        match self {
            LayoutNode::Leaf { pane_id } => vec![*pane_id],
            LayoutNode::Split { first, second, .. } => {
                let mut ids = first.collect_pane_ids();
                ids.extend(second.collect_pane_ids());
                ids
            }
        }
    }

    pub fn calculate_rects(&self, area: Rect) -> Vec<(usize, Rect)> {
        match self {
            LayoutNode::Leaf { pane_id } => vec![(*pane_id, area)],
            LayoutNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (first_area, second_area) = split_rect(area, *direction, *ratio);
                let mut result = first.calculate_rects(first_area);
                result.extend(second.calculate_rects(second_area));
                result
            }
        }
    }

    pub fn split_pane(
        &mut self,
        target_id: usize,
        new_id: usize,
        direction: SplitDirection,
    ) -> bool {
        match self {
            LayoutNode::Leaf { pane_id } => {
                if *pane_id == target_id {
                    let old_id = *pane_id;
                    *self = LayoutNode::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(LayoutNode::Leaf { pane_id: old_id }),
                        second: Box::new(LayoutNode::Leaf { pane_id: new_id }),
                    };
                    true
                } else {
                    false
                }
            }
            LayoutNode::Split { first, second, .. } => {
                first.split_pane(target_id, new_id, direction)
                    || second.split_pane(target_id, new_id, direction)
            }
        }
    }

    pub fn remove_pane(&mut self, target_id: usize) -> bool {
        match self {
            LayoutNode::Leaf { .. } => false,
            LayoutNode::Split { first, second, .. } => {
                if let LayoutNode::Leaf { pane_id } = first.as_ref() {
                    if *pane_id == target_id {
                        let second =
                            std::mem::replace(second.as_mut(), LayoutNode::Leaf { pane_id: 0 });
                        *self = second;
                        return true;
                    }
                }
                if let LayoutNode::Leaf { pane_id } = second.as_ref() {
                    if *pane_id == target_id {
                        let first =
                            std::mem::replace(first.as_mut(), LayoutNode::Leaf { pane_id: 0 });
                        *self = first;
                        return true;
                    }
                }
                first.remove_pane(target_id) || second.remove_pane(target_id)
            }
        }
    }

    /// Find the split boundary position and direction for hit testing.
    /// Returns a list of (boundary_position, direction, depth) for each Split node.
    pub fn split_boundaries(&self, area: Rect) -> Vec<(u16, SplitDirection, Vec<bool>)> {
        let mut result = Vec::new();
        self.collect_boundaries(area, &mut Vec::new(), &mut result);
        result
    }

    fn collect_boundaries(
        &self,
        area: Rect,
        path: &mut Vec<bool>, // false=first, true=second
        result: &mut Vec<(u16, SplitDirection, Vec<bool>)>,
    ) {
        if let LayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } = self
        {
            let (first_area, second_area) = split_rect(area, *direction, *ratio);

            // The boundary is at the edge between first and second
            let boundary = match direction {
                SplitDirection::Vertical => first_area.x + first_area.width,
                SplitDirection::Horizontal => first_area.y + first_area.height,
            };
            result.push((boundary, *direction, path.clone()));

            path.push(false);
            first.collect_boundaries(first_area, path, result);
            path.pop();

            path.push(true);
            second.collect_boundaries(second_area, path, result);
            path.pop();
        }
    }

    /// Update ratio by path (path identifies which Split node).
    pub fn update_ratio(&mut self, path: &[bool], new_ratio: f32) {
        if path.is_empty() {
            if let LayoutNode::Split { ratio, .. } = self {
                *ratio = new_ratio.clamp(0.15, 0.85);
            }
        } else if let LayoutNode::Split { first, second, .. } = self {
            if path[0] {
                second.update_ratio(&path[1..], new_ratio);
            } else {
                first.update_ratio(&path[1..], new_ratio);
            }
        }
    }

    pub fn pane_count(&self) -> usize {
        match self {
            LayoutNode::Leaf { .. } => 1,
            LayoutNode::Split { first, second, .. } => first.pane_count() + second.pane_count(),
        }
    }
}

fn split_rect(area: Rect, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
    let ratio = ratio.clamp(0.1, 0.9);
    match direction {
        SplitDirection::Vertical => {
            let first_w = (area.width as f32 * ratio) as u16;
            let first_w = first_w.max(1).min(area.width.saturating_sub(1));
            (
                Rect::new(area.x, area.y, first_w, area.height),
                Rect::new(area.x + first_w, area.y, area.width - first_w, area.height),
            )
        }
        SplitDirection::Horizontal => {
            let first_h = (area.height as f32 * ratio) as u16;
            let first_h = first_h.max(1).min(area.height.saturating_sub(1));
            (
                Rect::new(area.x, area.y, area.width, first_h),
                Rect::new(area.x, area.y + first_h, area.width, area.height - first_h),
            )
        }
    }
}
