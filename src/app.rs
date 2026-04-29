use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

mod app_core;
mod app_state;
mod codex_peer;
mod ipc_handlers;
mod keyboard_input;
mod layout_ops;
mod layout_tree;
mod pointer_input;
mod runtime;
mod selection;
mod sidebar_input;
mod workspace_state;

pub(crate) use self::app_state::CLAUDE_PEER_LAUNCH_CMD;
pub use self::app_state::{App, AppCommand, AppEvent};
use self::codex_peer::{
    write_input_to_pane, CodexPeerNotificationState, PendingCodexPeerDelivery,
};
#[cfg(test)]
use self::codex_peer::{
    codex_prompt_allows_peer_nudge_on_screen, format_codex_peer_message, screen_tail_lines,
    PendingCodexPeerMessage,
};
pub(crate) use self::keyboard_input::key_event_to_bytes_pub;
use self::keyboard_input::{extract_preview_selected_text, extract_selected_text};
use self::layout_ops::{
    default_command_for_role, dir_name, resolve_optional_cwd, strip_verbatim_prefix,
};
pub use self::layout_tree::{LayoutNode, SplitDirection};
#[cfg(test)]
use self::pointer_input::{mouse_forward_disabled, pane_local_coords, pane_local_coords_clamped};
pub use self::selection::{SelectionTarget, TextSelection};
#[cfg(test)]
use self::workspace_state::resolve_pane_ref_impl;
pub use self::workspace_state::{DragTarget, FocusTarget, Workspace};
use crate::filetree::FileTree;
use crate::ipc::{self, PaneInfo, PaneRef, PeerClientKind, PeerInfo};
use crate::layout_config::{DirectionSpec, LayoutConfig, LayoutNodeSpec};
use crate::pane::{Pane, PointerAction, PointerButton};
use crate::preview::Preview;

// ─── IME composition overlay ──────────────────────────────
//
// `OverlayState` and its modal key handler (`handle_overlay_key`) live
// in [`crate::input::overlay`] — first slice of Issue #66. Re-exported
// here so downstream code can keep referring to `crate::app::OverlayState`
// during the rest of the #66 rollout.
pub use crate::input::overlay::OverlayState;
#[cfg(test)]
mod tests;
