//! IME composition overlay state and its modal key handler.
//!
//! Extracted from `src/app.rs` as the first slice of Issue #66. The
//! module owns [`OverlayState`] and [`handle_overlay_key`]; `App` still
//! owns the `Option<OverlayState>` field and the open/close call sites.
//!
//! The overlay is a centered multi-line composition box. Enter inserts
//! a newline; Alt+Enter (portable across all tier-1 terminals) or
//! Ctrl+Enter (Windows Terminal / wezterm / VS Code) commit the
//! buffer. The host terminal's IME candidate window anchors to the
//! caret inside the box.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

/// Multi-line IME composition buffer. The buffer stores `\n` as a
/// regular character; cursor is a char offset into the buffer, so
/// Japanese/Chinese multibyte text edits correctly. See
/// [`handle_overlay_key`] for the key-routing contract.
#[derive(Debug, Clone)]
pub struct OverlayState {
    /// Pane id the overlay will commit its buffer to. Stored at open
    /// time so focus changes during composition don't reroute the
    /// text; the overlay always commits to the pane that originated
    /// it.
    pub target_pane: usize,
    /// Composed characters so far, indexed by character (not byte),
    /// so inserting multibyte text and editing via arrow keys
    /// behaves intuitively for Japanese/Chinese composition.
    pub buffer: String,
    /// Cursor position in `buffer`, measured in characters. Valid
    /// range: `0..=buffer.chars().count()`.
    pub cursor: usize,
}

impl OverlayState {
    pub fn new(target_pane: usize) -> Self {
        Self {
            target_pane,
            buffer: String::new(),
            cursor: 0,
        }
    }

    /// Insert `ch` at the overlay cursor and advance. `'\n'` is a
    /// regular character — call `insert_char('\n')` to add a line
    /// break.
    pub fn insert_char(&mut self, ch: char) {
        let byte_idx = self
            .buffer
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.buffer.len());
        self.buffer.insert(byte_idx, ch);
        self.cursor += 1;
    }

    /// Backspace: remove the char left of the cursor.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.cursor - 1;
        let (start, ch) = match self.buffer.char_indices().nth(prev) {
            Some(pair) => pair,
            None => return,
        };
        let end = start + ch.len_utf8();
        self.buffer.replace_range(start..end, "");
        self.cursor = prev;
    }

    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn cursor_right(&mut self) {
        let len = self.buffer.chars().count();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    /// Home in a multi-line buffer moves to the start of the current
    /// line, not the start of the whole buffer.
    pub fn cursor_home(&mut self) {
        let (_line, col) = self.line_col();
        self.cursor -= col;
    }

    /// End moves to the end of the current line (just before the
    /// next `\n`, or buffer end).
    pub fn cursor_end(&mut self) {
        // Walk the char iterator from the current cursor — avoid
        // materializing a Vec<char> for the whole buffer just to
        // scan a single line.
        let additional = self
            .buffer
            .chars()
            .skip(self.cursor)
            .take_while(|c| *c != '\n')
            .count();
        self.cursor += additional;
    }

    /// Absolute start of the buffer — multi-line equivalent of
    /// `Ctrl+Home` in a traditional editor.
    pub fn cursor_buffer_start(&mut self) {
        self.cursor = 0;
    }

    /// Absolute end of the buffer — multi-line equivalent of
    /// `Ctrl+End`.
    pub fn cursor_buffer_end(&mut self) {
        self.cursor = self.buffer.chars().count();
    }

    /// Arrow-up: move to the same column on the previous line. If
    /// already on the first line, collapse to column 0 (home of
    /// buffer) so a stuck user can always escape upward.
    pub fn cursor_up(&mut self) {
        let (line, col) = self.line_col();
        if line == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = self.line_col_to_cursor(line - 1, col);
    }

    /// Arrow-down: move to the same column on the next line. On the
    /// last line, jump to buffer end so Down is always meaningful.
    pub fn cursor_down(&mut self) {
        let (line, col) = self.line_col();
        let total_lines = self.total_lines();
        if line + 1 >= total_lines {
            self.cursor = self.buffer.chars().count();
            return;
        }
        self.cursor = self.line_col_to_cursor(line + 1, col);
    }

    /// Current cursor as (0-based line, 0-based column). Column is
    /// the number of chars on the current line before the cursor.
    pub fn line_col(&self) -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        for (i, ch) in self.buffer.chars().enumerate() {
            if i >= self.cursor {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Convert a (line, col) target back to a char offset, clamping
    /// col to the length of the requested line (so Up/Down over a
    /// shorter line lands at the line's end, matching editor norms).
    fn line_col_to_cursor(&self, target_line: usize, target_col: usize) -> usize {
        let mut line = 0usize;
        let mut col = 0usize;
        let mut last_in_target_line: Option<usize> = None;
        for (i, ch) in self.buffer.chars().enumerate() {
            if line == target_line {
                if col == target_col {
                    return i;
                }
                last_in_target_line = Some(i);
            }
            if ch == '\n' {
                if line == target_line {
                    // Reached end of target line before hitting the
                    // requested column — clamp to the line-end
                    // position (just before the newline).
                    return i;
                }
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        // Ran off the end of the buffer. If we were ever on the
        // target line, clamp to buffer end; otherwise the target
        // line didn't exist (shouldn't happen — callers check
        // total_lines).
        if line == target_line || last_in_target_line.is_some() {
            self.buffer.chars().count()
        } else {
            self.cursor
        }
    }

    /// Total number of logical lines = `\n` count + 1. An empty
    /// buffer has 1 line.
    pub fn total_lines(&self) -> usize {
        self.buffer.chars().filter(|c| *c == '\n').count() + 1
    }
}

/// Modal key handling while the IME composition overlay is open.
///
/// Commits with **Alt+Enter** (portable across all tier-1 terminals,
/// including macOS Option+Return) or **Ctrl+Enter** (Windows Terminal
/// / wezterm / VS Code / most Linux terminals). Bare `Enter` inserts
/// a newline into the composition buffer. Cancels with Esc or
/// Ctrl+C. Arrow keys navigate, Backspace deletes, other printable
/// characters are inserted; Ctrl/Alt-modified chars are swallowed so
/// ccmux chords can't leak mid-composition. On commit, forwards the
/// buffer to the original target pane via the existing
/// bracketed-paste path.
pub(crate) fn handle_overlay_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    let overlay = match app.overlay.as_mut() {
        Some(o) => o,
        None => return Ok(false),
    };

    // Cancel (Esc or Ctrl+C).
    if matches!(key.code, KeyCode::Esc)
        || (key.modifiers == KeyModifiers::CONTROL && matches!(key.code, KeyCode::Char('c')))
    {
        let is_always = app.ime_mode == crate::config::ImeMode::Always;
        let target_pane = overlay.target_pane;
        let buffer_empty = overlay.buffer.is_empty();

        if is_always && !buffer_empty {
            // First press on a composing overlay clears the buffer
            // but keeps the overlay open. The user needs a second
            // press to actually dismiss, which mirrors the common
            // "Esc once to abort composition, twice to exit" IME
            // expectation.
            overlay.buffer.clear();
            overlay.cursor = 0;
            app.mark_layout_change();
            return Ok(true);
        }

        app.overlay = None;
        app.mark_layout_change();

        if is_always {
            // Always mode would re-open the overlay on the next
            // tick of maybe_auto_open_always_overlay. Record the
            // explicit dismissal so the user gets a window to
            // interact with the pane directly (Claude Esc-to-
            // interrupt, shell-level Ctrl+C, …). The suppression
            // clears when focus moves away and comes back.
            app.always_dismissed_pane = Some(target_pane);
            // Forward the cancel key to the pane so the user's
            // intent (interrupt Claude, send Ctrl+C to shell)
            // reaches its real target. Only on an already-empty
            // buffer, because a non-empty buffer was clearing
            // composition, not interacting with the pane. Propagate
            // the write error (same policy as the Enter-commit
            // path) instead of silently swallowing it.
            let focused_before = app.ws().focused_pane_id;
            app.ws_mut().focused_pane_id = target_pane;
            let forward_result = app.forward_key_to_pty(key);
            app.ws_mut().focused_pane_id = focused_before;
            forward_result?;
        }
        return Ok(true);
    }

    // Enter handling: Alt+Enter / Ctrl+Enter / Cmd+Enter (kitty
    // keyboard protocol only) commit the buffer. Bare Enter inserts
    // a newline into the multi-line composition area.
    if matches!(key.code, KeyCode::Enter) {
        let is_commit = key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER);
        if !is_commit {
            // Shift+Enter also inserts a newline — matches chat-app
            // conventions and keeps the "no commit modifier" rule
            // intuitive.
            if overlay.buffer.chars().count() < 4096 {
                overlay.insert_char('\n');
            }
            app.dirty = true;
            return Ok(true);
        }

        let target_pane = overlay.target_pane;
        let buffer = std::mem::take(&mut overlay.buffer);

        // Target sanity check — if the pane disappeared (close tab,
        // shell exit, …) while the user was composing, don't
        // silently discard their input. Keep the overlay open with
        // the buffer restored and fall out of this frame so the
        // user can recover. The buffer was `mem::take`'d above, so
        // put it back.
        let target_alive = app
            .ws()
            .panes
            .get(&target_pane)
            .map(|p| !p.exited)
            .unwrap_or(false);
        if !target_alive {
            if let Some(o) = app.overlay.as_mut() {
                o.buffer = buffer;
                o.cursor = o.buffer.chars().count();
            }
            app.dirty = true;
            return Ok(true);
        }

        // Target alive: close the overlay and deliver. If the
        // paste write fails (PTY closed mid-send, very rare), the
        // error propagates so the top-level render loop can log
        // or surface it instead of the text being dropped
        // silently.
        app.overlay = None;
        let mut commit_result: Result<()> = Ok(());
        if !buffer.is_empty() {
            let focused_before = app.ws().focused_pane_id;
            // forward_paste_to_pty writes to the currently-focused
            // pane, so temporarily refocus the overlay's target so
            // the paste reaches the right pane even if focus moved.
            app.ws_mut().focused_pane_id = target_pane;
            commit_result = app.forward_paste_to_pty(&buffer);
            app.ws_mut().focused_pane_id = focused_before;
        }
        app.mark_layout_change();
        commit_result?;
        return Ok(true);
    }

    // Edit.
    match key.code {
        KeyCode::Backspace => overlay.backspace(),
        KeyCode::Left => overlay.cursor_left(),
        KeyCode::Right => overlay.cursor_right(),
        KeyCode::Up => overlay.cursor_up(),
        KeyCode::Down => overlay.cursor_down(),
        KeyCode::Home => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                overlay.cursor_buffer_start();
            } else {
                overlay.cursor_home();
            }
        }
        KeyCode::End => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                overlay.cursor_buffer_end();
            } else {
                overlay.cursor_end();
            }
        }
        KeyCode::Char(c) => {
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                // Don't leak ccmux chord keys (Ctrl+D, Alt+T …)
                // into the buffer, but also don't let them
                // trigger ccmux's layout commands mid-composition
                // — the overlay is modal.
                return Ok(true);
            }
            // Cap at a generous but bounded size so a stuck key
            // can't grow the buffer without limit. Multi-line drafts
            // need more headroom than the old single-line overlay,
            // so the cap is bumped to 4096 chars.
            if overlay.buffer.chars().count() < 4096 {
                overlay.insert_char(c);
            }
        }
        _ => return Ok(true),
    }
    app.dirty = true;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_newline_is_regular_char() {
        let mut o = OverlayState::new(0);
        o.insert_char('a');
        o.insert_char('\n');
        o.insert_char('b');
        assert_eq!(o.buffer, "a\nb");
        assert_eq!(o.cursor, 3);
        assert_eq!(o.total_lines(), 2);
    }

    #[test]
    fn line_col_tracks_newlines() {
        let mut o = OverlayState::new(0);
        for ch in "abc\ndef\ngh".chars() {
            o.insert_char(ch);
        }
        // cursor at end: line 2, col 2
        assert_eq!(o.line_col(), (2, 2));

        o.cursor = 0;
        assert_eq!(o.line_col(), (0, 0));

        o.cursor = 4; // just after first \n
        assert_eq!(o.line_col(), (1, 0));
    }

    #[test]
    fn cursor_up_preserves_column_when_possible() {
        let mut o = OverlayState::new(0);
        for ch in "abcdef\nghi".chars() {
            o.insert_char(ch);
        }
        // cursor at end of "ghi" → (1, 3)
        assert_eq!(o.line_col(), (1, 3));
        o.cursor_up();
        // should land at (0, 3), i.e. inside "abcdef" at col 3
        assert_eq!(o.line_col(), (0, 3));
    }

    #[test]
    fn cursor_up_clamps_when_target_line_shorter() {
        let mut o = OverlayState::new(0);
        for ch in "ab\n12345".chars() {
            o.insert_char(ch);
        }
        // cursor at end of line 1 → (1, 5)
        assert_eq!(o.line_col(), (1, 5));
        o.cursor_up();
        // line 0 only has "ab" → clamp to line-end (1, 2)
        assert_eq!(o.line_col(), (0, 2));
    }

    #[test]
    fn cursor_up_on_first_line_goes_home() {
        let mut o = OverlayState::new(0);
        for ch in "abc".chars() {
            o.insert_char(ch);
        }
        o.cursor_up();
        assert_eq!(o.cursor, 0);
    }

    #[test]
    fn cursor_down_on_last_line_goes_end() {
        let mut o = OverlayState::new(0);
        for ch in "abc\nde".chars() {
            o.insert_char(ch);
        }
        o.cursor = 5; // inside "de" at col 1 (after 'd')
        o.cursor_down();
        assert_eq!(o.cursor, 6, "last-line Down should land at buffer end");
    }

    #[test]
    fn home_end_respect_current_line() {
        let mut o = OverlayState::new(0);
        for ch in "abc\ndef".chars() {
            o.insert_char(ch);
        }
        // cursor at end of line 1
        o.cursor_home();
        assert_eq!(o.line_col(), (1, 0), "Home should go to start of line");
        o.cursor_end();
        assert_eq!(o.line_col(), (1, 3), "End should go to end of line");

        // From start of line 1, Home stays put; End moves to end.
        o.cursor = 4;
        o.cursor_home();
        assert_eq!(o.cursor, 4);
    }

    #[test]
    fn buffer_start_end_navigate_whole_buffer() {
        let mut o = OverlayState::new(0);
        for ch in "abc\ndef\nghi".chars() {
            o.insert_char(ch);
        }
        o.cursor = 5; // middle of line 1
        o.cursor_buffer_start();
        assert_eq!(o.cursor, 0);
        o.cursor_buffer_end();
        assert_eq!(o.cursor, 11);
    }

    #[test]
    fn backspace_across_newline() {
        let mut o = OverlayState::new(0);
        for ch in "abc\n".chars() {
            o.insert_char(ch);
        }
        assert_eq!(o.total_lines(), 2);
        o.backspace();
        assert_eq!(o.buffer, "abc");
        assert_eq!(o.total_lines(), 1);
    }
}
