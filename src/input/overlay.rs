//! IME composition overlay state and its modal key handler.
//!
//! Extracted from `src/app.rs` as the first slice of Issue #66. The
//! module owns [`OverlayState`] and [`handle_overlay_key`]; `App` still
//! owns the `Option<OverlayState>` field and the open/close call sites,
//! which keeps this extraction strictly behavior-preserving.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

/// Phase 4b overlay state. When present, ccmux reserves a one-line
/// bottom row as a plain text-input widget so the host terminal's
/// IME attaches its candidate window to a concrete position the
/// user actually sees (Issue #25). The buffer holds the in-progress
/// composition; on Enter we send it to `target_pane` via
/// [`App::forward_paste_to_pty`] (bracketed paste when the PTY
/// supports it, raw bytes otherwise) and close the overlay.
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

    /// Insert `ch` at the overlay cursor and advance.
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

    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor = self.buffer.chars().count();
    }
}

/// Modal key handling while the IME composition overlay is open.
/// Commits with Enter, cancels with Esc or Ctrl+C, edits the
/// buffer with Backspace / Arrow / Home / End, inserts other
/// printable characters (Shift allowed, Ctrl/Alt modifiers
/// ignored so ccmux chords can't sneak into the buffer). On
/// commit, forwards the buffer to the original target pane via
/// the existing bracketed-paste path, which matches how Claude
/// Code already handles multi-character input.
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

    // Commit.
    if matches!(key.code, KeyCode::Enter) {
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
        KeyCode::Home => overlay.cursor_home(),
        KeyCode::End => overlay.cursor_end(),
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
            // can't grow the buffer without limit.
            if overlay.buffer.chars().count() < 1024 {
                overlay.insert_char(c);
            }
        }
        _ => return Ok(true),
    }
    app.dirty = true;
    Ok(true)
}
