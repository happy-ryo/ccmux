use super::*;

impl App {
    fn scroll_pane_to_click(&self, pane_id: usize, click_row: u16, inner: &Rect) {
        if let Some(pane) = self.ws().panes.get(&pane_id) {
            let (_, total_lines) = pane.scrollbar_info();
            let visible_rows = inner.height as usize;
            if total_lines <= visible_rows {
                return;
            }
            let max_scroll = total_lines.saturating_sub(visible_rows);
            let relative_y = click_row.saturating_sub(inner.y) as f32;
            let ratio = relative_y / inner.height.max(1) as f32;
            let target_scroll = ((1.0 - ratio) * max_scroll as f32) as usize;
            let mut parser = pane.parser.lock().unwrap_or_else(|e| e.into_inner());
            parser.screen_mut().set_scrollback(target_scroll);
        }
    }

    fn is_on_file_tree_border(&self, col: u16) -> bool {
        if let Some(rect) = self.ws().last_file_tree_rect {
            let border_col = rect.x + rect.width;
            col >= border_col.saturating_sub(1) && col <= border_col
        } else {
            false
        }
    }

    fn is_on_preview_border(&self, col: u16) -> bool {
        if let Some(rect) = self.ws().last_preview_rect {
            let border_col = if self.layout_swapped {
                rect.x + rect.width
            } else {
                rect.x
            };
            col >= border_col.saturating_sub(1) && col <= border_col
        } else {
            false
        }
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Down(_)) && self.rename_input.is_some() {
            let needs_relayout = !self.status_bar_visible;
            self.rename_input = None;
            self.dirty = true;
            if needs_relayout {
                self.mark_layout_change();
            }
        }

        if let Some(DragTarget::PaneMouseReport(ws_idx, pane_id, rect, btn)) = self.dragging.clone()
        {
            match mouse.kind {
                MouseEventKind::Drag(_) => {
                    self.forward_pointer_to_pane(
                        ws_idx,
                        pane_id,
                        rect,
                        btn,
                        PointerAction::Drag,
                        &mouse,
                    );
                    return;
                }
                MouseEventKind::Up(_) => {
                    self.forward_pointer_to_pane(
                        ws_idx,
                        pane_id,
                        rect,
                        btn,
                        PointerAction::Release,
                        &mouse,
                    );
                    self.dragging = None;
                    return;
                }
                _ => {}
            }
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let col = mouse.column;
                let row = mouse.row;
                self.selection = None;

                for &(tab_idx, rect) in &self.last_tab_rects {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        let now = Instant::now();
                        let is_double = matches!(
                            self.last_tab_click,
                            Some((prev_idx, prev_t))
                                if prev_idx == tab_idx
                                    && now.duration_since(prev_t).as_millis() < 500
                        );
                        if self.active_tab != tab_idx {
                            self.suspend_overlay();
                        }
                        self.active_tab = tab_idx;
                        if is_double {
                            self.rename_input = Some(String::new());
                            self.last_tab_click = None;
                        } else {
                            self.last_tab_click = Some((tab_idx, now));
                        }
                        self.dirty = true;
                        return;
                    }
                }
                self.last_tab_click = None;

                if let Some(rect) = self.last_new_tab_rect {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        if let Ok(new_id) = self.new_tab() {
                            self.emit_pane_started(new_id);
                        }
                        return;
                    }
                }

                if self.is_on_file_tree_border(col) {
                    self.dragging = Some(DragTarget::FileTreeBorder);
                    return;
                }
                if self.is_on_preview_border(col) {
                    self.dragging = Some(DragTarget::PreviewBorder);
                    return;
                }

                if let Some(pane_area) = self.ws().last_pane_rects.first().map(|_| {
                    let rects = &self.ws().last_pane_rects;
                    let min_x = rects.iter().map(|(_, r)| r.x).min().unwrap_or(0);
                    let min_y = rects.iter().map(|(_, r)| r.y).min().unwrap_or(0);
                    let max_x = rects.iter().map(|(_, r)| r.x + r.width).max().unwrap_or(0);
                    let max_y = rects.iter().map(|(_, r)| r.y + r.height).max().unwrap_or(0);
                    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
                }) {
                    let boundaries = self.ws().layout.split_boundaries(pane_area);
                    for (boundary, direction, path) in boundaries {
                        let on_border = match direction {
                            SplitDirection::Vertical => {
                                col >= boundary.saturating_sub(1)
                                    && col <= boundary
                                    && row >= pane_area.y
                                    && row < pane_area.y + pane_area.height
                            }
                            SplitDirection::Horizontal => {
                                row >= boundary.saturating_sub(1)
                                    && row <= boundary
                                    && col >= pane_area.x
                                    && col < pane_area.x + pane_area.width
                            }
                        };
                        if on_border {
                            self.dragging = Some(DragTarget::PaneSplit(path, direction, pane_area));
                            return;
                        }
                    }
                }

                if let Some(rect) = self.ws().last_file_tree_rect {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        self.ws_mut().focus_target = FocusTarget::FileTree;
                        let inner_y = row.saturating_sub(rect.y + 1);
                        let scroll = self.ws().file_tree.scroll_offset;
                        let entry_idx = scroll + inner_y as usize;
                        let entry_count = self.ws().file_tree.visible_entries().len();
                        if entry_idx < entry_count {
                            self.ws_mut().file_tree.selected_index = entry_idx;
                            let path = self.ws_mut().file_tree.toggle_or_select();
                            if let Some(path) = path {
                                self.clear_selection_if_preview();
                                let messages = self.messages();
                                let mut picker = self.image_picker.take();
                                self.ws_mut().preview.load(&path, picker.as_mut(), messages);
                                self.image_picker = picker;
                            }
                        }
                        return;
                    }
                }

                if let Some(rect) = self.ws().last_preview_rect {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        self.ws_mut().focus_target = FocusTarget::Preview;
                        return;
                    }
                }

                let pane_rects = self.ws().last_pane_rects.clone();
                for (pane_id, rect) in pane_rects {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        self.ws_mut().focused_pane_id = pane_id;
                        self.ws_mut().focus_target = FocusTarget::Pane;

                        if !mouse.modifiers.contains(KeyModifiers::SHIFT)
                            && !mouse_forward_disabled()
                            && self.try_forward_pane_press(
                                pane_id,
                                rect,
                                PointerButton::Left,
                                col,
                                row,
                            )
                        {
                            return;
                        }

                        let scrollbar_col = rect.x + rect.width - 2;
                        if col >= scrollbar_col {
                            let inner = Rect::new(
                                rect.x + 1,
                                rect.y + 1,
                                rect.width.saturating_sub(2),
                                rect.height.saturating_sub(2),
                            );
                            self.scroll_pane_to_click(pane_id, row, &inner);
                            self.dragging = Some(DragTarget::Scrollbar(pane_id, inner));
                        }
                        return;
                    }
                }
            }
            MouseEventKind::Down(btn @ (MouseButton::Middle | MouseButton::Right)) => {
                let col = mouse.column;
                let row = mouse.row;
                if mouse.modifiers.contains(KeyModifiers::SHIFT) || mouse_forward_disabled() {
                    return;
                }
                let pointer_btn = match btn {
                    MouseButton::Middle => PointerButton::Middle,
                    MouseButton::Right => PointerButton::Right,
                    MouseButton::Left => unreachable!(),
                };
                let pane_rects = self.ws().last_pane_rects.clone();
                for (pane_id, rect) in pane_rects {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        self.try_forward_pane_press(pane_id, rect, pointer_btn, col, row);
                        return;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let col = mouse.column;
                let row = mouse.row;

                if let Some(ref target) = self.dragging.clone() {
                    match target {
                        DragTarget::FileTreeBorder => {
                            self.file_tree_width = col.clamp(10, 60);
                        }
                        DragTarget::PreviewBorder => {
                            if let Some(rect) = self.ws().last_preview_rect {
                                if self.layout_swapped {
                                    let new_width = col.saturating_sub(rect.x).clamp(15, 80);
                                    self.preview_width = new_width;
                                } else {
                                    let total_right = rect.x + rect.width;
                                    let new_width = total_right.saturating_sub(col).clamp(15, 80);
                                    self.preview_width = new_width;
                                }
                            }
                        }
                        DragTarget::PaneSplit(path, direction, area) => {
                            let new_ratio = match direction {
                                SplitDirection::Vertical => {
                                    (col.saturating_sub(area.x) as f32) / area.width.max(1) as f32
                                }
                                SplitDirection::Horizontal => {
                                    (row.saturating_sub(area.y) as f32) / area.height.max(1) as f32
                                }
                            };
                            self.ws_mut().layout.update_ratio(path, new_ratio);
                        }
                        DragTarget::Scrollbar(pane_id, inner) => {
                            self.scroll_pane_to_click(*pane_id, row, inner);
                        }
                        DragTarget::PaneMouseReport(..) => {
                            debug_assert!(false, "PaneMouseReport leaked to border-drag match");
                        }
                    }
                    return;
                }

                if let Some(ref mut sel) = self.selection {
                    let inner = sel.content_rect;
                    match sel.target {
                        SelectionTarget::Pane(_) => {
                            sel.end_col = col
                                .saturating_sub(inner.x)
                                .min(inner.width.saturating_sub(1))
                                as u32;
                            sel.end_row = row
                                .saturating_sub(inner.y)
                                .min(inner.height.saturating_sub(1))
                                as u32;
                        }
                        SelectionTarget::Preview => {
                            let scroll_v = self.ws().preview.scroll_offset;
                            let h_scroll = self.ws().preview.h_scroll_offset;

                            let mut screen_col = col.saturating_sub(inner.x);
                            let mut screen_row = row.saturating_sub(inner.y);

                            if col < inner.x {
                                self.ws_mut().preview.scroll_left(2);
                                screen_col = 0;
                            } else if col >= inner.x + inner.width {
                                self.ws_mut().preview.scroll_right(2);
                                screen_col = inner.width.saturating_sub(1);
                            }
                            if row < inner.y {
                                self.ws_mut().preview.scroll_up(1);
                                screen_row = 0;
                            } else if row >= inner.y + inner.height {
                                self.ws_mut().preview.scroll_down(1);
                                screen_row = inner.height.saturating_sub(1);
                            }

                            let scroll_v = self.ws().preview.scroll_offset.max(scroll_v);
                            let h_scroll = self.ws().preview.h_scroll_offset.max(h_scroll);
                            let lines_len = self.ws().preview.lines.len();
                            let abs_row =
                                (scroll_v + screen_row as usize).min(lines_len.saturating_sub(1));
                            let abs_col = screen_col as usize + h_scroll;
                            if let Some(sel) = self.selection.as_mut() {
                                sel.end_row = abs_row as u32;
                                sel.end_col = abs_col as u32;
                            }
                        }
                    }
                } else {
                    let pane_rects = self.ws().last_pane_rects.clone();
                    let mut started = false;
                    for (pane_id, rect) in pane_rects {
                        if col >= rect.x
                            && col < rect.x + rect.width
                            && row >= rect.y
                            && row < rect.y + rect.height
                        {
                            let inner = Rect::new(
                                rect.x + 1,
                                rect.y + 1,
                                rect.width.saturating_sub(2),
                                rect.height.saturating_sub(2),
                            );
                            let cell_col = col.saturating_sub(inner.x) as u32;
                            let cell_row = row.saturating_sub(inner.y) as u32;
                            self.selection = Some(TextSelection {
                                target: SelectionTarget::Pane(pane_id),
                                start_row: cell_row,
                                start_col: cell_col,
                                end_row: cell_row,
                                end_col: cell_col,
                                content_rect: inner,
                            });
                            started = true;
                            break;
                        }
                    }
                    if !started {
                        if let Some(rect) = self.ws().last_preview_rect {
                            if col >= rect.x
                                && col < rect.x + rect.width
                                && row >= rect.y
                                && row < rect.y + rect.height
                            {
                                const GUTTER: u16 = 5;
                                let inner = Rect::new(
                                    rect.x + 1 + GUTTER,
                                    rect.y + 1,
                                    rect.width.saturating_sub(2 + GUTTER),
                                    rect.height.saturating_sub(2),
                                );
                                if col >= inner.x && row >= inner.y {
                                    let screen_col = col.saturating_sub(inner.x);
                                    let screen_row = row.saturating_sub(inner.y);
                                    let scroll_v = self.ws().preview.scroll_offset;
                                    let h_scroll = self.ws().preview.h_scroll_offset;
                                    let lines_len = self.ws().preview.lines.len();
                                    let abs_row = (scroll_v + screen_row as usize)
                                        .min(lines_len.saturating_sub(1));
                                    let abs_col = screen_col as usize + h_scroll;
                                    self.selection = Some(TextSelection {
                                        target: SelectionTarget::Preview,
                                        start_row: abs_row as u32,
                                        start_col: abs_col as u32,
                                        end_row: abs_row as u32,
                                        end_col: abs_col as u32,
                                        content_rect: inner,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = None;

                if let Some(sel) = self.selection.clone() {
                    let (sr, sc, er, ec) = sel.normalized();
                    if sr != er || sc != ec {
                        let text = match sel.target {
                            SelectionTarget::Pane(pane_id) => self
                                .ws()
                                .panes
                                .get(&pane_id)
                                .map(|p| extract_selected_text(p, sr, sc, er, ec))
                                .unwrap_or_default(),
                            SelectionTarget::Preview => {
                                extract_preview_selected_text(&self.ws().preview, sr, sc, er, ec)
                            }
                        };
                        if !text.is_empty() {
                            self.copy_to_clipboard(&text);
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => self.handle_wheel(mouse.column, mouse.row, false),
            MouseEventKind::ScrollDown => self.handle_wheel(mouse.column, mouse.row, true),
            MouseEventKind::ScrollLeft => {
                let col = mouse.column;
                let row = mouse.row;
                if let Some(rect) = self.ws().last_preview_rect {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        self.ws_mut().preview.scroll_left(4);
                    }
                }
            }
            MouseEventKind::ScrollRight => {
                let col = mouse.column;
                let row = mouse.row;
                if let Some(rect) = self.ws().last_preview_rect {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        self.ws_mut().preview.scroll_right(4);
                    }
                }
            }
            MouseEventKind::Moved => {
                let col = mouse.column;
                let old_hover = self.hover_border.clone();
                if self.is_on_file_tree_border(col) {
                    self.hover_border = Some(DragTarget::FileTreeBorder);
                } else if self.is_on_preview_border(col) {
                    self.hover_border = Some(DragTarget::PreviewBorder);
                } else {
                    self.hover_border = None;
                }
                if self.hover_border != old_hover {
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    fn handle_wheel(&mut self, col: u16, row: u16, scroll_down: bool) {
        if let Some(rect) = self.ws().last_file_tree_rect {
            if col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
            {
                if scroll_down {
                    self.ws_mut().file_tree.scroll_down(3);
                } else {
                    self.ws_mut().file_tree.scroll_up(3);
                }
                return;
            }
        }
        if let Some(rect) = self.ws().last_preview_rect {
            if col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
            {
                if scroll_down {
                    self.ws_mut().preview.scroll_down(3);
                } else {
                    self.ws_mut().preview.scroll_up(3);
                }
                return;
            }
        }

        let disable_forward = mouse_forward_disabled();
        let pane_rects = self.ws().last_pane_rects.clone();
        for (pane_id, rect) in pane_rects {
            if !(col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height)
            {
                continue;
            }
            let local_col = col.saturating_sub(rect.x).saturating_sub(1);
            let local_row = row.saturating_sub(rect.y).saturating_sub(1);
            let codex_hint = self.pane_expects_codex_peer_delivery(self.active_tab, pane_id);

            let bytes = if disable_forward {
                None
            } else {
                self.ws().panes.get(&pane_id).and_then(|p| {
                    p.wheel_forward_bytes(codex_hint, scroll_down, local_col, local_row)
                })
            };

            if let Some(data) = bytes {
                if let Some(pane) = self.ws_mut().panes.get_mut(&pane_id) {
                    let _ = pane.write_input(&data);
                    self.dirty = true;
                }
            } else if let Some(pane) = self.ws().panes.get(&pane_id) {
                if scroll_down {
                    pane.scroll_down(3);
                } else {
                    pane.scroll_up(3);
                }
                self.dirty = true;
            }
            return;
        }
    }

    fn try_forward_pane_press(
        &mut self,
        pane_id: usize,
        rect: Rect,
        button: PointerButton,
        col: u16,
        row: u16,
    ) -> bool {
        let (local_col, local_row) = match pane_local_coords(rect, col, row) {
            Some(lc) => lc,
            None => return false,
        };
        let bytes = self.ws().panes.get(&pane_id).and_then(|p| {
            p.click_forward_bytes(
                self.pane_expects_codex_peer_delivery(self.active_tab, pane_id),
                button,
                PointerAction::Press,
                local_col,
                local_row,
            )
        });
        let Some(data) = bytes else {
            return false;
        };
        let ws_idx = self.active_tab;
        if let Some(pane) = self.ws_mut().panes.get_mut(&pane_id) {
            let _ = pane.write_input(&data);
            self.dragging = Some(DragTarget::PaneMouseReport(ws_idx, pane_id, rect, button));
            self.dirty = true;
            return true;
        }
        false
    }

    fn forward_pointer_to_pane(
        &mut self,
        ws_idx: usize,
        pane_id: usize,
        rect: Rect,
        button: PointerButton,
        action: PointerAction,
        mouse: &MouseEvent,
    ) {
        let (local_col, local_row) = pane_local_coords_clamped(rect, mouse.column, mouse.row);
        let bytes = self
            .workspaces
            .get(ws_idx)
            .and_then(|ws| ws.panes.get(&pane_id))
            .and_then(|p| {
                p.click_forward_bytes(
                    self.pane_expects_codex_peer_delivery(ws_idx, pane_id),
                    button,
                    action,
                    local_col,
                    local_row,
                )
            });
        let Some(data) = bytes else {
            return;
        };
        if let Some(pane) = self
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.panes.get_mut(&pane_id))
        {
            let _ = pane.write_input(&data);
            if ws_idx == self.active_tab {
                self.dirty = true;
            }
        }
    }
}

pub(crate) fn mouse_forward_disabled() -> bool {
    std::env::var("RENGA_DISABLE_MOUSE_FORWARD")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

pub(crate) fn pane_local_coords(rect: Rect, col: u16, row: u16) -> Option<(u16, u16)> {
    if rect.width < 3 || rect.height < 3 {
        return None;
    }
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);
    if col <= rect.x || col.saturating_add(1) >= right {
        return None;
    }
    if row <= rect.y || row.saturating_add(1) >= bottom {
        return None;
    }
    Some((col - rect.x - 1, row - rect.y - 1))
}

pub(crate) fn pane_local_coords_clamped(rect: Rect, col: u16, row: u16) -> (u16, u16) {
    let inner_x = rect.x.saturating_add(1);
    let inner_y = rect.y.saturating_add(1);
    let inner_w = rect.width.saturating_sub(2);
    let inner_h = rect.height.saturating_sub(2);
    let max_col = inner_w.saturating_sub(1);
    let max_row = inner_h.saturating_sub(1);
    let local_col = col.saturating_sub(inner_x).min(max_col);
    let local_row = row.saturating_sub(inner_y).min(max_row);
    (local_col, local_row)
}
