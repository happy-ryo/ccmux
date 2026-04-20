use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::filetree::FileTree;
use crate::ipc::{self, PaneInfo, PaneRef};
use crate::layout_config::{DirectionSpec, LayoutConfig, LayoutNodeSpec};
use crate::pane::Pane;
use crate::preview::Preview;

/// Commands that flow from the IPC server thread into the App's event
/// loop. Each variant carries a `oneshot::Sender` so the server thread
/// can block-wait for the App to finish processing.
#[allow(dead_code)] // constructed by the IPC server (wired in Step 3.3)
#[derive(Debug)]
pub enum AppCommand {
    /// Snapshot the pane list of the active workspace.
    List {
        reply: oneshot::Sender<Vec<PaneInfo>>,
    },
    /// Write `data` to the target pane's PTY.
    Send {
        target: PaneRef,
        data: Vec<u8>,
        append_enter: bool,
        reply: oneshot::Sender<std::result::Result<(), ipc::CodedError>>,
    },
    /// Move keyboard focus to the target pane in the active workspace.
    Focus {
        target: PaneRef,
        reply: oneshot::Sender<std::result::Result<(), ipc::CodedError>>,
    },
    /// Split the target pane. If `command` is given, it's queued on the
    /// new pane and flushed when its shell prompt appears. If `name` is
    /// given, it's registered so later IPC calls can address the pane by
    /// name. Returns the new pane's id on success.
    Split {
        target: PaneRef,
        direction: ipc::Direction,
        command: Option<String>,
        name: Option<String>,
        role: Option<String>,
        reply: oneshot::Sender<std::result::Result<usize, ipc::CodedError>>,
    },
    /// Open a new tab with a fresh single pane. Focus switches to the
    /// new tab (mirrors the Alt+T keybinding). Returns the new pane's
    /// id on success.
    NewTab {
        command: Option<String>,
        name: Option<String>,
        label: Option<String>,
        role: Option<String>,
        reply: oneshot::Sender<std::result::Result<usize, ipc::CodedError>>,
    },
    /// Snapshot the visible screen of the target pane. See
    /// [`ipc::Request::Inspect`] for the response shape.
    Inspect {
        target: PaneRef,
        lines: Option<usize>,
        include_cursor: bool,
        reply: oneshot::Sender<std::result::Result<serde_json::Value, ipc::CodedError>>,
    },
    /// Close the target pane. Returns the id of the pane that was
    /// closed, so the caller can confirm which pane was resolved.
    Close {
        target: PaneRef,
        reply: oneshot::Sender<std::result::Result<usize, ipc::CodedError>>,
    },
}

/// Resolve a [`PaneRef`] to a concrete pane id.
///
/// Pure helper separated from `Workspace` so it can be unit-tested
/// without spawning real PTYs. Returns `None` when the reference points
/// at a pane that no longer exists (e.g. a stale name or a closed id).
pub(crate) fn resolve_pane_ref_impl(
    r: &PaneRef,
    pane_names: &HashMap<String, usize>,
    known_ids: &HashSet<usize>,
    focused: usize,
) -> Option<usize> {
    match r {
        PaneRef::Focused => known_ids.contains(&focused).then_some(focused),
        PaneRef::Id(id) => known_ids.contains(id).then_some(*id),
        PaneRef::Name(name) => pane_names
            .get(name)
            .copied()
            .filter(|id| known_ids.contains(id)),
    }
}

/// Events dispatched within the app.
pub enum AppEvent {
    /// PTY output received for a pane.
    PtyOutput(#[allow(dead_code)] usize),
    /// PTY process exited for a pane.
    PtyEof(usize),
    /// Shell changed working directory (pane_id, new path).
    CwdChanged(usize, PathBuf),
}

/// Split direction for layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

/// Which area has focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusTarget {
    Pane,
    FileTree,
    Preview,
}

/// Which border is being dragged.
#[derive(Debug, Clone, PartialEq)]
pub enum DragTarget {
    FileTreeBorder,
    PreviewBorder,
    PaneSplit(Vec<bool>, SplitDirection, Rect),
    Scrollbar(usize, Rect), // pane_id, inner area
}

// ─── Layout Tree ──────────────────────────────────────────

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

// ─── Text Selection ───────────────────────────────────────

/// What the current text selection is anchored to.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionTarget {
    Pane(usize),
    Preview,
}

/// Text selection state. Works for both terminal panes and the file
/// preview panel — `target` tells rendering and extraction which
/// source to read.
///
/// Coordinate semantics differ by target:
/// - **Pane**: start/end rows+cols are screen-relative to
///   `content_rect` (the inner area of the pane border).
/// - **Preview**: rows are **absolute line indices** into
///   `preview.lines`; cols are **char offsets** within the line.
///   This lets the selection survive vertical and horizontal
///   scrolling — overlay rendering subtracts the current scroll
///   to turn source coords back into screen coords.
#[derive(Debug, Clone)]
pub struct TextSelection {
    pub target: SelectionTarget,
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
    /// Content area used for coordinate mapping — the inside of the
    /// pane border, or (for previews) the area excluding the line
    /// number gutter.
    pub content_rect: Rect,
}

impl TextSelection {
    /// Get normalized (top-left to bottom-right) selection range.
    pub fn normalized(&self) -> (u32, u32, u32, u32) {
        if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_row, self.start_col, self.end_row, self.end_col)
        } else {
            (self.end_row, self.end_col, self.start_row, self.start_col)
        }
    }

    /// Check if a cell is within the selection.
    pub fn contains(&self, row: u32, col: u32) -> bool {
        let (sr, sc, er, ec) = self.normalized();
        if row < sr || row > er {
            return false;
        }
        if row == sr && row == er {
            return col >= sc && col <= ec;
        }
        if row == sr {
            return col >= sc;
        }
        if row == er {
            return col <= ec;
        }
        true
    }
}

// ─── Workspace (per-tab state) ────────────────────────────

/// A workspace holds all state for one tab.
#[allow(dead_code)]
pub struct Workspace {
    pub name: String,
    /// Session-only rename; when Some, takes precedence over `name` for
    /// display. Not persisted; `cd` does not touch this.
    pub custom_name: Option<String>,
    pub cwd: PathBuf,
    pub panes: HashMap<usize, Pane>,
    pub layout: LayoutNode,
    pub focused_pane_id: usize,
    /// Stable human-friendly name → pane id map, populated by layout
    /// files (Phase 2) and by IPC `Split { id: Some(name), .. }` calls
    /// (Phase 3). Entries are removed when the referenced pane is
    /// closed (see `remove_pane_from_layout` and `close_tab`) so a
    /// subsequent split can reuse the same name. Natural-exit PTY EOF
    /// leaves the entry until the pane is explicitly reaped — callers
    /// should treat an entry whose pane id is absent from `panes` as
    /// stale (which `resolve_pane_ref_impl` already does).
    pub pane_names: HashMap<String, usize>,
    pub file_tree: FileTree,
    pub file_tree_visible: bool,
    pub preview: Preview,
    pub focus_target: FocusTarget,
    // Cached rects (updated on each render)
    pub last_pane_rects: Vec<(usize, Rect)>,
    pub last_file_tree_rect: Option<Rect>,
    pub last_preview_rect: Option<Rect>,
}

impl Workspace {
    fn new(
        name: String,
        cwd: PathBuf,
        pane_id: usize,
        rows: u16,
        cols: u16,
        event_tx: Sender<AppEvent>,
    ) -> Result<Self> {
        let pane = Pane::new(pane_id, rows, cols, event_tx)?;
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);

        Ok(Self {
            name,
            custom_name: None,
            file_tree: FileTree::new(cwd.clone()),
            cwd,
            panes,
            layout: LayoutNode::Leaf { pane_id },
            focused_pane_id: pane_id,
            pane_names: HashMap::new(),
            file_tree_visible: true,
            preview: Preview::new(),
            focus_target: FocusTarget::Pane,
            last_pane_rects: Vec::new(),
            last_file_tree_rect: None,
            last_preview_rect: None,
        })
    }

    /// Resolve a [`PaneRef`] against this workspace's panes and names.
    pub fn resolve_pane_ref(&self, r: &PaneRef) -> Option<usize> {
        let known: HashSet<usize> = self.panes.keys().copied().collect();
        resolve_pane_ref_impl(r, &self.pane_names, &known, self.focused_pane_id)
    }

    fn shutdown(&mut self) {
        for pane in self.panes.values_mut() {
            pane.kill();
        }
    }

    /// Tab label to show in the UI: custom rename wins over the
    /// cwd-derived name.
    pub fn display_name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.name)
    }
}

// ─── IME composition overlay ──────────────────────────────

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

// ─── App (global state) ───────────────────────────────────

pub struct App {
    pub workspaces: Vec<Workspace>,
    pub active_tab: usize,
    pub should_quit: bool,
    pub event_tx: Sender<AppEvent>,
    pub event_rx: Receiver<AppEvent>,
    /// Clonable sender for the IPC server thread. Drop the server thread
    /// to stop producing commands; the receiver lives on the App side.
    pub command_tx: Sender<AppCommand>,
    command_rx: Receiver<AppCommand>,
    next_pane_id: usize,
    pub dirty: bool,
    pub paste_cooldown: u8, // frames to skip rendering after paste
    /// Frames to skip rendering after a layout change (split, close,
    /// sidebar toggle, terminal resize). Gives Claude Code / bash time
    /// to process SIGWINCH and send a fresh redraw before we paint,
    /// avoiding the brief "old buffer at new size" garbled frame.
    pub resize_cooldown: u8,
    /// Last known terminal size (cols, rows). Updated from main.rs on
    /// Event::Resize and from ui::render on every frame. Used by
    /// `relayout_panes()` so layout-change handlers can resize PTYs
    /// without needing a Frame reference.
    pub last_term_size: (u16, u16),
    // Shared settings
    pub file_tree_width: u16,
    pub preview_width: u16,
    // Layout: swap preview and terminal positions
    pub layout_swapped: bool,
    // Toggle status bar visibility (Alt+S)
    pub status_bar_visible: bool,
    // Drag/hover state
    pub dragging: Option<DragTarget>,
    pub hover_border: Option<DragTarget>,
    // Tab bar rects for mouse click
    pub last_tab_rects: Vec<(usize, Rect)>,
    pub last_new_tab_rect: Option<Rect>,
    /// Active tab rename input buffer. When `Some`, key input is
    /// routed to this buffer instead of the focused PTY; Enter commits
    /// to the active workspace's `custom_name`, Esc cancels.
    pub rename_input: Option<String>,
    /// IME composition overlay. When `Some`, key input is routed into
    /// this buffer instead of the focused PTY; the overlay reserves a
    /// bottom row so the host terminal's IME candidate window has a
    /// concrete text-input widget to anchor to (Issue #25 / Phase 4b).
    /// Enter commits the composed text to the target pane via the
    /// existing bracketed-paste path; Esc / Ctrl+C cancels.
    pub overlay: Option<OverlayState>,
    /// (tab index, timestamp) of the last left-click on a tab label.
    /// Used to detect a double-click → enter rename mode.
    last_tab_click: Option<(usize, Instant)>,
    // Text selection
    pub selection: Option<TextSelection>,
    // Version check (background)
    pub version_info: crate::version_check::VersionInfo,
    // Claude Code JSONL monitoring
    pub claude_monitor: crate::claude_monitor::ClaudeMonitor,
    // Reusable clipboard handle (lazy-initialized)
    clipboard: Option<arboard::Clipboard>,
    // Pane lifecycle event bus shared with IPC subscribers.
    pub event_bus: crate::ipc::EventBus,
    /// IME overlay mode resolved from config + CLI. `Off` disables
    /// the Ctrl+; hotkey so the keystroke reaches the PTY untouched.
    pub ime_mode: crate::config::ImeMode,
    /// Resolved main-loop `event::poll` timeout (ms) to use while the
    /// IME composition overlay is open. Populated from config + CLI
    /// by [`App::apply_config`] and consumed by the main loop in
    /// `src/main.rs`. See Issue #38.
    pub ime_overlay_poll_ms: u64,
    /// In `Always` mode, the pane id where the user explicitly
    /// dismissed (Esc with empty buffer / Ctrl+C) the auto-opened
    /// overlay. Blocks re-open on the same focus. Cleared whenever
    /// focus moves to a different pane so the overlay reappears the
    /// next time the user returns.
    always_dismissed_pane: Option<usize>,
    /// Pane id that held focus the last time we observed it. Used
    /// by `maybe_auto_open_always_overlay` to detect focus changes
    /// and clear `always_dismissed_pane`.
    last_focused_pane: Option<usize>,
    /// Minimum width (cols) each child must retain after a vertical
    /// split. Populated from `--min-pane-width`; `0` is clamped to `1`
    /// in `set_min_pane_size` to avoid degenerate halving math.
    /// Private — the setter is the only supported entry point so the
    /// clamp invariant cannot be bypassed.
    min_pane_width: u16,
    /// Minimum height (rows) each child must retain after a horizontal
    /// split. See [`App::set_min_pane_size`] for the clamp rule.
    min_pane_height: u16,
}

impl App {
    pub fn new(rows: u16, cols: u16) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();

        let pane_rows = rows.saturating_sub(5); // title + tab bar + status + borders
        let pane_cols = cols.saturating_sub(2);

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let name = dir_name(&cwd);

        let ws = Workspace::new(name, cwd, 1, pane_rows, pane_cols, event_tx.clone())?;

        let event_bus = crate::ipc::EventBus::new();
        // Initial pane already exists at this point; emit its
        // PaneStarted so subscribers joining immediately after App
        // construction see it.
        event_bus.emit(crate::ipc::Event::PaneStarted {
            id: 1,
            name: None,
            role: None,
            ts_ms: crate::ipc::events::now_ms(),
        });

        Ok(Self {
            workspaces: vec![ws],
            active_tab: 0,
            should_quit: false,
            event_tx,
            event_rx,
            command_tx,
            command_rx,
            next_pane_id: 2,
            dirty: true,
            paste_cooldown: 0,
            resize_cooldown: 0,
            last_term_size: (cols, rows),
            file_tree_width: 20,
            preview_width: 40,
            layout_swapped: true,
            status_bar_visible: true,
            dragging: None,
            hover_border: None,
            last_tab_rects: Vec::new(),
            last_new_tab_rect: None,
            rename_input: None,
            overlay: None,
            last_tab_click: None,
            selection: None,
            version_info: {
                let info = crate::version_check::VersionInfo::new();
                crate::version_check::spawn_check(info.clone());
                info
            },
            claude_monitor: crate::claude_monitor::ClaudeMonitor::new(),
            clipboard: None,
            event_bus,
            ime_mode: crate::config::ImeMode::default(),
            ime_overlay_poll_ms: crate::config::DEFAULT_OVERLAY_POLL_MS,
            always_dismissed_pane: None,
            last_focused_pane: None,
            min_pane_width: 20,
            min_pane_height: 5,
        })
    }

    /// Install a user-level config on top of the default App state.
    /// Called by `main` right after [`App::new`] so the CLI / config
    /// precedence in `config::Config::apply_cli_overrides` has already
    /// collapsed into a single resolved value.
    pub fn apply_config(&mut self, cfg: &crate::config::Config) {
        self.ime_mode = cfg.ime.mode;
        // Config::apply_cli_overrides already clamped this to
        // MIN_OVERLAY_POLL_MS, but re-apply the floor here so direct
        // tests that mutate cfg.ime.overlay_poll_ms without going
        // through the setter still get a safe value.
        self.ime_overlay_poll_ms = cfg
            .ime
            .overlay_poll_ms
            .max(crate::config::MIN_OVERLAY_POLL_MS);
    }

    /// Override the minimum per-child split dimensions. Values of `0`
    /// are clamped to `1` so `rect.width / 2 < min` stays meaningful
    /// (`0` would let splits succeed on a 1-column pane and produce
    /// zero-width children).
    pub fn set_min_pane_size(&mut self, width: u16, height: u16) {
        self.min_pane_width = width.max(1);
        self.min_pane_height = height.max(1);
    }

    /// In `Always` mode, make sure the IME overlay is open whenever
    /// focus rests on a non-scrolled Claude pane. This is the
    /// JP-IME-friendly behavior: the overlay provides the IME anchor
    /// from the moment focus lands, so the terminal's IME composes
    /// directly into it without the user having to press Ctrl+;
    /// first. Idempotent; safe to call on every tick.
    ///
    /// Dismissal: a user-initiated close on a pane (Esc/Ctrl+C with
    /// empty buffer — see `handle_overlay_key`) records that pane in
    /// `always_dismissed_pane` so we don't immediately re-open. The
    /// suppression clears the next time focus moves to a different
    /// pane; returning refreshes the overlay.
    pub fn maybe_auto_open_always_overlay(&mut self) {
        if self.ime_mode != crate::config::ImeMode::Always {
            return;
        }
        let focused_id = self.ws().focused_pane_id;
        let pane_focused = matches!(self.ws().focus_target, FocusTarget::Pane)
            && self.ws().panes.contains_key(&focused_id);

        // Detect focus change and clear the one-pane dismissal.
        // "Focus moves away and comes back" means any transition that
        // changes `focused_id`, including returning to the previously
        // dismissed pane — the user coming back is the signal they
        // want the overlay again.
        if self.last_focused_pane != Some(focused_id) {
            self.always_dismissed_pane = None;
            self.last_focused_pane = Some(focused_id);
        }

        if !pane_focused {
            return;
        }
        if self.overlay.is_some() {
            return;
        }
        if self.always_dismissed_pane == Some(focused_id) {
            return;
        }
        if self.rename_input.is_some() {
            return;
        }
        let (is_claude, is_scrolled) = {
            let pane = &self.ws().panes[&focused_id];
            (pane.is_claude_running(), pane.is_scrolled_back())
        };
        if !is_claude || is_scrolled {
            return;
        }
        self.overlay = Some(OverlayState::new(focused_id));
        self.mark_layout_change();
    }

    /// Emit a [`PaneStarted`] event for the given pane id. Pulls the
    /// current name/role from the active workspace so subscribers
    /// receive the metadata that was just attached.
    fn emit_pane_started(&self, pane_id: usize) {
        let ws = self.ws();
        let name = ws
            .pane_names
            .iter()
            .find(|(_, id)| **id == pane_id)
            .map(|(n, _)| n.clone());
        let role = ws.panes.get(&pane_id).and_then(|p| p.role.clone());
        self.event_bus.emit(crate::ipc::Event::PaneStarted {
            id: pane_id,
            name,
            role,
            ts_ms: crate::ipc::events::now_ms(),
        });
    }

    /// Emit a [`PaneExited`] event. Expects the caller to have already
    /// set `Pane.exit_event_emitted = true` (or to be about to remove
    /// the pane) so the event is exactly-once.
    fn emit_pane_exited(&self, pane_id: usize, name: Option<String>, role: Option<String>) {
        self.event_bus.emit(crate::ipc::Event::PaneExited {
            id: pane_id,
            name,
            role,
            ts_ms: crate::ipc::events::now_ms(),
        });
    }

    /// Copy text to clipboard, reusing the handle if available.
    fn copy_to_clipboard(&mut self, text: &str) {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(ref mut cb) = self.clipboard {
            let _ = cb.set_text(text);
        }
    }

    /// Drop the current selection if it targets the preview. Called
    /// whenever preview state shifts (scroll, new file) so the
    /// highlighted range can't point at different text than what
    /// Ctrl+C or mouse-up actually copies.
    fn clear_selection_if_preview(&mut self) {
        if matches!(
            self.selection.as_ref().map(|s| &s.target),
            Some(SelectionTarget::Preview)
        ) {
            self.selection = None;
        }
    }

    /// Recompute pane rectangles and apply sizes to every PTY in the
    /// active workspace. Returns `true` if any pane was actually
    /// resized (so callers can decide whether to enter the post-resize
    /// cooldown). Safe to call without a Frame — uses the cached
    /// `last_term_size`.
    pub fn relayout_panes(&mut self) -> bool {
        let (cols, rows) = self.last_term_size;
        if cols < 20 || rows < 5 {
            return false;
        }

        // Mirror the area math in ui::render / render_main_area,
        // including the fallback where tree / preview are hidden when
        // the terminal is too narrow. Keeping these in sync prevents
        // PTY size drift from the actually-painted pane size.
        const MIN_PANE_AREA_WIDTH: u16 = 20;
        let tab_h = 1u16;
        let status_h: u16 = if self.status_bar_visible || self.rename_input.is_some() {
            1
        } else {
            0
        };
        let overlay_h: u16 = if self.overlay.is_some() { 1 } else { 0 };
        let main_h = rows.saturating_sub(tab_h + status_h + overlay_h);

        let mut has_tree = self.ws().file_tree_visible;
        let mut has_preview = self.ws().preview.is_active();
        let tree_w_nom = self.file_tree_width;
        let preview_w_nom = self.preview_width;

        let needed = MIN_PANE_AREA_WIDTH
            + if has_tree { tree_w_nom } else { 0 }
            + if has_preview { preview_w_nom } else { 0 };
        if cols < needed && has_preview {
            has_preview = false;
        }
        let needed = MIN_PANE_AREA_WIDTH + if has_tree { tree_w_nom } else { 0 };
        if cols < needed && has_tree {
            has_tree = false;
        }

        let tree_w = if has_tree { tree_w_nom } else { 0 };
        let preview_w = if has_preview { preview_w_nom } else { 0 };
        let pane_w = cols.saturating_sub(tree_w).saturating_sub(preview_w);

        // Mirror ui::render_main_area's chunk ordering so the cached
        // rects reflect actual on-screen positions (not just sizes).
        // The IPC `list` response and mouse hit-testing both read x/y
        // from last_pane_rects, so getting the origin right matters
        // here even between renders. Chunk order there is:
        //   [tree?] [preview(if swapped)?] [panes] [preview(if !swapped)?]
        let pane_x = tree_w + if self.layout_swapped { preview_w } else { 0 };
        let pane_area = Rect::new(pane_x, tab_h, pane_w, main_h);
        let rects = self.ws().layout.calculate_rects(pane_area);

        let mut any_changed = false;
        for (pane_id, rect) in &rects {
            if let Some(pane) = self.ws_mut().panes.get_mut(pane_id) {
                let inner_rows = rect.height.saturating_sub(2);
                let inner_cols = rect.width.saturating_sub(2);
                if pane.resize(inner_rows, inner_cols).unwrap_or(false) {
                    any_changed = true;
                }
            }
        }

        self.ws_mut().last_pane_rects = rects;
        any_changed
    }

    /// Mark a layout change: apply resizes immediately and, if sizes
    /// actually changed, delay the next paint for a few frames so the
    /// PTY child can respond to SIGWINCH with a fresh redraw before
    /// we render. When no size changes happen (e.g. a sidebar toggle
    /// that fits in the same remaining width) we skip the cooldown so
    /// the UI stays responsive. Also drops any live selection, whose
    /// stored `content_rect` / `pane_id` could reference a layout that
    /// no longer exists.
    pub fn mark_layout_change(&mut self) {
        let changed = self.relayout_panes();
        if changed {
            // Take max so a freshly-triggered layout change on top of
            // an existing cooldown doesn't prematurely cut the wait.
            self.resize_cooldown = self.resize_cooldown.max(5);
        }
        // Any in-flight selection is bound to the old geometry.
        self.selection = None;
        self.dirty = true;
    }

    /// Called from main.rs on crossterm Resize events so we can update
    /// the cached terminal size and propagate the resize into panes.
    pub fn on_terminal_resize(&mut self, cols: u16, rows: u16) {
        self.last_term_size = (cols, rows);
        self.mark_layout_change();
    }

    /// Get the active workspace.
    pub fn ws(&self) -> &Workspace {
        &self.workspaces[self.active_tab]
    }

    /// Get the active workspace mutably.
    pub fn ws_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_tab]
    }

    // ─── Key handling ─────────────────────────────────────

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<bool> {
        // Emergency escape hatch: Ctrl+Q must always quit ccmux, even
        // while the IME composition overlay is holding input. Checked
        // before overlay routing so the user can never get trapped in
        // a wedged composition mode.
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return Ok(true);
        }

        // IME composition overlay — route every relevant key into the
        // buffer until the user commits or cancels. Takes precedence
        // over rename and every other handler so composition never
        // leaks into the layout / PTY unintentionally.
        if self.overlay.is_some() {
            return self.handle_overlay_key(key);
        }

        // Rename mode — swallow all input until Enter/Esc.
        if self.rename_input.is_some() {
            return Ok(self.handle_rename_key(key));
        }

        // Ctrl+; — open the IME composition overlay whenever a PTY
        // pane is focused. Initially gated to "is_claude_running()"
        // panes, but that proved flaky: Claude briefly retitles the
        // pane while running tools (e.g. "Edit", "Bash"), so the
        // detection would flicker and the hotkey would "mysteriously
        // stop working" mid-session. Open the overlay unconditionally
        // when focused on a pane and let the user choose when to
        // invoke it — users who don't need IME just don't press
        // Ctrl+; in that pane.
        if key.modifiers == KeyModifiers::CONTROL && matches!(key.code, KeyCode::Char(';')) {
            match self.ime_mode {
                crate::config::ImeMode::Off => {
                    // User opted out of the overlay. Don't leak a bare
                    // ';' to the PTY either: terminals encode Ctrl+;
                    // inconsistently, and falling through to
                    // `key_event_to_bytes` strips the Ctrl modifier and
                    // injects a stray semicolon into the shell. Silent
                    // swallow matches the "off" intent — the hotkey
                    // simply does nothing.
                    return Ok(true);
                }
                crate::config::ImeMode::Hotkey | crate::config::ImeMode::Always => {
                    // `always` still honors the explicit hotkey — users
                    // should be able to open the overlay deliberately
                    // before typing (e.g. when they know the first
                    // character belongs in IME).
                }
            }
            let focused_id = self.ws().focused_pane_id;
            let pane_focused = matches!(self.ws().focus_target, FocusTarget::Pane)
                && self.ws().panes.contains_key(&focused_id);
            if pane_focused {
                self.overlay = Some(OverlayState::new(focused_id));
                // Layout includes an overlay row now — repaint so the
                // panes shrink and the row appears.
                self.mark_layout_change();
                return Ok(true);
            }
            // Fall through when focus is on the file tree / preview;
            // Ctrl+; in those contexts has no meaning and shouldn't
            // open an overlay attached to a hidden target.
        }

        // Always-on IME: a printable key in a focused Claude pane
        // auto-opens the overlay and absorbs the first character, so
        // the user can type JP text without a hotkey dance. Gated to
        // Claude panes (bash / vim shouldn't hijack Enter+text), and
        // disabled when the pane is scrolled back so scrollback key
        // shortcuts still work.
        //
        // Note on `is_claude_running()` flakiness: PR #36 removed the
        // same gate from Ctrl+; because Claude briefly retitles the
        // pane while running tools, causing the hotkey to
        // "mysteriously stop working" mid-session. The failure mode
        // for always-on is the mirror image: a momentary false-
        // negative just means one keystroke goes to the PTY instead
        // of the overlay, which is recoverable (user presses Ctrl+;
        // or retypes). A false-positive would be worse — a non-
        // Claude pane hijacking text input — but the detector keys
        // on Claude-specific title substrings, so false positives
        // are rare in practice. Accepting the tradeoff intentionally
        // here; revisit if users report shell panes being hijacked.
        if self.ime_mode == crate::config::ImeMode::Always {
            if let Some(ch) = always_on_trigger_char(&key) {
                let focused_id = self.ws().focused_pane_id;
                let pane_focused = matches!(self.ws().focus_target, FocusTarget::Pane)
                    && self.ws().panes.contains_key(&focused_id);
                // Dismissal suppresses the key-trigger too. Otherwise
                // "dismiss overlay to send a raw Esc, then type /" would
                // reopen the overlay on the first shell char and trap
                // the input again.
                let dismissed_here = self.always_dismissed_pane == Some(focused_id);
                if pane_focused && !dismissed_here {
                    let (is_claude, is_scrolled) = {
                        let pane = &self.ws().panes[&focused_id];
                        (pane.is_claude_running(), pane.is_scrolled_back())
                    };
                    if is_claude && !is_scrolled {
                        let mut overlay = OverlayState::new(focused_id);
                        overlay.insert_char(ch);
                        self.overlay = Some(overlay);
                        self.mark_layout_change();
                        return Ok(true);
                    }
                }
            }
        }

        // Ctrl+Q — quit
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return Ok(true);
        }

        // Alt+R — rename active tab (session only)
        if key.modifiers == KeyModifiers::ALT
            && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
        {
            self.rename_input = Some(String::new());
            if !self.status_bar_visible {
                self.mark_layout_change();
            }
            return Ok(true);
        }

        // Ctrl+C — if text is selected, copy to clipboard instead of sending SIGINT
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            if let Some(ref sel) = self.selection.clone() {
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
                    self.selection = None;
                    return Ok(true);
                }
            }
            // No selection — fall through to forward Ctrl+C to PTY
        }

        // Ctrl+T / Alt+T — new tab (Alt+T groups with Alt-based tab nav)
        if (key.modifiers == KeyModifiers::CONTROL || key.modifiers == KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char('t') | KeyCode::Char('T'))
        {
            let new_id = self.new_tab()?;
            self.emit_pane_started(new_id);
            return Ok(true);
        }

        // Alt+Right — next tab
        if key.modifiers == KeyModifiers::ALT && key.code == KeyCode::Right {
            if !self.workspaces.is_empty() {
                self.active_tab = (self.active_tab + 1) % self.workspaces.len();
                self.overlay = None;
            }
            return Ok(true);
        }

        // Alt+Left — previous tab
        if key.modifiers == KeyModifiers::ALT && key.code == KeyCode::Left {
            if !self.workspaces.is_empty() {
                self.active_tab = if self.active_tab == 0 {
                    self.workspaces.len() - 1
                } else {
                    self.active_tab - 1
                };
                self.overlay = None;
            }
            return Ok(true);
        }

        // Alt+S — toggle status bar
        if key.modifiers == KeyModifiers::ALT
            && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
        {
            self.status_bar_visible = !self.status_bar_visible;
            self.mark_layout_change();
            return Ok(true);
        }

        // Alt+1 .. Alt+9 — jump to tab N
        if key.modifiers == KeyModifiers::ALT {
            if let KeyCode::Char(c) = key.code {
                if let Some(digit) = c.to_digit(10) {
                    if digit >= 1 && (digit as usize) <= self.workspaces.len() {
                        self.active_tab = (digit as usize) - 1;
                        self.overlay = None;
                        return Ok(true);
                    }
                }
            }
        }

        // Ctrl+Right — next pane
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Right {
            self.focus_next_pane();
            return Ok(true);
        }

        // Ctrl+Left — previous pane
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Left {
            self.focus_prev_pane();
            return Ok(true);
        }

        // Preview mode
        if self.ws().focus_target == FocusTarget::Preview {
            return self.handle_preview_key(key);
        }

        // File tree mode
        if self.ws().focus_target == FocusTarget::FileTree {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('f') {
                self.toggle_file_tree();
                return Ok(true);
            }
            return self.handle_file_tree_key(key);
        }

        // Ctrl+F — toggle file tree
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('f') {
            self.toggle_file_tree();
            return Ok(true);
        }

        // Ctrl+P — swap preview and terminal positions
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('p') {
            self.layout_swapped = !self.layout_swapped;
            return Ok(true);
        }

        let multi_pane = self.ws().layout.pane_count() > 1;
        let multi_tab = self.workspaces.len() > 1;

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                if let Some(new_id) = self.split_focused_pane(SplitDirection::Vertical)? {
                    self.emit_pane_started(new_id);
                }
                Ok(true)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                if let Some(new_id) = self.split_focused_pane(SplitDirection::Horizontal)? {
                    self.emit_pane_started(new_id);
                }
                Ok(true)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                if self.ws().focus_target == FocusTarget::Preview {
                    // Close preview and return to pane
                    self.ws_mut().preview.close();
                    self.ws_mut().focus_target = FocusTarget::Pane;
                    Ok(true)
                } else if multi_pane {
                    self.close_focused_pane();
                    Ok(true)
                } else if multi_tab {
                    self.close_tab(self.active_tab);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
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
    fn handle_overlay_key(&mut self, key: KeyEvent) -> Result<bool> {
        let overlay = match self.overlay.as_mut() {
            Some(o) => o,
            None => return Ok(false),
        };

        // Cancel (Esc or Ctrl+C).
        if matches!(key.code, KeyCode::Esc)
            || (key.modifiers == KeyModifiers::CONTROL && matches!(key.code, KeyCode::Char('c')))
        {
            let is_always = self.ime_mode == crate::config::ImeMode::Always;
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
                self.mark_layout_change();
                return Ok(true);
            }

            self.overlay = None;
            self.mark_layout_change();

            if is_always {
                // Always mode would re-open the overlay on the next
                // tick of maybe_auto_open_always_overlay. Record the
                // explicit dismissal so the user gets a window to
                // interact with the pane directly (Claude Esc-to-
                // interrupt, shell-level Ctrl+C, …). The suppression
                // clears when focus moves away and comes back.
                self.always_dismissed_pane = Some(target_pane);
                // Forward the cancel key to the pane so the user's
                // intent (interrupt Claude, send Ctrl+C to shell)
                // reaches its real target. Only on an already-empty
                // buffer, because a non-empty buffer was clearing
                // composition, not interacting with the pane. Propagate
                // the write error (same policy as the Enter-commit
                // path) instead of silently swallowing it.
                let focused_before = self.ws().focused_pane_id;
                self.ws_mut().focused_pane_id = target_pane;
                let forward_result = self.forward_key_to_pty(key);
                self.ws_mut().focused_pane_id = focused_before;
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
            let target_alive = self
                .ws()
                .panes
                .get(&target_pane)
                .map(|p| !p.exited)
                .unwrap_or(false);
            if !target_alive {
                if let Some(o) = self.overlay.as_mut() {
                    o.buffer = buffer;
                    o.cursor = o.buffer.chars().count();
                }
                self.dirty = true;
                return Ok(true);
            }

            // Target alive: close the overlay and deliver. If the
            // paste write fails (PTY closed mid-send, very rare), the
            // error propagates so the top-level render loop can log
            // or surface it instead of the text being dropped
            // silently.
            self.overlay = None;
            let mut commit_result: Result<()> = Ok(());
            if !buffer.is_empty() {
                let focused_before = self.ws().focused_pane_id;
                // forward_paste_to_pty writes to the currently-focused
                // pane, so temporarily refocus the overlay's target so
                // the paste reaches the right pane even if focus moved.
                self.ws_mut().focused_pane_id = target_pane;
                commit_result = self.forward_paste_to_pty(&buffer);
                self.ws_mut().focused_pane_id = focused_before;
            }
            self.mark_layout_change();
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
        self.dirty = true;
        Ok(true)
    }

    fn handle_rename_key(&mut self, key: KeyEvent) -> bool {
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
                // Ignore chars combined with Ctrl/Alt so shortcuts like
                // Ctrl+C don't leak into the buffer as literal letters.
                // Shift is fine — that's just uppercase.
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return true;
                }
                // Cap at something sane so a stuck key can't grow the tab bar forever.
                if buf.chars().count() < 32 {
                    buf.push(c);
                }
            }
            _ => return true,
        }
        self.dirty = true;
        true
    }

    fn handle_file_tree_key(&mut self, key: KeyEvent) -> Result<bool> {
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
                    self.ws_mut().preview.load(&path);
                }
                Ok(true)
            }
            KeyCode::Char('.') => {
                self.ws_mut().file_tree.toggle_hidden();
                Ok(true)
            }
            KeyCode::Esc => {
                // Return to pane, keep preview open
                self.ws_mut().focus_target = FocusTarget::Pane;
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    fn handle_preview_key(&mut self, key: KeyEvent) -> Result<bool> {
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
            // Horizontal scroll — unmodified arrow keys and vim-style h/l.
            // Ctrl+Left/Right remain focus navigation (matched below).
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

    // ─── Tab management ───────────────────────────────────

    /// Create a new tab with a fresh single pane, and return the new
    /// pane id.
    ///
    /// **Does not emit `PaneStarted`.** Callers must fire the event
    /// after attaching any metadata (name, role, custom tab label) so
    /// subscribers see the final identity — mirrors the contract on
    /// `split_focused_pane`.
    fn new_tab(&mut self) -> Result<usize> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let name = dir_name(&cwd);
        let pane_id = self.next_pane_id;
        self.next_pane_id = self.next_pane_id.wrapping_add(1);

        let ws = Workspace::new(name, cwd, pane_id, 10, 40, self.event_tx.clone())?;
        self.workspaces.push(ws);
        self.active_tab = self.workspaces.len() - 1;
        self.overlay = None;
        Ok(pane_id)
    }

    fn close_tab(&mut self, index: usize) {
        if self.workspaces.len() <= 1 {
            return;
        }

        // Snapshot (pane_id, name, role) for each not-yet-emitted pane
        // in this tab. We mark them as emitted *before* the actual
        // workspace removal so the natural-exit detection in the event
        // loop can't race us and double-fire.
        let mut to_emit: Vec<(usize, Option<String>, Option<String>)> = Vec::new();
        {
            let ws = &mut self.workspaces[index];
            let pane_ids: Vec<usize> = ws.panes.keys().copied().collect();
            for pid in &pane_ids {
                let name = ws
                    .pane_names
                    .iter()
                    .find(|(_, id)| **id == *pid)
                    .map(|(n, _)| n.clone());
                if let Some(pane) = ws.panes.get_mut(pid) {
                    if !pane.exit_event_emitted {
                        pane.exit_event_emitted = true;
                        to_emit.push((*pid, name, pane.role.clone()));
                    }
                }
            }
            for pid in pane_ids {
                self.claude_monitor.remove(pid);
            }
        }

        // If the overlay targets a pane in the tab being closed, cancel it.
        let overlay_in_tab = self
            .overlay
            .as_ref()
            .is_some_and(|o| self.workspaces[index].panes.contains_key(&o.target_pane));
        if overlay_in_tab {
            self.overlay = None;
        }

        // `pane_names` is dropped alongside the workspace, so we don't
        // need to retain() like `remove_pane_from_layout` does.
        self.workspaces[index].shutdown();
        self.workspaces.remove(index);
        let prev_active = self.active_tab;
        if self.active_tab >= self.workspaces.len() {
            self.active_tab = self.workspaces.len() - 1;
        }
        if prev_active != self.active_tab {
            self.overlay = None;
        }
        // Relayout the (possibly new) active tab so its rects are
        // recomputed and a repaint is scheduled. Needed when close_tab
        // is invoked from a non-key-driven path (IPC close on the last
        // pane of a non-last tab) since otherwise the render loop
        // wouldn't observe any dirty flag.
        self.mark_layout_change();
        for (pid, name, role) in to_emit {
            self.emit_pane_exited(pid, name, role);
        }
    }

    // ─── Pane management ──────────────────────────────────

    fn toggle_file_tree(&mut self) {
        let ws = self.ws_mut();
        let was_visible = ws.file_tree_visible;
        let will_be_visible;
        if ws.file_tree_visible && ws.focus_target == FocusTarget::FileTree {
            // Closing the tree — keep the preview open so the user can
            // continue reading the file they just opened. Focus moves
            // to the preview if it's active, otherwise back to the pane.
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

        // Only relayout if the pane area actually changes (visibility flipped).
        if was_visible != will_be_visible {
            self.mark_layout_change();
        }
    }

    const MAX_PANES: usize = 16;

    /// Create a new pane by splitting the focused one. Returns the new
    /// pane id on success, or `None` when the split was refused because
    /// the workspace is at `MAX_PANES` or the focused pane is too
    /// small to halve.
    ///
    /// **Does not emit `PaneStarted`.** Callers are responsible for
    /// firing the event after they've attached metadata (name, role) so
    /// subscribers see the pane with its final identity — otherwise the
    /// event races the metadata and lands with `name: None, role: None`.
    fn split_focused_pane(&mut self, direction: SplitDirection) -> Result<Option<usize>> {
        if self.ws().layout.pane_count() >= Self::MAX_PANES {
            return Ok(None);
        }

        if let Some(&(_, rect)) = self
            .ws()
            .last_pane_rects
            .iter()
            .find(|(id, _)| *id == self.ws().focused_pane_id)
        {
            match direction {
                SplitDirection::Vertical => {
                    if rect.width / 2 < self.min_pane_width {
                        return Ok(None);
                    }
                }
                SplitDirection::Horizontal => {
                    if rect.height / 2 < self.min_pane_height {
                        return Ok(None);
                    }
                }
            }
        }

        let new_id = self.next_pane_id;
        self.next_pane_id = self.next_pane_id.wrapping_add(1);

        // Inherit CWD from the focused pane
        let parent_cwd = self
            .ws()
            .panes
            .get(&self.ws().focused_pane_id)
            .map(|p| p.cwd.clone());

        let pane = Pane::new_with_cwd(new_id, 10, 40, self.event_tx.clone(), parent_cwd)?;
        let ws = self.ws_mut();
        ws.panes.insert(new_id, pane);
        ws.layout.split_pane(ws.focused_pane_id, new_id, direction);
        // Focus moves to the freshly-created pane so the user can type
        // in it immediately after splitting.
        ws.focused_pane_id = new_id;

        self.mark_layout_change();
        Ok(Some(new_id))
    }

    /// Apply a multi-pane layout to the active workspace.
    ///
    /// The workspace is expected to contain a single initial pane created
    /// by `App::new`. The root of the layout tree is mapped onto that
    /// initial pane; splits are produced by recursively driving
    /// `split_focused_pane`. Each `Pane` node's `command` (if any) is
    /// queued via `Pane::queue_startup_command`, which the main event
    /// loop will flush once the shell prompt is observed.
    ///
    /// Note: Phase 2 ignores the per-split `ratio`; splits use the
    /// existing equal-split semantics. A follow-up may extend this.
    pub fn apply_layout(&mut self, config: &LayoutConfig) -> Result<()> {
        let initial_pane_id = self.ws().focused_pane_id;
        self.apply_layout_node(&config.root, initial_pane_id)?;
        Ok(())
    }

    fn apply_layout_node(&mut self, node: &LayoutNodeSpec, target_pane_id: usize) -> Result<()> {
        match node {
            LayoutNodeSpec::Pane { id, command, role } => {
                // Register the leaf's id as a human-friendly name so IPC
                // clients (and external tools like `ccmux-send`) can
                // target this pane later without tracking numeric ids.
                if !id.is_empty() {
                    self.ws_mut().pane_names.insert(id.clone(), target_pane_id);
                }
                if let Some(pane) = self.ws_mut().panes.get_mut(&target_pane_id) {
                    if let Some(r) = role {
                        pane.role = Some(r.clone());
                    }
                    if let Some(cmd) = command {
                        pane.queue_startup_command(cmd);
                    }
                }
                Ok(())
            }
            LayoutNodeSpec::Split {
                direction,
                ratio: _,
                first,
                second,
            } => {
                // split_focused_pane operates on the focused pane and
                // moves focus to the freshly-created child, so we focus
                // the target first, then capture the new id afterwards.
                self.ws_mut().focused_pane_id = target_pane_id;
                let split_dir = match direction {
                    DirectionSpec::Vertical => SplitDirection::Vertical,
                    DirectionSpec::Horizontal => SplitDirection::Horizontal,
                };
                let new_pane_id = self.split_focused_pane(split_dir)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "layout split refused (too small or MAX_PANES) while applying layout"
                    )
                })?;
                // Recurse into both subtrees first — the leaves attach
                // name/role to their pane ids — then emit PaneStarted
                // so subscribers see the new pane with its final
                // identity (the initial pane was already emitted by
                // App::new; only the freshly-split one needs it here).
                self.apply_layout_node(first, target_pane_id)?;
                self.apply_layout_node(second, new_pane_id)?;
                self.emit_pane_started(new_pane_id);
                Ok(())
            }
        }
    }

    fn close_focused_pane(&mut self) {
        let ws_index = self.active_tab;
        let focused = self.ws().focused_pane_id;
        if self.workspaces[ws_index].layout.pane_count() <= 1 {
            // Preserve the historical TUI behavior: Ctrl+W on the
            // only remaining pane of the active tab is a no-op. The
            // CLI path (`handle_close`) uses a different policy.
            return;
        }
        let _ = self.remove_pane_from_layout(ws_index, focused);
    }

    /// Remove a single pane from a workspace's layout, killing its
    /// process and emitting `PaneExited` (guarded by
    /// `exit_event_emitted`). Assumes the workspace has at least two
    /// panes — the caller is responsible for handling the
    /// last-pane-in-tab case (close the tab or refuse).
    fn remove_pane_from_layout(
        &mut self,
        ws_index: usize,
        pane_id: usize,
    ) -> std::result::Result<(), ipc::CodedError> {
        let ws = &mut self.workspaces[ws_index];
        if !ws.panes.contains_key(&pane_id) {
            return Err(ipc::CodedError::new(
                ipc::err_code::PANE_VANISHED,
                "pane vanished",
            ));
        }
        let pane_ids = ws.layout.collect_pane_ids();
        let current_idx = pane_ids.iter().position(|&id| id == pane_id);

        // Capture PaneExited metadata before the removal. If a prior
        // path (e.g. natural shell exit) already emitted, we skip.
        let exited_meta: Option<(Option<String>, Option<String>)> = {
            let name = ws
                .pane_names
                .iter()
                .find(|(_, id)| **id == pane_id)
                .map(|(n, _)| n.clone());
            match ws.panes.get_mut(&pane_id) {
                Some(pane) if !pane.exit_event_emitted => {
                    pane.exit_event_emitted = true;
                    Some((name, pane.role.clone()))
                }
                _ => None,
            }
        };

        ws.layout.remove_pane(pane_id);

        if let Some(mut pane) = ws.panes.remove(&pane_id) {
            pane.kill();
        }

        // Drop any pane_names entry pointing at the removed pane so a
        // subsequent split can reuse the same name.
        ws.pane_names.retain(|_, id| *id != pane_id);

        // If the IME overlay targets the pane being removed, cancel it
        // so we don't leave a modal pointing at a dead pane id.
        if self
            .overlay
            .as_ref()
            .is_some_and(|o| o.target_pane == pane_id)
        {
            self.overlay = None;
        }

        // Clean up claude monitor state for this pane.
        self.claude_monitor.remove(pane_id);

        let ws = &mut self.workspaces[ws_index];
        let remaining_ids = ws.layout.collect_pane_ids();
        // Only reassign focus if the closed pane belonged to the
        // currently-focused workspace AND was the focused pane there.
        // Otherwise the closed pane lived in a background tab and we
        // must not disturb the active tab's focus.
        if ws.focused_pane_id == pane_id {
            if let Some(idx) = current_idx {
                let new_idx = if idx >= remaining_ids.len() {
                    remaining_ids.len().saturating_sub(1)
                } else {
                    idx
                };
                if let Some(&next) = remaining_ids.get(new_idx) {
                    ws.focused_pane_id = next;
                }
            } else if let Some(&first) = remaining_ids.first() {
                ws.focused_pane_id = first;
            }
        }

        if ws_index == self.active_tab {
            // Active tab: full relayout so the remaining panes resize
            // into the freed space.
            self.mark_layout_change();
        } else {
            // Background tab: no geometry change to the visible layout,
            // but the tab strip and pane-count indicators still need a
            // repaint, and `ccmux list` must reflect the new state on
            // the very next tick.
            self.dirty = true;
        }
        if let Some((name, role)) = exited_meta {
            self.emit_pane_exited(pane_id, name, role);
        }
        Ok(())
    }

    /// CLI close handler: resolve `target` across every workspace,
    /// then either remove it from its tab or, if it's the last pane of
    /// a non-last tab, close the whole tab. Refuses with `LAST_PANE`
    /// when closing would empty the only remaining workspace.
    fn handle_close(&mut self, target: &PaneRef) -> std::result::Result<usize, ipc::CodedError> {
        let (ws_index, pane_id) = self.resolve_pane_across_workspaces(target).ok_or_else(|| {
            ipc::CodedError::new(
                ipc::err_code::PANE_NOT_FOUND,
                format!("pane not found: {target:?}"),
            )
        })?;

        let is_only_pane = self.workspaces[ws_index].layout.pane_count() <= 1;
        if is_only_pane {
            if self.workspaces.len() <= 1 {
                return Err(ipc::CodedError::new(
                    ipc::err_code::LAST_PANE,
                    "cannot close the last pane of the only tab",
                ));
            }
            // `close_tab` emits PaneExited for every pane in the tab
            // (guarded by exit_event_emitted), so the subscriber sees
            // a single PaneExited for `pane_id` as expected.
            self.close_tab(ws_index);
            return Ok(pane_id);
        }

        self.remove_pane_from_layout(ws_index, pane_id)?;
        Ok(pane_id)
    }

    /// Resolve a `PaneRef` against every workspace, not just the
    /// active tab. Returns `(workspace_index, pane_id)`. `Focused`
    /// still maps to the active workspace's focused pane for
    /// symmetry with the other IPC commands.
    fn resolve_pane_across_workspaces(&self, target: &PaneRef) -> Option<(usize, usize)> {
        match target {
            PaneRef::Focused => {
                let ws = self.ws();
                if ws.panes.contains_key(&ws.focused_pane_id) {
                    Some((self.active_tab, ws.focused_pane_id))
                } else {
                    None
                }
            }
            PaneRef::Id(id) => self
                .workspaces
                .iter()
                .enumerate()
                .find(|(_, ws)| ws.panes.contains_key(id))
                .map(|(i, _)| (i, *id)),
            PaneRef::Name(name) => {
                // Names are workspace-local. Scan active tab first so a
                // duplicate name across tabs prefers the visible one.
                let ordered: Vec<usize> = std::iter::once(self.active_tab)
                    .chain((0..self.workspaces.len()).filter(|i| *i != self.active_tab))
                    .collect();
                for i in ordered {
                    let ws = &self.workspaces[i];
                    if let Some(&id) = ws.pane_names.get(name) {
                        if ws.panes.contains_key(&id) {
                            return Some((i, id));
                        }
                    }
                }
                None
            }
        }
    }

    /// Cycle focus forward: FileTree → Preview → Pane1 → Pane2 → ... → FileTree
    fn focus_next_pane(&mut self) {
        let ws = self.ws_mut();
        let ids = ws.layout.collect_pane_ids();
        let tree_visible = ws.file_tree_visible;
        let preview_active = ws.preview.is_active();
        let _swapped = false; // preview position doesn't affect focus order

        match ws.focus_target {
            FocusTarget::FileTree => {
                // File tree → preview (if active) or first pane
                if preview_active {
                    ws.focus_target = FocusTarget::Preview;
                } else {
                    ws.focus_target = FocusTarget::Pane;
                }
            }
            FocusTarget::Preview => {
                // Preview → first pane
                ws.focus_target = FocusTarget::Pane;
            }
            FocusTarget::Pane => {
                if let Some(idx) = ids.iter().position(|&id| id == ws.focused_pane_id) {
                    if idx + 1 < ids.len() {
                        ws.focused_pane_id = ids[idx + 1];
                    } else if tree_visible {
                        ws.focus_target = FocusTarget::FileTree;
                    } else if preview_active {
                        ws.focus_target = FocusTarget::Preview;
                    } else {
                        ws.focused_pane_id = ids[0];
                    }
                }
            }
        }
    }

    /// Cycle focus backward
    fn focus_prev_pane(&mut self) {
        let ws = self.ws_mut();
        let ids = ws.layout.collect_pane_ids();
        let tree_visible = ws.file_tree_visible;
        let preview_active = ws.preview.is_active();

        match ws.focus_target {
            FocusTarget::FileTree => {
                // File tree → last pane
                ws.focus_target = FocusTarget::Pane;
                if let Some(&last) = ids.last() {
                    ws.focused_pane_id = last;
                }
            }
            FocusTarget::Preview => {
                // Preview → file tree (if visible) or last pane
                if tree_visible {
                    ws.focus_target = FocusTarget::FileTree;
                } else {
                    ws.focus_target = FocusTarget::Pane;
                    if let Some(&last) = ids.last() {
                        ws.focused_pane_id = last;
                    }
                }
            }
            FocusTarget::Pane => {
                if let Some(idx) = ids.iter().position(|&id| id == ws.focused_pane_id) {
                    if idx > 0 {
                        ws.focused_pane_id = ids[idx - 1];
                    } else if preview_active {
                        ws.focus_target = FocusTarget::Preview;
                    } else if tree_visible {
                        ws.focus_target = FocusTarget::FileTree;
                    } else {
                        ws.focused_pane_id = ids[ids.len() - 1];
                    }
                }
            }
        }
    }

    /// Scroll a pane based on scrollbar click position.
    fn scroll_pane_to_click(&self, pane_id: usize, click_row: u16, inner: &Rect) {
        if let Some(pane) = self.ws().panes.get(&pane_id) {
            let (_, total_lines) = pane.scrollbar_info();
            let visible_rows = inner.height as usize;
            if total_lines <= visible_rows {
                return;
            }
            let max_scroll = total_lines.saturating_sub(visible_rows);
            // click_row relative to inner area: top = max scroll, bottom = 0
            let relative_y = click_row.saturating_sub(inner.y) as f32;
            let ratio = relative_y / inner.height.max(1) as f32;
            let target_scroll = ((1.0 - ratio) * max_scroll as f32) as usize;
            let mut parser = pane.parser.lock().unwrap_or_else(|e| e.into_inner());
            parser.screen_mut().set_scrollback(target_scroll);
        }
    }

    // ─── Mouse handling ───────────────────────────────────

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
            // When swapped: [tree][preview][panes] → drag the RIGHT edge of preview
            // When normal:  [tree][panes][preview] → drag the LEFT edge of preview
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
        // Cancel any in-progress rename on mouse click so
        // the buffer can't silently migrate to another tab.
        if matches!(mouse.kind, MouseEventKind::Down(_)) && self.rename_input.is_some() {
            let needs_relayout = !self.status_bar_visible;
            self.rename_input = None;
            self.dirty = true;
            if needs_relayout {
                self.mark_layout_change();
            }
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let col = mouse.column;
                let row = mouse.row;

                // Clear previous selection on any click
                self.selection = None;

                // Check tab bar clicks
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
                            self.overlay = None;
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
                // Click missed the tab bar — reset double-click tracker.
                self.last_tab_click = None;

                // Check [+] new tab button
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

                // Check border drag (file tree / preview)
                if self.is_on_file_tree_border(col) {
                    self.dragging = Some(DragTarget::FileTreeBorder);
                    return;
                }
                if self.is_on_preview_border(col) {
                    self.dragging = Some(DragTarget::PreviewBorder);
                    return;
                }

                // Check pane split border drag
                if let Some(pane_area) = self.ws().last_pane_rects.first().map(|_| {
                    // Compute the total pane area from all pane rects
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

                // Check file tree click
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
                                self.ws_mut().preview.load(&path);
                            }
                        }
                        return;
                    }
                }

                // Check preview click
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

                // Check pane clicks
                let pane_rects = self.ws().last_pane_rects.clone();
                for (pane_id, rect) in pane_rects {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        self.ws_mut().focused_pane_id = pane_id;
                        self.ws_mut().focus_target = FocusTarget::Pane;

                        // Check if clicking on scrollbar (rightmost column inside border)
                        let scrollbar_col = rect.x + rect.width - 2; // -1 border, -1 scrollbar
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
            MouseEventKind::Drag(MouseButton::Left) => {
                let col = mouse.column;
                let row = mouse.row;

                // Border drag takes priority
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
                    }
                    return;
                }

                // Text selection: extend if active, or start new
                if let Some(ref mut sel) = self.selection {
                    let inner = sel.content_rect;
                    match sel.target {
                        SelectionTarget::Pane(_) => {
                            // Pane: screen-relative coords inside inner.
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
                            // Preview: translate screen coords to
                            // source (absolute line + char offset)
                            // using the current scroll state.
                            let scroll_v = self.ws().preview.scroll_offset;
                            let h_scroll = self.ws().preview.h_scroll_offset;

                            let mut screen_col = col.saturating_sub(inner.x);
                            let mut screen_row = row.saturating_sub(inner.y);

                            // Auto-scroll when drag reaches an edge.
                            // Move the underlying scroll by one step
                            // so the cursor can "pull" more content
                            // into view. Clamp screen position so the
                            // computed source coord tracks the new edge.
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

                            // Re-read scroll state in case we changed it above.
                            let scroll_v = self.ws().preview.scroll_offset.max(scroll_v);
                            let h_scroll = self.ws().preview.h_scroll_offset.max(h_scroll);
                            // Clamp end_row to a valid absolute line index.
                            let lines_len = self.ws().preview.lines.len();
                            let abs_row =
                                (scroll_v + screen_row as usize).min(lines_len.saturating_sub(1));
                            let abs_col = screen_col as usize + h_scroll;
                            // Update the selection endpoint (source coords).
                            if let Some(sel) = self.selection.as_mut() {
                                sel.end_row = abs_row as u32;
                                sel.end_col = abs_col as u32;
                            }
                        }
                    }
                } else {
                    // Start new selection — try pane areas first, then preview
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
                    // Preview drag selection. Content area is the inside
                    // of the preview border minus the 5-column line-number
                    // gutter (format "{:>4}│"). Selection stores source
                    // coords (abs line index, char offset) so it can
                    // survive scrolling.
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
                                // Ignore drags that start inside the gutter
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

                // Copy selected text to clipboard
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
                    // Keep selection visible until next click
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

    /// Dispatch a mouse-wheel event. Routes the event to whichever
    /// widget the cursor sits over: file tree / preview use their own
    /// scroll API; panes either scroll their vt100 scrollback (normal
    /// shell) or forward the wheel to the PTY as an xterm mouse report
    /// / arrow-key fallback when running an alternate-screen TUI
    /// (Claude Code `/tui fullscreen`, vim, less, …). See Issue #52
    /// and `Pane::wheel_forward_bytes` for the decision table.
    ///
    /// `CCMUX_DISABLE_MOUSE_FORWARD=1` forces the legacy behavior
    /// everywhere (vt100 scrollback only), as an escape hatch for
    /// nested ccmux or terminals with mismatched mouse-protocol
    /// encoding.
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

        let disable_forward = std::env::var("CCMUX_DISABLE_MOUSE_FORWARD")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);

        let pane_rects = self.ws().last_pane_rects.clone();
        for (pane_id, rect) in pane_rects {
            if !(col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height)
            {
                continue;
            }
            // Pane content area starts one cell inside the border.
            let local_col = col.saturating_sub(rect.x).saturating_sub(1);
            let local_row = row.saturating_sub(rect.y).saturating_sub(1);

            let bytes = if disable_forward {
                None
            } else {
                self.ws()
                    .panes
                    .get(&pane_id)
                    .and_then(|p| p.wheel_forward_bytes(scroll_down, local_col, local_row))
            };

            if let Some(data) = bytes {
                if let Some(pane) = self.ws_mut().panes.get_mut(&pane_id) {
                    // Best-effort forward — a PTY write failure here
                    // is non-fatal (Claude Code still renders without
                    // the wheel event reaching it).
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

    // ─── PTY forwarding ───────────────────────────────────

    /// Forward pasted text to PTY, wrapping in bracketed paste only if
    /// the PTY application has enabled the mode (e.g. Claude Code, modern
    /// readline). Sending bracketed paste to a shell that hasn't opted in
    /// causes the escape sequences to appear as literal text (issue #2).
    pub fn forward_paste_to_pty(&mut self, text: &str) -> Result<()> {
        let focused_id = self.ws().focused_pane_id;
        if let Some(pane) = self.ws_mut().panes.get_mut(&focused_id) {
            pane.scroll_reset();
            if pane.is_bracketed_paste_enabled() {
                let mut data = Vec::with_capacity(text.len() + 12);
                data.extend_from_slice(b"\x1b[200~");
                data.extend_from_slice(text.as_bytes());
                data.extend_from_slice(b"\x1b[201~");
                pane.write_input(&data)?;
            } else {
                pane.write_input(text.as_bytes())?;
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn forward_key_to_pty(&mut self, key: KeyEvent) -> Result<()> {
        let focused_id = self.ws().focused_pane_id;
        if let Some(pane) = self.ws_mut().panes.get_mut(&focused_id) {
            pane.scroll_reset();
            if let Some(bytes) = key_event_to_bytes(&key) {
                pane.write_input(&bytes)?;
            }
        }
        Ok(())
    }

    pub fn drain_pty_events(&mut self) -> bool {
        let mut had_events = false;
        while let Ok(event) = self.event_rx.try_recv() {
            had_events = true;
            match event {
                AppEvent::PtyEof(pane_id) => {
                    let mut exit_meta: Option<(Option<String>, Option<String>)> = None;
                    for ws in &mut self.workspaces {
                        if let Some(pane) = ws.panes.get_mut(&pane_id) {
                            pane.exited = true;
                            if !pane.exit_event_emitted {
                                pane.exit_event_emitted = true;
                                let name = ws
                                    .pane_names
                                    .iter()
                                    .find(|(_, id)| **id == pane_id)
                                    .map(|(n, _)| n.clone());
                                exit_meta = Some((name, pane.role.clone()));
                            }
                            break;
                        }
                    }
                    if let Some((name, role)) = exit_meta {
                        self.emit_pane_exited(pane_id, name, role);
                    }
                }
                AppEvent::CwdChanged(pane_id, new_cwd) => {
                    // Security: resolve symlinks and relative components.
                    // Reject paths that don't resolve to a real directory
                    // (prevents OSC 7 escape sequence path injection).
                    let new_cwd = match new_cwd.canonicalize() {
                        Ok(p) if p.is_dir() => p,
                        _ => continue,
                    };
                    for ws in &mut self.workspaces {
                        if ws.panes.contains_key(&pane_id) {
                            // Update pane's cwd
                            if let Some(pane) = ws.panes.get_mut(&pane_id) {
                                pane.cwd = new_cwd.clone();
                            }
                            if ws.focused_pane_id == pane_id {
                                let prev_show_hidden = ws.file_tree.show_hidden;
                                ws.file_tree = FileTree::new(new_cwd.clone());
                                // FileTree::new defaults to show_hidden=true
                                // Only toggle if the previous state was different
                                if ws.file_tree.show_hidden != prev_show_hidden {
                                    ws.file_tree.toggle_hidden();
                                }
                                ws.cwd = new_cwd;
                                ws.name = dir_name(&ws.cwd);
                                ws.preview.close();
                            }
                            break;
                        }
                    }
                }
                AppEvent::PtyOutput(_) => {}
            }
        }
        if had_events {
            self.dirty = true;
        }
        had_events
    }

    pub fn shutdown(&mut self) {
        // Surface PaneExited for every still-live pane before we tear
        // down the workspaces, so an event-stream subscriber observes
        // the final state consistently with the exactly-once contract.
        // `exit_event_emitted` guards against re-emitting any pane that
        // already exited via Ctrl+W, close_tab, or a natural shell exit.
        let mut pending: Vec<(usize, Option<String>, Option<String>)> = Vec::new();
        for ws in &mut self.workspaces {
            let pane_ids: Vec<usize> = ws.panes.keys().copied().collect();
            for pid in pane_ids {
                let name = ws
                    .pane_names
                    .iter()
                    .find(|(_, id)| **id == pid)
                    .map(|(n, _)| n.clone());
                if let Some(pane) = ws.panes.get_mut(&pid) {
                    if !pane.exit_event_emitted {
                        pane.exit_event_emitted = true;
                        pending.push((pid, name, pane.role.clone()));
                    }
                }
            }
        }
        for (pid, name, role) in pending {
            self.emit_pane_exited(pid, name, role);
        }
        for ws in &mut self.workspaces {
            ws.shutdown();
        }
    }

    /// Drain any pending IPC commands and dispatch them. Safe to call
    /// every frame — it's a no-op when the channel is empty. Commands
    /// always target the active workspace.
    pub fn drain_app_commands(&mut self) {
        // Pop into a local Vec first so we can borrow `self` mutably
        // inside `handle_app_command` without fighting the receiver
        // borrow.
        let mut cmds: Vec<AppCommand> = Vec::new();
        while let Ok(cmd) = self.command_rx.try_recv() {
            cmds.push(cmd);
        }
        for cmd in cmds {
            self.handle_app_command(cmd);
        }
    }

    fn handle_app_command(&mut self, cmd: AppCommand) {
        match cmd {
            AppCommand::List { reply } => {
                let ws = self.ws();
                let focused = ws.focused_pane_id;
                // Invert pane_names so we can look up by id.
                let mut name_by_id: HashMap<usize, String> = HashMap::new();
                for (name, id) in &ws.pane_names {
                    name_by_id.insert(*id, name.clone());
                }
                let rect_by_id: HashMap<usize, Rect> = ws.last_pane_rects.iter().copied().collect();
                let mut infos: Vec<PaneInfo> = Vec::new();
                for id in ws.layout.collect_pane_ids() {
                    let role = ws.panes.get(&id).and_then(|p| p.role.clone());
                    let rect = rect_by_id.get(&id).copied().unwrap_or_default();
                    infos.push(PaneInfo {
                        id,
                        name: name_by_id.get(&id).cloned(),
                        role,
                        focused: id == focused,
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    });
                }
                let _ = reply.send(infos);
            }
            AppCommand::Send {
                target,
                data,
                append_enter,
                reply,
            } => {
                let result = self.handle_send(&target, &data, append_enter);
                let _ = reply.send(result);
            }
            AppCommand::Focus { target, reply } => {
                let result = self.handle_focus(&target);
                let _ = reply.send(result);
            }
            AppCommand::Split {
                target,
                direction,
                command,
                name,
                role,
                reply,
            } => {
                let result = self.handle_split(&target, direction, command, name, role);
                let _ = reply.send(result);
            }
            AppCommand::NewTab {
                command,
                name,
                label,
                role,
                reply,
            } => {
                let result = self.handle_new_tab(command, name, label, role);
                let _ = reply.send(result);
            }
            AppCommand::Inspect {
                target,
                lines,
                include_cursor,
                reply,
            } => {
                let result = self.handle_inspect(&target, lines, include_cursor);
                let _ = reply.send(result);
            }
            AppCommand::Close { target, reply } => {
                let result = self.handle_close(&target);
                let _ = reply.send(result);
            }
        }
    }

    fn handle_inspect(
        &self,
        target: &PaneRef,
        lines: Option<usize>,
        include_cursor: bool,
    ) -> std::result::Result<serde_json::Value, ipc::CodedError> {
        let ws = self.ws();
        let pane_id = ws.resolve_pane_ref(target).ok_or_else(|| {
            ipc::CodedError::new(
                ipc::err_code::PANE_NOT_FOUND,
                format!("pane not found: {target:?}"),
            )
        })?;
        let pane = ws
            .panes
            .get(&pane_id)
            .ok_or_else(|| ipc::CodedError::new(ipc::err_code::PANE_VANISHED, "pane vanished"))?;

        let pane_name = ws
            .pane_names
            .iter()
            .find(|(_, id)| **id == pane_id)
            .map(|(n, _)| n.clone());

        // Snapshot the vt100 state under the lock, release it before
        // JSON serialization.
        let (rows, cols, line_start, line_count, collected, cursor) = {
            let parser = pane.parser.lock().map_err(|_| {
                ipc::CodedError::new(ipc::err_code::INTERNAL, "vt100 parser lock poisoned")
            })?;
            let screen = parser.screen();
            let size = screen.size();
            let total_rows = size.0 as usize;
            let total_cols = size.1 as usize;

            let want: usize = lines.map(|n| n.min(total_rows)).unwrap_or(total_rows);
            let start: usize = total_rows.saturating_sub(want);
            let end: usize = total_rows;

            let mut collected: Vec<(usize, String)> = Vec::with_capacity(end - start);
            for row in start..end {
                let mut s = String::with_capacity(total_cols);
                for col in 0..total_cols {
                    if let Some(cell) = screen.cell(row as u16, col as u16) {
                        s.push_str(cell.contents());
                    }
                }
                // Trim trailing spaces but preserve the row's existence
                // so callers can rely on positional indexing.
                let trimmed = s.trim_end().to_string();
                collected.push((row, trimmed));
            }

            let cursor = if include_cursor {
                let (crow, ccol) = screen.cursor_position();
                Some((!screen.hide_cursor(), crow as usize, ccol as usize))
            } else {
                None
            };

            (
                total_rows,
                total_cols,
                start,
                end - start,
                collected,
                cursor,
            )
        };

        let text = collected
            .iter()
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let lines_json: Vec<serde_json::Value> = collected
            .into_iter()
            .map(|(row, text)| serde_json::json!({ "row": row, "text": text }))
            .collect();

        // Build `pane` so `name` is omitted when the pane has no
        // registered IPC name, matching the existing `PaneInfo`
        // convention of omitting absent optionals.
        let mut pane_obj = serde_json::json!({ "id": pane_id });
        if let Some(n) = pane_name {
            pane_obj["name"] = serde_json::Value::String(n);
        }
        let mut payload = serde_json::json!({
            "pane": pane_obj,
            "screen": {
                "rows": rows,
                "cols": cols,
                "line_start": line_start,
                "line_count": line_count,
            },
            "lines": lines_json,
            "text": text,
        });

        if let Some((visible, crow, ccol)) = cursor {
            payload["cursor"] = serde_json::json!({
                "visible": visible,
                "row": crow,
                "col": ccol,
            });
        }

        Ok(payload)
    }

    fn handle_new_tab(
        &mut self,
        command: Option<String>,
        name: Option<String>,
        label: Option<String>,
        role: Option<String>,
    ) -> std::result::Result<usize, ipc::CodedError> {
        // Delegate to the existing keybinding-driven new_tab so we
        // don't drift from the interactive behavior (pane id bookkeeping,
        // active_tab update, cwd inheritance). The freshly-created tab
        // becomes the active one, so `ws_mut()` points at it. new_tab
        // deliberately does not emit PaneStarted — we emit below after
        // attaching name / role / label so subscribers see the final
        // identity.
        let new_pane_id = self
            .new_tab()
            .map_err(|e| ipc::CodedError::new(ipc::err_code::IO_ERROR, e.to_string()))?;
        if let Some(pane) = self.ws_mut().panes.get_mut(&new_pane_id) {
            if let Some(cmd) = command {
                pane.queue_startup_command(&cmd);
            }
            if let Some(r) = role {
                pane.role = Some(r);
            }
        }
        if let Some(name) = name {
            if !name.is_empty() {
                self.ws_mut().pane_names.insert(name, new_pane_id);
            }
        }
        if let Some(label) = label {
            if !label.is_empty() {
                self.ws_mut().custom_name = Some(label);
            }
        }
        self.dirty = true;
        self.emit_pane_started(new_pane_id);
        Ok(new_pane_id)
    }

    fn handle_send(
        &mut self,
        target: &PaneRef,
        data: &[u8],
        append_enter: bool,
    ) -> std::result::Result<(), ipc::CodedError> {
        let pane_id = self.ws().resolve_pane_ref(target).ok_or_else(|| {
            ipc::CodedError::new(
                ipc::err_code::PANE_NOT_FOUND,
                format!("pane not found: {target:?}"),
            )
        })?;
        let pane =
            self.ws_mut().panes.get_mut(&pane_id).ok_or_else(|| {
                ipc::CodedError::new(ipc::err_code::PANE_VANISHED, "pane vanished")
            })?;
        pane.write_input(data)
            .map_err(|e| ipc::CodedError::new(ipc::err_code::IO_ERROR, e.to_string()))?;
        if append_enter {
            pane.write_input(b"\r")
                .map_err(|e| ipc::CodedError::new(ipc::err_code::IO_ERROR, e.to_string()))?;
        }
        self.dirty = true;
        Ok(())
    }

    fn handle_focus(&mut self, target: &PaneRef) -> std::result::Result<(), ipc::CodedError> {
        let pane_id = self.ws().resolve_pane_ref(target).ok_or_else(|| {
            ipc::CodedError::new(
                ipc::err_code::PANE_NOT_FOUND,
                format!("pane not found: {target:?}"),
            )
        })?;
        let ws = self.ws_mut();
        ws.focused_pane_id = pane_id;
        ws.focus_target = FocusTarget::Pane;
        self.dirty = true;
        Ok(())
    }

    fn handle_split(
        &mut self,
        target: &PaneRef,
        direction: ipc::Direction,
        command: Option<String>,
        name: Option<String>,
        role: Option<String>,
    ) -> std::result::Result<usize, ipc::CodedError> {
        let target_pane_id = self.ws().resolve_pane_ref(target).ok_or_else(|| {
            ipc::CodedError::new(
                ipc::err_code::PANE_NOT_FOUND,
                format!("pane not found: {target:?}"),
            )
        })?;
        let prev_focus = self.ws().focused_pane_id;
        self.ws_mut().focused_pane_id = target_pane_id;
        let split_dir = match direction {
            ipc::Direction::Vertical => SplitDirection::Vertical,
            ipc::Direction::Horizontal => SplitDirection::Horizontal,
        };
        let new_pane_id = match self
            .split_focused_pane(split_dir)
            .map_err(|e| ipc::CodedError::new(ipc::err_code::IO_ERROR, e.to_string()))?
        {
            Some(id) => id,
            None => {
                // Refused (too small or MAX_PANES). Restore focus so
                // the refusal is invisible to the caller state.
                self.ws_mut().focused_pane_id = prev_focus;
                return Err(ipc::CodedError::new(
                    ipc::err_code::SPLIT_REFUSED,
                    "split refused (max panes reached or pane too small)",
                ));
            }
        };
        // Attach name/role/command BEFORE emitting PaneStarted so
        // subscribers see the final identity. Otherwise the event races
        // the metadata and lands with `name: None, role: None`, breaking
        // consumers that filter on `.name == "<stable>"`.
        if let Some(pane) = self.ws_mut().panes.get_mut(&new_pane_id) {
            if let Some(cmd) = command {
                pane.queue_startup_command(&cmd);
            }
            if let Some(r) = role {
                pane.role = Some(r);
            }
        }
        if let Some(name) = name {
            if !name.is_empty() {
                self.ws_mut().pane_names.insert(name, new_pane_id);
            }
        }
        self.emit_pane_started(new_pane_id);
        Ok(new_pane_id)
    }
}

/// Extract directory name from a path for tab title.
fn dir_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// Extract text from a pane's vt100 screen within a selection range.
fn extract_selected_text(pane: &Pane, sr: u32, sc: u32, er: u32, ec: u32) -> String {
    let parser = pane.parser.lock().unwrap_or_else(|e| e.into_inner());
    let screen = parser.screen();
    let mut lines = Vec::new();

    for row in sr..=er {
        let mut line = String::new();
        let col_start = if row == sr { sc } else { 0 };
        let col_end = if row == er { ec } else { 999 };

        for col in col_start..=col_end {
            if let Some(cell) = screen.cell(row as u16, col as u16) {
                let contents = cell.contents();
                if contents.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(contents);
                }
            }
        }
        lines.push(line.trim_end().to_string());
    }

    // Remove trailing empty lines
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    lines.join("\n")
}

/// Extract text from the file preview within a selection range.
/// `sr`/`er` are absolute line indices; `sc`/`ec` are char offsets
/// within the line (selection is stored in source coordinates so it
/// survives scrolling). Trailing empty lines are stripped.
fn extract_preview_selected_text(
    preview: &crate::preview::Preview,
    sr: u32,
    sc: u32,
    er: u32,
    ec: u32,
) -> String {
    let lines = &preview.lines;
    let mut out: Vec<String> = Vec::new();

    for abs_row in sr..=er {
        let idx = abs_row as usize;
        if idx >= lines.len() {
            break;
        }
        let line = &lines[idx];
        let chars: Vec<char> = line.chars().collect();

        let col_start = if abs_row == sr { sc as usize } else { 0 };
        let col_end_inclusive = if abs_row == er {
            ec as usize
        } else {
            chars.len().saturating_sub(1)
        };

        let start = col_start.min(chars.len());
        let end = (col_end_inclusive.saturating_add(1)).min(chars.len());
        let slice: String = if start < end {
            chars[start..end].iter().collect()
        } else {
            String::new()
        };
        out.push(slice);
    }

    // Strip trailing empty lines only.
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }

    out.join("\n")
}

/// Public wrapper for key_event_to_bytes (used by main.rs paste detection).
pub fn key_event_to_bytes_pub(key: &KeyEvent) -> Option<Vec<u8>> {
    key_event_to_bytes(key)
}

/// For IME `always` mode: return `Some(ch)` if pressing this key in
/// a focused Claude pane should auto-open the overlay and seed it
/// with `ch`. Anything else (Ctrl-modifier, Alt-modifier, navigation
/// keys, Enter, Backspace, Esc, function keys, …) is NOT a trigger —
/// those keep their existing pass-through meaning for the pane.
///
/// We deliberately let plain SHIFT through because capital letters
/// and shifted symbols are still printable text the user would want
/// composed in the overlay.
fn always_on_trigger_char(key: &KeyEvent) -> Option<char> {
    // Ctrl / Alt / Super / Meta → not a plain printable press.
    let ignore_mods = KeyModifiers::CONTROL
        | KeyModifiers::ALT
        | KeyModifiers::SUPER
        | KeyModifiers::HYPER
        | KeyModifiers::META;
    if key.modifiers.intersects(ignore_mods) {
        return None;
    }
    match key.code {
        KeyCode::Char(c) => Some(c),
        _ => None,
    }
}

/// Convert a crossterm KeyEvent into bytes suitable for PTY input.
fn key_event_to_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                let ctrl_byte = (c.to_ascii_lowercase() as u8)
                    .wrapping_sub(b'a')
                    .wrapping_add(1);
                if ctrl_byte <= 26 {
                    if alt {
                        // Alt+Ctrl+Char → ESC + ctrl byte
                        Some(vec![0x1b, ctrl_byte])
                    } else {
                        Some(vec![ctrl_byte])
                    }
                } else {
                    Some(c.to_string().into_bytes())
                }
            } else if alt {
                // Alt+Char → ESC + char (standard xterm behavior)
                let mut bytes = vec![0x1b];
                bytes.extend_from_slice(c.to_string().as_bytes());
                Some(bytes)
            } else {
                Some(c.to_string().into_bytes())
            }
        }
        // Alt+Enter → send newline (\n) for multi-line input in Claude Code
        KeyCode::Enter if alt => Some(vec![b'\n']),
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
        KeyCode::Insert => Some(b"\x1b[2~".to_vec()),
        KeyCode::F(n) => {
            let seq = match n {
                1 => "\x1bOP",
                2 => "\x1bOQ",
                3 => "\x1bOR",
                4 => "\x1bOS",
                5 => "\x1b[15~",
                6 => "\x1b[17~",
                7 => "\x1b[18~",
                8 => "\x1b[19~",
                9 => "\x1b[20~",
                10 => "\x1b[21~",
                11 => "\x1b[23~",
                12 => "\x1b[24~",
                _ => return None,
            };
            Some(seq.as_bytes().to_vec())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn always_on_trigger_plain_char_fires() {
        let k = mk_key(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(always_on_trigger_char(&k), Some('a'));
    }

    #[test]
    fn always_on_trigger_shift_char_fires() {
        // Shift keeps printable text (capital letters, shifted symbols).
        let k = mk_key(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(always_on_trigger_char(&k), Some('A'));
    }

    #[test]
    fn always_on_trigger_ctrl_does_not_fire() {
        let k = mk_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_alt_does_not_fire() {
        let k = mk_key(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_enter_does_not_fire() {
        let k = mk_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_backspace_does_not_fire() {
        let k = mk_key(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_arrow_does_not_fire() {
        let k = mk_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_esc_does_not_fire() {
        let k = mk_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_function_key_does_not_fire() {
        let k = mk_key(KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_unicode_fires() {
        // Wide character typical of JP IME-produced input reaching
        // ccmux (if the host passes it as Char rather than a paste).
        let k = mk_key(KeyCode::Char('あ'), KeyModifiers::NONE);
        assert_eq!(always_on_trigger_char(&k), Some('あ'));
    }

    #[test]
    fn always_on_trigger_ctrl_alt_does_not_fire() {
        // Windows AltGr surfaces as CTRL | ALT; must not open the
        // overlay or the composed character wouldn't reach the
        // terminal's own AltGr-driven keymap.
        let k = mk_key(
            KeyCode::Char('e'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_super_does_not_fire() {
        let k = mk_key(KeyCode::Char('a'), KeyModifiers::SUPER);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_meta_does_not_fire() {
        let k = mk_key(KeyCode::Char('a'), KeyModifiers::META);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_hyper_does_not_fire() {
        let k = mk_key(KeyCode::Char('a'), KeyModifiers::HYPER);
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_on_trigger_ctrl_shift_does_not_fire() {
        let k = mk_key(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(always_on_trigger_char(&k), None);
    }

    #[test]
    fn always_mode_dismissal_and_refocus_cycle() {
        // Exercises the dismissed_pane / last_focused_pane state
        // machine without touching any PTY or UI: we manipulate the
        // App's focus fields directly and only assert on the
        // auto-open-gating logic inside
        // maybe_auto_open_always_overlay. Non-Claude shells fail the
        // `is_claude_running` gate, so the overlay itself never
        // actually opens here — instead we cover focus-change
        // dismissal clearing, idempotency on stable focus, and
        // suppression persistence until focus moves.
        let mut app = App::new(40, 80).expect("App::new");
        app.ime_mode = crate::config::ImeMode::Always;

        let pane_a = app.ws().focused_pane_id;

        // Seed a dismissal on the currently-focused pane.
        app.always_dismissed_pane = Some(pane_a);
        app.last_focused_pane = Some(pane_a);

        // Re-invoking on the same focus must not clear the dismissal.
        app.maybe_auto_open_always_overlay();
        assert_eq!(
            app.always_dismissed_pane,
            Some(pane_a),
            "dismissal should persist while focus doesn't move"
        );

        // Simulate focus moving to a different pane (the pane need
        // not actually exist for the dismissal-clear logic; the
        // method bails out before touching `panes` when the pane id
        // is unknown, but the clear runs first).
        app.ws_mut().focused_pane_id = pane_a + 100;
        app.maybe_auto_open_always_overlay();
        assert_eq!(
            app.always_dismissed_pane, None,
            "focus change should clear the dismissal"
        );
    }

    #[test]
    fn always_mode_noop_when_disabled() {
        let mut app = App::new(40, 80).expect("App::new");
        // Default mode is Hotkey; auto-open must never run.
        let pane_a = app.ws().focused_pane_id;
        app.always_dismissed_pane = Some(pane_a);
        app.maybe_auto_open_always_overlay();
        // Dismissal is not touched — the method short-circuits on
        // `ime_mode != Always` without observing or clearing state.
        assert_eq!(app.always_dismissed_pane, Some(pane_a));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn test_layout_single_pane() {
        let layout = LayoutNode::Leaf { pane_id: 1 };
        assert_eq!(layout.pane_count(), 1);
        assert_eq!(layout.collect_pane_ids(), vec![1]);
    }

    #[test]
    fn test_layout_split_vertical() {
        let mut layout = LayoutNode::Leaf { pane_id: 1 };
        layout.split_pane(1, 2, SplitDirection::Vertical);
        assert_eq!(layout.pane_count(), 2);
        assert_eq!(layout.collect_pane_ids(), vec![1, 2]);
    }

    #[test]
    fn test_layout_split_horizontal() {
        let mut layout = LayoutNode::Leaf { pane_id: 1 };
        layout.split_pane(1, 2, SplitDirection::Horizontal);
        assert_eq!(layout.pane_count(), 2);
    }

    #[test]
    fn test_layout_nested_split() {
        let mut layout = LayoutNode::Leaf { pane_id: 1 };
        layout.split_pane(1, 2, SplitDirection::Vertical);
        layout.split_pane(1, 3, SplitDirection::Horizontal);
        assert_eq!(layout.pane_count(), 3);
        assert_eq!(layout.collect_pane_ids(), vec![1, 3, 2]);
    }

    #[test]
    fn test_layout_remove_pane() {
        let mut layout = LayoutNode::Leaf { pane_id: 1 };
        layout.split_pane(1, 2, SplitDirection::Vertical);
        layout.remove_pane(2);
        assert_eq!(layout.pane_count(), 1);
        assert_eq!(layout.collect_pane_ids(), vec![1]);
    }

    #[test]
    fn test_layout_remove_first_pane() {
        let mut layout = LayoutNode::Leaf { pane_id: 1 };
        layout.split_pane(1, 2, SplitDirection::Vertical);
        layout.remove_pane(1);
        assert_eq!(layout.collect_pane_ids(), vec![2]);
    }

    #[test]
    fn test_calculate_rects_vertical() {
        let layout = LayoutNode::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf { pane_id: 1 }),
            second: Box::new(LayoutNode::Leaf { pane_id: 2 }),
        };
        let rects = layout.calculate_rects(Rect::new(0, 0, 100, 50));
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], (1, Rect::new(0, 0, 50, 50)));
        assert_eq!(rects[1], (2, Rect::new(50, 0, 50, 50)));
    }

    #[test]
    fn test_calculate_rects_horizontal() {
        let layout = LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf { pane_id: 1 }),
            second: Box::new(LayoutNode::Leaf { pane_id: 2 }),
        };
        let rects = layout.calculate_rects(Rect::new(0, 0, 100, 50));
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], (1, Rect::new(0, 0, 100, 25)));
        assert_eq!(rects[1], (2, Rect::new(0, 25, 100, 25)));
    }

    #[test]
    fn test_focus_cycling() {
        let ids = [1, 2, 3];
        assert_eq!(1 % ids.len(), 1);
        assert_eq!((2 + 1) % ids.len(), 0);
    }

    // ─── resolve_pane_ref_impl (Phase 3 Step 3.2) ────────────

    fn mk_ids(ids: &[usize]) -> HashSet<usize> {
        ids.iter().copied().collect()
    }

    #[test]
    fn resolve_focused_returns_focused_id_when_known() {
        let names = HashMap::new();
        let ids = mk_ids(&[1, 2, 3]);
        assert_eq!(
            resolve_pane_ref_impl(&PaneRef::Focused, &names, &ids, 2),
            Some(2)
        );
    }

    #[test]
    fn resolve_focused_returns_none_when_focus_stale() {
        let names = HashMap::new();
        let ids = mk_ids(&[1, 3]);
        assert_eq!(
            resolve_pane_ref_impl(&PaneRef::Focused, &names, &ids, 2),
            None
        );
    }

    #[test]
    fn resolve_by_id_returns_id_when_known() {
        let names = HashMap::new();
        let ids = mk_ids(&[1, 2, 3]);
        assert_eq!(
            resolve_pane_ref_impl(&PaneRef::Id(3), &names, &ids, 1),
            Some(3)
        );
    }

    #[test]
    fn resolve_by_id_returns_none_when_unknown() {
        let names = HashMap::new();
        let ids = mk_ids(&[1, 2]);
        assert_eq!(
            resolve_pane_ref_impl(&PaneRef::Id(99), &names, &ids, 1),
            None
        );
    }

    #[test]
    fn resolve_by_name_returns_id_when_registered() {
        let mut names = HashMap::new();
        names.insert("engineering".to_string(), 7);
        let ids = mk_ids(&[1, 7]);
        assert_eq!(
            resolve_pane_ref_impl(&PaneRef::Name("engineering".into()), &names, &ids, 1),
            Some(7)
        );
    }

    #[test]
    fn resolve_by_name_returns_none_when_unregistered() {
        let names = HashMap::new();
        let ids = mk_ids(&[1, 7]);
        assert_eq!(
            resolve_pane_ref_impl(&PaneRef::Name("missing".into()), &names, &ids, 1),
            None
        );
    }

    #[test]
    fn resolve_by_name_returns_none_when_pane_closed() {
        // Name still registered but the pane has been removed — the
        // dangling entry must not resolve to a ghost id.
        let mut names = HashMap::new();
        names.insert("engineering".to_string(), 7);
        let ids = mk_ids(&[1]); // 7 has been closed
        assert_eq!(
            resolve_pane_ref_impl(&PaneRef::Name("engineering".into()), &names, &ids, 1),
            None
        );
    }

    // ─── apply_layout integration (Phase 2 review fix) ────────
    //
    // These tests spawn real PTYs through App::new / Pane::new_with_cwd
    // because apply_layout drives split_focused_pane, which unavoidably
    // creates child shell processes. On a dev machine this costs a few
    // milliseconds per pane; in CI it's measurable but acceptable for a
    // handful of tests. Each test calls `app.shutdown()` at the end so
    // the spawned shells don't linger.

    fn make_pane_spec(id: &str) -> crate::layout_config::LayoutNodeSpec {
        crate::layout_config::LayoutNodeSpec::Pane {
            id: id.to_string(),
            command: None,
            role: None,
        }
    }

    #[test]
    fn apply_layout_maps_split_first_to_target_and_second_to_new() {
        // Given a 2-pane Split spec, after apply_layout:
        // - pane_names["left"]  must point at the workspace's original
        //   pane (what we split off)
        // - pane_names["right"] must point at the freshly-created pane
        // - the LayoutNode tree must have Leaf(left) in `first` and
        //   Leaf(right) in `second`
        // Regression guard for the reviewer's concern that the
        // first/second recursion arms in apply_layout_node might drift.
        let cfg = crate::layout_config::LayoutConfig {
            version: 1,
            name: "test".into(),
            root: crate::layout_config::LayoutNodeSpec::Split {
                direction: crate::layout_config::DirectionSpec::Vertical,
                ratio: 0.5,
                first: Box::new(make_pane_spec("left")),
                second: Box::new(make_pane_spec("right")),
            },
        };

        let mut app = App::new(40, 80).expect("App::new");
        let initial_pane_id = app.ws().focused_pane_id;

        app.apply_layout(&cfg).expect("apply_layout");

        let ws = app.ws();
        let left_id = *ws
            .pane_names
            .get("left")
            .expect("pane_names[left] should be registered");
        let right_id = *ws
            .pane_names
            .get("right")
            .expect("pane_names[right] should be registered");

        assert_eq!(
            left_id, initial_pane_id,
            "`first` spec ('left') must map to the original/split-target pane"
        );
        assert_ne!(
            right_id, initial_pane_id,
            "`second` spec ('right') must map to a newly-spawned pane"
        );

        match &ws.layout {
            LayoutNode::Split { first, second, .. } => {
                match first.as_ref() {
                    LayoutNode::Leaf { pane_id } => {
                        assert_eq!(*pane_id, left_id, "layout.first must be the 'left' pane")
                    }
                    other => panic!("expected Leaf in first, got {other:?}"),
                }
                match second.as_ref() {
                    LayoutNode::Leaf { pane_id } => {
                        assert_eq!(*pane_id, right_id, "layout.second must be the 'right' pane")
                    }
                    other => panic!("expected Leaf in second, got {other:?}"),
                }
            }
            other => panic!("expected Split at root, got {other:?}"),
        }

        app.shutdown();
    }

    #[test]
    fn apply_layout_nested_split_preserves_positions() {
        // A right-heavy tree: Split { first: "A", second: Split { "B", "C" } }.
        // After apply, the LayoutNode must mirror that shape exactly.
        let cfg = crate::layout_config::LayoutConfig {
            version: 1,
            name: "nested".into(),
            root: crate::layout_config::LayoutNodeSpec::Split {
                direction: crate::layout_config::DirectionSpec::Vertical,
                ratio: 0.5,
                first: Box::new(make_pane_spec("A")),
                second: Box::new(crate::layout_config::LayoutNodeSpec::Split {
                    direction: crate::layout_config::DirectionSpec::Horizontal,
                    ratio: 0.5,
                    first: Box::new(make_pane_spec("B")),
                    second: Box::new(make_pane_spec("C")),
                }),
            },
        };

        let mut app = App::new(40, 80).expect("App::new");
        app.apply_layout(&cfg).expect("apply_layout");

        let ws = app.ws();
        let a_id = *ws.pane_names.get("A").expect("A registered");
        let b_id = *ws.pane_names.get("B").expect("B registered");
        let c_id = *ws.pane_names.get("C").expect("C registered");

        match &ws.layout {
            LayoutNode::Split {
                first: outer_first,
                second: outer_second,
                ..
            } => {
                match outer_first.as_ref() {
                    LayoutNode::Leaf { pane_id } => assert_eq!(*pane_id, a_id),
                    other => panic!("outer.first must be Leaf(A), got {other:?}"),
                }
                match outer_second.as_ref() {
                    LayoutNode::Split {
                        first: inner_first,
                        second: inner_second,
                        ..
                    } => {
                        match inner_first.as_ref() {
                            LayoutNode::Leaf { pane_id } => assert_eq!(*pane_id, b_id),
                            other => panic!("inner.first must be Leaf(B), got {other:?}"),
                        }
                        match inner_second.as_ref() {
                            LayoutNode::Leaf { pane_id } => assert_eq!(*pane_id, c_id),
                            other => panic!("inner.second must be Leaf(C), got {other:?}"),
                        }
                    }
                    other => panic!("outer.second must be a Split, got {other:?}"),
                }
            }
            other => panic!("expected Split at root, got {other:?}"),
        }

        app.shutdown();
    }

    #[test]
    fn handle_close_refuses_last_pane_of_only_tab() {
        // Fresh App has exactly one workspace with one pane; closing
        // that pane must fail with the `last_pane` code so subscribers
        // can distinguish "can't close" from "doesn't exist".
        let mut app = App::new(40, 80).expect("App::new");
        let only = app.ws().focused_pane_id;

        let err = app
            .handle_close(&ipc::PaneRef::Id(only))
            .expect_err("closing the last pane must fail");
        assert_eq!(err.code, Some(ipc::err_code::LAST_PANE));

        // Pane still alive.
        assert!(app.ws().panes.contains_key(&only));
        app.shutdown();
    }

    #[test]
    fn handle_close_returns_pane_not_found_for_bogus_id() {
        let mut app = App::new(40, 80).expect("App::new");
        let err = app
            .handle_close(&ipc::PaneRef::Id(9_999))
            .expect_err("bogus id should fail");
        assert_eq!(err.code, Some(ipc::err_code::PANE_NOT_FOUND));
        app.shutdown();
    }

    #[test]
    fn handle_close_removes_pane_and_returns_id() {
        // Build a 2-pane layout so `handle_close` has something to
        // remove without tripping the last-pane guard.
        let cfg = crate::layout_config::LayoutConfig {
            version: 1,
            name: "close-test".into(),
            root: crate::layout_config::LayoutNodeSpec::Split {
                direction: crate::layout_config::DirectionSpec::Vertical,
                ratio: 0.5,
                first: Box::new(make_pane_spec("left")),
                second: Box::new(make_pane_spec("right")),
            },
        };
        let mut app = App::new(40, 80).expect("App::new");
        app.apply_layout(&cfg).expect("apply_layout");

        let right_id = *app.ws().pane_names.get("right").expect("right registered");
        let left_id = *app.ws().pane_names.get("left").expect("left registered");

        let closed = app
            .handle_close(&ipc::PaneRef::Name("right".into()))
            .expect("close right pane");
        assert_eq!(closed, right_id);

        let ws = app.ws();
        assert!(!ws.panes.contains_key(&right_id), "pane must be removed");
        assert!(ws.panes.contains_key(&left_id), "left must survive");
        assert!(
            !ws.pane_names.contains_key("right"),
            "pane_names entry must be dropped"
        );
        assert_eq!(ws.layout.pane_count(), 1);
        assert_eq!(ws.focused_pane_id, left_id);

        app.shutdown();
    }

    #[test]
    fn handle_close_in_background_tab_marks_dirty_and_updates_list() {
        // Cover the Codex review bug: closing a pane that lives in a
        // non-active workspace must still schedule a render and make
        // the pane disappear from subsequent `ccmux list` snapshots on
        // the freshly-touched tab. Prior to the fix the dirty flag
        // stayed low because `mark_layout_change` was gated on the
        // active tab.
        let cfg = crate::layout_config::LayoutConfig {
            version: 1,
            name: "bg-close".into(),
            root: crate::layout_config::LayoutNodeSpec::Split {
                direction: crate::layout_config::DirectionSpec::Vertical,
                ratio: 0.5,
                first: Box::new(make_pane_spec("bg-left")),
                second: Box::new(make_pane_spec("bg-right")),
            },
        };
        let mut app = App::new(40, 80).expect("App::new");
        app.apply_layout(&cfg).expect("apply_layout");

        // The 2-pane layout lives in workspace 0. Open a second tab so
        // workspace 0 becomes a background tab; the new tab (index 1)
        // becomes active with a single fresh pane.
        app.new_tab().expect("new_tab");
        assert_eq!(app.active_tab, 1);
        let active_focus_before = app.ws().focused_pane_id;

        // Clear the dirty flag set by new_tab so we can attribute the
        // next mutation to `handle_close` only.
        app.dirty = false;

        let bg_right_id = app.workspaces[0]
            .pane_names
            .get("bg-right")
            .copied()
            .expect("bg-right registered");

        let closed = app
            .handle_close(&ipc::PaneRef::Id(bg_right_id))
            .expect("close bg-right");
        assert_eq!(closed, bg_right_id);

        assert!(
            app.dirty,
            "close on a background workspace must schedule a repaint"
        );
        assert!(
            !app.workspaces[0].panes.contains_key(&bg_right_id),
            "bg-right must be gone from workspace 0"
        );
        // Active tab must be untouched.
        assert_eq!(app.active_tab, 1);
        assert_eq!(app.ws().focused_pane_id, active_focus_before);

        app.shutdown();
    }

    #[test]
    fn close_releases_pane_name_for_reuse() {
        // After close, the stable name must be available again so a
        // subsequent `ccmux split --id same-name` doesn't collide with
        // a dangling entry.
        let cfg = crate::layout_config::LayoutConfig {
            version: 1,
            name: "reuse".into(),
            root: crate::layout_config::LayoutNodeSpec::Split {
                direction: crate::layout_config::DirectionSpec::Vertical,
                ratio: 0.5,
                first: Box::new(make_pane_spec("keeper")),
                second: Box::new(make_pane_spec("victim")),
            },
        };
        let mut app = App::new(40, 80).expect("App::new");
        app.apply_layout(&cfg).expect("apply_layout");

        let victim_id_before = *app.ws().pane_names.get("victim").expect("registered");
        app.handle_close(&ipc::PaneRef::Name("victim".into()))
            .expect("close victim");
        assert!(!app.ws().pane_names.contains_key("victim"));

        // Split again asking for the same name; handle_split must
        // succeed and the new pane id must be different from the old.
        let new_id = app
            .handle_split(
                &ipc::PaneRef::Focused,
                ipc::Direction::Vertical,
                None,
                Some("victim".into()),
                None,
            )
            .expect("split with reused name");
        assert_ne!(new_id, victim_id_before);
        assert_eq!(
            app.ws().pane_names.get("victim").copied(),
            Some(new_id),
            "pane_names must point at the freshly-created pane, not the dead one"
        );

        app.shutdown();
    }

    #[test]
    fn close_after_natural_exit_does_not_double_emit() {
        // The EOF detection path and the CLI close path both guard on
        // `Pane.exit_event_emitted`, so a subscriber must see at most
        // one `PaneExited` per pane id regardless of order. We can't
        // drive a real EOF in a unit test, but we can simulate the
        // race by flipping the flag manually before calling
        // `handle_close` — exercising the same guard the natural-exit
        // path would have used.
        let cfg = crate::layout_config::LayoutConfig {
            version: 1,
            name: "race".into(),
            root: crate::layout_config::LayoutNodeSpec::Split {
                direction: crate::layout_config::DirectionSpec::Vertical,
                ratio: 0.5,
                first: Box::new(make_pane_spec("a")),
                second: Box::new(make_pane_spec("b")),
            },
        };
        let mut app = App::new(40, 80).expect("App::new");
        app.apply_layout(&cfg).expect("apply_layout");

        let (_sub_id, rx) = app.event_bus.subscribe();
        let b_id = *app.ws().pane_names.get("b").expect("b registered");

        // Simulate PtyEof having beaten us to the emission.
        app.workspaces[0]
            .panes
            .get_mut(&b_id)
            .expect("pane b")
            .exit_event_emitted = true;

        let closed = app
            .handle_close(&ipc::PaneRef::Id(b_id))
            .expect("close pane b");
        assert_eq!(closed, b_id);
        assert!(!app.ws().panes.contains_key(&b_id));

        // Drain any events that did fire. The pre-flag means
        // handle_close must not emit PaneExited for b_id.
        let mut saw_b_exited = false;
        while let Ok(ev) = rx.try_recv() {
            if let ipc::Event::PaneExited { id, .. } = ev {
                if id == b_id {
                    saw_b_exited = true;
                }
            }
        }
        assert!(
            !saw_b_exited,
            "PaneExited for b must be suppressed when exit_event_emitted was already set"
        );

        app.shutdown();
    }

    #[test]
    fn handle_split_emits_pane_started_with_attached_name_and_role() {
        // Regression: previously split_focused_pane emitted PaneStarted
        // before handle_split attached name / role, so subscribers saw
        // `name: None, role: None` and could never filter on the
        // stable identifier. Guard against that regression by
        // subscribing and verifying the emitted event carries both.
        let mut app = App::new(40, 80).expect("App::new");
        let (_sub_id, rx) = app.event_bus.subscribe();

        let id = app
            .handle_split(
                &ipc::PaneRef::Focused,
                ipc::Direction::Vertical,
                None,
                Some("worker-1".into()),
                Some("worker".into()),
            )
            .expect("split succeeds");

        let mut observed: Option<(Option<String>, Option<String>)> = None;
        while let Ok(ev) = rx.try_recv() {
            if let ipc::Event::PaneStarted {
                id: ev_id,
                name,
                role,
                ..
            } = ev
            {
                if ev_id == id {
                    observed = Some((name, role));
                    break;
                }
            }
        }
        let (name, role) = observed.expect("PaneStarted for new pane");
        assert_eq!(name.as_deref(), Some("worker-1"));
        assert_eq!(role.as_deref(), Some("worker"));

        app.shutdown();
    }

    #[test]
    fn apply_layout_emits_pane_started_after_leaf_metadata_is_attached() {
        // Regression: apply_layout_node's Split arm used to emit
        // PaneStarted for the freshly-created pane before recursing
        // into the leaf that attaches its role, so subscribers saw
        // the new pane with `role: None`.
        let cfg = crate::layout_config::LayoutConfig {
            version: 1,
            name: "role-test".into(),
            root: crate::layout_config::LayoutNodeSpec::Split {
                direction: crate::layout_config::DirectionSpec::Vertical,
                ratio: 0.5,
                first: Box::new(crate::layout_config::LayoutNodeSpec::Pane {
                    id: "keeper".into(),
                    command: None,
                    role: Some("keeper-role".into()),
                }),
                second: Box::new(crate::layout_config::LayoutNodeSpec::Pane {
                    id: "new-leaf".into(),
                    command: None,
                    role: Some("leaf-role".into()),
                }),
            },
        };

        let mut app = App::new(40, 80).expect("App::new");
        let (_sub_id, rx) = app.event_bus.subscribe();

        app.apply_layout(&cfg).expect("apply_layout");

        let new_leaf_id = *app.ws().pane_names.get("new-leaf").expect("registered");
        let mut observed: Option<(Option<String>, Option<String>)> = None;
        while let Ok(ev) = rx.try_recv() {
            if let ipc::Event::PaneStarted {
                id: ev_id,
                name,
                role,
                ..
            } = ev
            {
                if ev_id == new_leaf_id {
                    observed = Some((name, role));
                    break;
                }
            }
        }
        let (name, role) = observed.expect("PaneStarted for freshly-split leaf");
        assert_eq!(name.as_deref(), Some("new-leaf"));
        assert_eq!(role.as_deref(), Some("leaf-role"));

        app.shutdown();
    }

    #[test]
    fn split_refused_keeps_focus_and_emits_no_pane_started() {
        // Drive handle_split into its refused arm (pane too small
        // after halving, below `min_pane_width` — default 20) and
        // confirm:
        //   * SPLIT_REFUSED bubbles up,
        //   * focus stays where it was,
        //   * the requested name is NOT registered,
        //   * no PaneStarted event leaks out for the nonexistent pane.
        let mut app = App::new(40, 80).expect("App::new");

        // First split succeeds: 80 cols minus file-tree (20) = 60 cols
        // of pane area → two panes of ~30 cols each.
        app.handle_split(
            &ipc::PaneRef::Focused,
            ipc::Direction::Vertical,
            None,
            Some("first".into()),
            None,
        )
        .expect("first split should succeed");

        let focus_before = app.ws().focused_pane_id;
        let (_sub_id, rx) = app.event_bus.subscribe();

        // Second vertical split on the now-focused ~30-col pane would
        // produce ~15-col children, below `min_pane_width` (default
        // 20) → refuse.
        let err = app
            .handle_split(
                &ipc::PaneRef::Focused,
                ipc::Direction::Vertical,
                None,
                Some("overflow".into()),
                None,
            )
            .expect_err("too-narrow split must be refused");
        assert_eq!(err.code, Some(ipc::err_code::SPLIT_REFUSED));

        assert_eq!(
            app.ws().focused_pane_id,
            focus_before,
            "refused split must not move focus"
        );
        assert!(
            !app.ws().pane_names.contains_key("overflow"),
            "refused split must not register its requested name"
        );

        let any_started = rx
            .try_iter()
            .any(|ev| matches!(ev, ipc::Event::PaneStarted { .. }));
        assert!(!any_started, "refused split must not emit PaneStarted");

        app.shutdown();
    }

    #[test]
    fn set_min_pane_size_lets_split_succeed_below_default_threshold() {
        // With defaults (20 / 5), a second vertical split on a
        // ~30-col pane refuses (same geometry as
        // `split_refused_keeps_focus_and_emits_no_pane_started`).
        // Lowering the threshold via `set_min_pane_size` must let the
        // same split succeed and emit exactly one PaneStarted with
        // the attached name. Exercises the runtime wiring that CLI
        // parse tests cannot cover.
        let mut app = App::new(40, 80).expect("App::new");

        // First split runs under defaults (20 / 5) so the cached rect
        // geometry feeding the second split is identical to the
        // refusal test's setup — only the threshold itself differs.
        app.handle_split(
            &ipc::PaneRef::Focused,
            ipc::Direction::Vertical,
            None,
            Some("first".into()),
            None,
        )
        .expect("first split should succeed");

        // Lower the threshold just before the split that would
        // otherwise refuse, so the causal contrast with the sibling
        // refusal test is explicit in the test body.
        app.set_min_pane_size(10, 3);

        let (_sub_id, rx) = app.event_bus.subscribe();

        let new_id = app
            .handle_split(
                &ipc::PaneRef::Focused,
                ipc::Direction::Vertical,
                None,
                Some("narrow".into()),
                None,
            )
            .expect("split should succeed once min_pane_width is lowered");

        assert_eq!(app.ws().focused_pane_id, new_id, "focus moves to new pane");
        assert_eq!(
            app.ws().pane_names.get("narrow").copied(),
            Some(new_id),
            "requested name registers on success"
        );

        let started_ids: Vec<usize> = rx
            .try_iter()
            .filter_map(|ev| match ev {
                ipc::Event::PaneStarted { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(
            started_ids,
            vec![new_id],
            "exactly one PaneStarted for the freshly-created pane"
        );

        app.shutdown();
    }

    #[test]
    fn set_min_pane_size_clamps_zero_to_one() {
        // `--min-pane-width 0` would make `rect.width / 2 < 0` always
        // false and let splits succeed on 1-col panes. The setter
        // must floor the value at 1.
        let mut app = App::new(40, 80).expect("App::new");
        app.set_min_pane_size(0, 0);
        assert_eq!(app.min_pane_width, 1);
        assert_eq!(app.min_pane_height, 1);
        app.shutdown();
    }

    #[test]
    fn handle_new_tab_emits_pane_started_with_attached_name_and_role() {
        // Same race as handle_split, but for handle_new_tab: metadata
        // must be attached before emitting PaneStarted.
        let mut app = App::new(40, 80).expect("App::new");
        let (_sub_id, rx) = app.event_bus.subscribe();

        let id = app
            .handle_new_tab(
                None,
                Some("tab-pane".into()),
                Some("tab label".into()),
                Some("tab-role".into()),
            )
            .expect("new tab succeeds");

        let mut observed: Option<(Option<String>, Option<String>)> = None;
        while let Ok(ev) = rx.try_recv() {
            if let ipc::Event::PaneStarted {
                id: ev_id,
                name,
                role,
                ..
            } = ev
            {
                if ev_id == id {
                    observed = Some((name, role));
                    break;
                }
            }
        }
        let (name, role) = observed.expect("PaneStarted for new tab's pane");
        assert_eq!(name.as_deref(), Some("tab-pane"));
        assert_eq!(role.as_deref(), Some("tab-role"));

        app.shutdown();
    }

    #[test]
    fn list_command_includes_rect_from_last_pane_rects() {
        let mut app = App::new(40, 80).expect("App::new");
        let pane_id = app.ws().focused_pane_id;
        app.ws_mut().last_pane_rects = vec![(
            pane_id,
            Rect {
                x: 2,
                y: 3,
                width: 50,
                height: 20,
            },
        )];

        let (reply_tx, reply_rx) = oneshot::channel();
        app.handle_app_command(AppCommand::List { reply: reply_tx });
        let infos = reply_rx.recv().expect("list reply");

        assert_eq!(infos.len(), 1);
        let info = &infos[0];
        assert_eq!(info.id, pane_id);
        assert!(info.focused);
        assert_eq!(info.x, 2);
        assert_eq!(info.y, 3);
        assert_eq!(info.width, 50);
        assert_eq!(info.height, 20);
    }

    #[test]
    fn relayout_panes_caches_rect_origin_accounting_for_sidebar() {
        // Before #80, relayout_panes() used Rect::new(0, tab_h, ...)
        // because only width/height mattered for PTY sizing. Now that
        // `ccmux list` also exposes x/y from the same cache, the
        // origin must match ui::render_main_area's chunk order (tree
        // on the left, preview on the swapped side) — otherwise a
        // List call between a layout change and the next draw would
        // return x=0 for a pane that's actually rendered past the
        // file-tree sidebar.
        let mut app = App::new(40, 120).expect("App::new");
        app.last_term_size = (120, 40);
        // Workspace::new sets file_tree_visible = true; set it
        // explicitly here to make the test's precondition obvious.
        app.ws_mut().file_tree_visible = true;
        let tree_w = app.file_tree_width;
        assert!(tree_w > 0, "file tree width should be non-zero");

        app.relayout_panes();

        let pane_id = app.ws().focused_pane_id;
        let rect = app
            .ws()
            .last_pane_rects
            .iter()
            .find(|(id, _)| *id == pane_id)
            .map(|(_, r)| *r)
            .expect("relayout should populate rect for focused pane");
        assert_eq!(
            rect.x, tree_w,
            "pane origin must sit past the file-tree sidebar"
        );
        assert_eq!(rect.y, 1, "pane origin must sit below the tab strip");
    }

    #[test]
    fn relayout_panes_rect_origin_follows_layout_swapped_preview() {
        // With `layout_swapped = true` the chunk order is
        // [tree] [preview] [panes] [...]. Pane origin must therefore
        // include the preview width too. With `layout_swapped = false`
        // preview sits to the right of the panes and does not offset
        // the origin.
        use std::path::PathBuf;

        for swapped in [true, false] {
            let mut app = App::new(40, 160).expect("App::new");
            app.last_term_size = (160, 40);
            app.ws_mut().file_tree_visible = true;
            // Activate preview without touching disk: is_active() just
            // checks Preview::file_path.is_some().
            app.ws_mut().preview.file_path = Some(PathBuf::from("dummy"));
            app.layout_swapped = swapped;

            let tree_w = app.file_tree_width;
            let preview_w = app.preview_width;

            app.relayout_panes();

            let pane_id = app.ws().focused_pane_id;
            let rect = app
                .ws()
                .last_pane_rects
                .iter()
                .find(|(id, _)| *id == pane_id)
                .map(|(_, r)| *r)
                .expect("relayout should populate rect for focused pane");
            let expected_x = tree_w + if swapped { preview_w } else { 0 };
            assert_eq!(
                rect.x, expected_x,
                "swapped={swapped}: pane x should be {expected_x} (tree_w={tree_w}, preview_w={preview_w})"
            );
        }
    }

    #[test]
    fn list_command_ignores_stale_rect_entries_for_removed_panes() {
        // Entries in last_pane_rects for pane ids that are no longer
        // in the layout must not leak into the response. Output is
        // keyed off layout.collect_pane_ids(), so a stale rect for a
        // nonexistent id should simply be dropped.
        let mut app = App::new(40, 80).expect("App::new");
        let pane_id = app.ws().focused_pane_id;
        let ghost_id = pane_id.wrapping_add(9999);
        app.ws_mut().last_pane_rects = vec![
            (
                pane_id,
                Rect {
                    x: 1,
                    y: 2,
                    width: 10,
                    height: 5,
                },
            ),
            (
                ghost_id,
                Rect {
                    x: 100,
                    y: 100,
                    width: 100,
                    height: 100,
                },
            ),
        ];

        let (reply_tx, reply_rx) = oneshot::channel();
        app.handle_app_command(AppCommand::List { reply: reply_tx });
        let infos = reply_rx.recv().expect("list reply");

        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, pane_id);
        assert_eq!(infos[0].width, 10);
        assert!(
            infos.iter().all(|i| i.id != ghost_id),
            "stale rect for removed pane leaked into list"
        );
    }

    #[test]
    fn list_command_zero_rect_when_pane_not_in_last_pane_rects() {
        let mut app = App::new(40, 80).expect("App::new");
        app.ws_mut().last_pane_rects.clear();

        let (reply_tx, reply_rx) = oneshot::channel();
        app.handle_app_command(AppCommand::List { reply: reply_tx });
        let infos = reply_rx.recv().expect("list reply");

        assert_eq!(infos.len(), 1);
        let info = &infos[0];
        assert_eq!(info.x, 0);
        assert_eq!(info.y, 0);
        assert_eq!(info.width, 0);
        assert_eq!(info.height, 0);
    }

    #[test]
    fn app_command_channel_sends_and_receives() {
        // Smoke-test that the AppCommand channel round-trips without
        // panicking. We can't exercise handle_* without spawning PTYs,
        // but confirming the types fit together catches breakage.
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(AppCommand::List { reply: reply_tx }).unwrap();
        match rx.try_recv() {
            Ok(AppCommand::List { reply }) => {
                reply.send(Vec::new()).unwrap();
                let list = reply_rx.recv().unwrap();
                assert!(list.is_empty());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
