use super::*;

impl App {
    pub(crate) fn clear_selection_if_preview(&mut self) {
        if matches!(
            self.selection.as_ref().map(|s| &s.target),
            Some(SelectionTarget::Preview)
        ) {
            self.selection = None;
        }
    }

    pub(crate) fn handle_rename_key(&mut self, key: KeyEvent) -> bool {
        let Some(buf) = self.rename_input.as_mut() else {
            return false;
        };
        let needs_relayout = !self.status_bar_visible;
        match key.code {
            KeyCode::Esc => {
                self.rename_input = None;
                if needs_relayout {
                    self.mark_layout_change();
                }
            }
            KeyCode::Enter => {
                let trimmed = buf.trim().to_string();
                self.ws_mut().custom_name = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                };
                self.rename_input = None;
                if needs_relayout {
                    self.mark_layout_change();
                }
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return true;
                }
                if buf.chars().count() < 32 {
                    buf.push(c);
                }
            }
            _ => return true,
        }
        self.dirty = true;
        true
    }

    pub(crate) fn handle_file_tree_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.ws_mut().file_tree.move_down();
                Ok(true)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.ws_mut().file_tree.move_up();
                Ok(true)
            }
            KeyCode::Enter => {
                let path = self.ws_mut().file_tree.toggle_or_select();
                if let Some(path) = path {
                    self.clear_selection_if_preview();
                    let messages = self.messages();
                    let mut picker = self.image_picker.take();
                    self.ws_mut().preview.load(&path, picker.as_mut(), messages);
                    self.image_picker = picker;
                }
                Ok(true)
            }
            KeyCode::Char('.') => {
                self.ws_mut().file_tree.toggle_hidden();
                Ok(true)
            }
            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                self.spawn_claude_in_selected_dir(SplitDirection::Vertical)?;
                Ok(true)
            }
            KeyCode::Char('v') if key.modifiers == KeyModifiers::NONE => {
                self.spawn_claude_in_selected_dir(SplitDirection::Horizontal)?;
                Ok(true)
            }
            KeyCode::Char('h') => {
                self.ws_mut().file_tree.go_to_parent();
                Ok(true)
            }
            KeyCode::Char('l') => {
                self.ws_mut().file_tree.descend_into_selected();
                Ok(true)
            }
            KeyCode::Esc => {
                self.ws_mut().focus_target = FocusTarget::Pane;
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    pub(crate) fn handle_preview_key(&mut self, key: KeyEvent) -> Result<bool> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                self.clear_selection_if_preview();
                self.ws_mut().preview.close();
                self.ws_mut().focus_target = FocusTarget::Pane;
                Ok(true)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                self.layout_swapped = !self.layout_swapped;
                Ok(true)
            }
            (_, KeyCode::Char('j')) | (_, KeyCode::Down) => {
                self.ws_mut().preview.scroll_down(1);
                Ok(true)
            }
            (_, KeyCode::Char('k')) | (_, KeyCode::Up) => {
                self.ws_mut().preview.scroll_up(1);
                Ok(true)
            }
            (_, KeyCode::PageDown) => {
                self.ws_mut().preview.scroll_down(20);
                Ok(true)
            }
            (_, KeyCode::PageUp) => {
                self.ws_mut().preview.scroll_up(20);
                Ok(true)
            }
            (KeyModifiers::NONE, KeyCode::Right)
            | (KeyModifiers::NONE, KeyCode::Char('l'))
            | (KeyModifiers::SHIFT, KeyCode::Right) => {
                self.ws_mut().preview.scroll_right(4);
                Ok(true)
            }
            (KeyModifiers::NONE, KeyCode::Left)
            | (KeyModifiers::NONE, KeyCode::Char('h'))
            | (KeyModifiers::SHIFT, KeyCode::Left) => {
                self.ws_mut().preview.scroll_left(4);
                Ok(true)
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.ws_mut().preview.h_scroll_offset = 0;
                Ok(true)
            }
            (_, KeyCode::Esc) => {
                self.ws_mut().focus_target = FocusTarget::Pane;
                Ok(true)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                self.should_quit = true;
                Ok(true)
            }
            (KeyModifiers::CONTROL, KeyCode::Right) => {
                self.focus_next_pane();
                Ok(true)
            }
            (KeyModifiers::CONTROL, KeyCode::Left) => {
                self.focus_prev_pane();
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    pub(crate) fn toggle_file_tree(&mut self) {
        let ws = self.ws_mut();
        let was_visible = ws.file_tree_visible;
        let will_be_visible;
        if ws.file_tree_visible && ws.focus_target == FocusTarget::FileTree {
            ws.file_tree_visible = false;
            ws.focus_target = if ws.preview.is_active() {
                FocusTarget::Preview
            } else {
                FocusTarget::Pane
            };
            will_be_visible = false;
        } else if ws.file_tree_visible {
            ws.focus_target = FocusTarget::FileTree;
            will_be_visible = true;
        } else {
            ws.file_tree_visible = true;
            ws.focus_target = FocusTarget::FileTree;
            will_be_visible = true;
        }

        if was_visible != will_be_visible {
            self.mark_layout_change();
        }
    }
}
