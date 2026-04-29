use super::*;

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
    /// A mouse press was forwarded to a pane running a TUI that
    /// subscribed to mouse reporting (Claude Code `/tui fullscreen`,
    /// vim, lazygit, etc.). Subsequent Drag and Up events must go to
    /// the same pane so the app sees a consistent press → drag* →
    /// release sequence, even if the cursor has wandered off the
    /// original pane by the time the drag continues. Stores the
    /// workspace index + pane id of the press target (so a tab
    /// switch mid-drag still routes back to the right pane — panes
    /// belong to workspaces, and `self.ws()` would otherwise resolve
    /// to whichever tab is active *now*), its outer rect (for local-
    /// coordinate math), and the button that started it.
    PaneMouseReport(
        /* ws_idx */ usize,
        /* pane_id */ usize,
        Rect,
        crate::pane::PointerButton,
    ),
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
    pub(crate) fn new(
        name: String,
        cwd: PathBuf,
        pane_id: usize,
        rows: u16,
        cols: u16,
        event_tx: Sender<AppEvent>,
    ) -> Result<Self> {
        // Spawn the initial pane's PTY in the workspace's cwd so
        // `new_tab_with_cwd(...)` actually takes effect. Without this
        // the shell would inherit the renga process cwd regardless of
        // what the workspace was constructed with.
        let pane = Pane::new_with_cwd(pane_id, rows, cols, event_tx, Some(cwd.clone()))?;
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

    pub(crate) fn shutdown(&mut self) {
        for pane in self.panes.values_mut() {
            pane.kill();
        }
    }

    /// Tab label to show in the UI: custom rename wins over the
    /// cwd-derived default.
    pub fn display_name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.name)
    }
}
