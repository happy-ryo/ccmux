use super::*;

impl App {
    pub(crate) fn new_tab(&mut self) -> Result<usize> {
        self.new_tab_with_cwd(None)
    }

    pub(crate) fn new_tab_with_cwd(&mut self, cwd_override: Option<PathBuf>) -> Result<usize> {
        let cwd = cwd_override
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let name = dir_name(&cwd);
        let pane_id = self.next_pane_id;
        self.next_pane_id = self.next_pane_id.wrapping_add(1);

        let ws = Workspace::new(name, cwd, pane_id, 10, 40, self.event_tx.clone())?;
        self.workspaces.push(ws);
        self.active_tab = self.workspaces.len() - 1;
        self.suspend_overlay();
        Ok(pane_id)
    }

    pub(crate) fn close_tab(&mut self, index: usize) {
        if self.workspaces.len() <= 1 {
            return;
        }

        let pane_ids_in_tab: Vec<usize> = self.workspaces[index].panes.keys().copied().collect();

        let mut to_emit: Vec<(usize, Option<String>, Option<String>)> = Vec::new();
        {
            let ws = &mut self.workspaces[index];
            for pid in &pane_ids_in_tab {
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
            for pid in &pane_ids_in_tab {
                self.saved_overlay_drafts.remove(pid);
                self.claude_monitor.remove(*pid);
                self.peer_client_kinds.remove(pid);
                self.pending_codex_peer_messages.remove(pid);
            }
        }
        if self
            .codex_peer_notification
            .as_ref()
            .is_some_and(|n| pane_ids_in_tab.contains(&n.target_pane))
        {
            self.codex_peer_notification = None;
        }

        let overlay_in_tab = self
            .overlay
            .as_ref()
            .is_some_and(|o| self.workspaces[index].panes.contains_key(&o.target_pane));
        if overlay_in_tab {
            self.overlay = None;
        }

        self.workspaces[index].shutdown();
        self.workspaces.remove(index);
        let prev_active = self.active_tab;
        if self.active_tab >= self.workspaces.len() {
            self.active_tab = self.workspaces.len() - 1;
        }
        if prev_active != self.active_tab {
            self.suspend_overlay();
        }
        self.mark_layout_change();
        for (pid, name, role) in to_emit {
            self.emit_pane_exited(pid, name, role);
        }
    }

    const MAX_PANES: usize = 16;

    pub(crate) fn split_focused_pane(
        &mut self,
        direction: SplitDirection,
        cwd_override: Option<PathBuf>,
    ) -> Result<Option<usize>> {
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

        let cwd = cwd_override.or_else(|| {
            self.ws()
                .panes
                .get(&self.ws().focused_pane_id)
                .map(|p| p.cwd.clone())
        });

        let pane = Pane::new_with_cwd(new_id, 10, 40, self.event_tx.clone(), cwd)?;
        let ws = self.ws_mut();
        ws.panes.insert(new_id, pane);
        ws.layout.split_pane(ws.focused_pane_id, new_id, direction);
        ws.focused_pane_id = new_id;

        self.mark_layout_change();
        Ok(Some(new_id))
    }

    pub(crate) fn spawn_claude_in_selected_dir(&mut self, direction: SplitDirection) -> Result<()> {
        let raw_cwd = self.ws().file_tree.selected_launch_cwd();
        let canon = raw_cwd.canonicalize().unwrap_or_else(|_| raw_cwd.clone());
        let cwd = if canon.is_dir() {
            strip_verbatim_prefix(canon)
        } else {
            self.ws().file_tree.root_path.clone()
        };

        let Some(new_id) = self.split_focused_pane(direction, Some(cwd))? else {
            return Ok(());
        };

        if let Some(pane) = self.ws_mut().panes.get_mut(&new_id) {
            pane.queue_startup_text(&format!("{CLAUDE_PEER_LAUNCH_CMD} "));
        }
        self.emit_pane_started(new_id);
        Ok(())
    }

    pub fn apply_layout(&mut self, config: &LayoutConfig) -> Result<()> {
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        validate_layout_cwds(&config.root, &base)?;
        let initial_pane_id = self.ws().focused_pane_id;
        self.apply_layout_node(&config.root, initial_pane_id)?;
        Ok(())
    }

    fn apply_layout_node(&mut self, node: &LayoutNodeSpec, target_pane_id: usize) -> Result<()> {
        match node {
            LayoutNodeSpec::Pane {
                id,
                command,
                role,
                cwd: _,
            } => {
                if !id.is_empty() {
                    self.ws_mut().pane_names.insert(id.clone(), target_pane_id);
                }
                if let Some(pane) = self.ws_mut().panes.get_mut(&target_pane_id) {
                    if let Some(r) = role {
                        pane.role = Some(r.clone());
                    }
                    if let Some(cmd) = command {
                        let upgraded = crate::mcp_peer::upgrade_claude_command(cmd);
                        pane.queue_startup_command(&upgraded);
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
                self.ws_mut().focused_pane_id = target_pane_id;
                let split_dir = match direction {
                    DirectionSpec::Vertical => SplitDirection::Vertical,
                    DirectionSpec::Horizontal => SplitDirection::Horizontal,
                };
                let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let new_pane_cwd = subtree_root_cwd(second, &base);
                let new_pane_id = self
                    .split_focused_pane(split_dir, new_pane_cwd)?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "layout split refused (too small or MAX_PANES) while applying layout"
                        )
                    })?;
                self.apply_layout_node(first, target_pane_id)?;
                self.apply_layout_node(second, new_pane_id)?;
                self.emit_pane_started(new_pane_id);
                Ok(())
            }
        }
    }

    pub(crate) fn close_focused_pane(&mut self) {
        let ws_index = self.active_tab;
        let focused = self.ws().focused_pane_id;
        if self.workspaces[ws_index].layout.pane_count() <= 1 {
            return;
        }
        let _ = self.remove_pane_from_layout(ws_index, focused);
    }

    pub(crate) fn remove_pane_from_layout(
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

        ws.pane_names.retain(|_, id| *id != pane_id);
        self.drop_overlay_for_pane(pane_id);
        self.claude_monitor.remove(pane_id);
        self.peer_client_kinds.remove(&pane_id);
        self.pending_codex_peer_messages.remove(&pane_id);
        if self
            .codex_peer_notification
            .as_ref()
            .is_some_and(|n| n.target_pane == pane_id)
        {
            self.codex_peer_notification = None;
        }

        let ws = &mut self.workspaces[ws_index];
        let remaining_ids = ws.layout.collect_pane_ids();
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
            self.mark_layout_change();
        } else {
            self.dirty = true;
        }
        if let Some((name, role)) = exited_meta {
            self.emit_pane_exited(pane_id, name, role);
        }
        Ok(())
    }

    pub(crate) fn handle_close(
        &mut self,
        target: &PaneRef,
    ) -> std::result::Result<usize, ipc::CodedError> {
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
            self.close_tab(ws_index);
            return Ok(pane_id);
        }

        self.remove_pane_from_layout(ws_index, pane_id)?;
        Ok(pane_id)
    }

    pub(crate) fn resolve_pane_across_workspaces(
        &self,
        target: &PaneRef,
    ) -> Option<(usize, usize)> {
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

    pub(crate) fn focus_next_pane(&mut self) {
        let ws = self.ws_mut();
        let ids = ws.layout.collect_pane_ids();
        let tree_visible = ws.file_tree_visible;
        let preview_active = ws.preview.is_active();

        match ws.focus_target {
            FocusTarget::FileTree => {
                if preview_active {
                    ws.focus_target = FocusTarget::Preview;
                } else {
                    ws.focus_target = FocusTarget::Pane;
                }
            }
            FocusTarget::Preview => {
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

    pub(crate) fn focus_prev_pane(&mut self) {
        let ws = self.ws_mut();
        let ids = ws.layout.collect_pane_ids();
        let tree_visible = ws.file_tree_visible;
        let preview_active = ws.preview.is_active();

        match ws.focus_target {
            FocusTarget::FileTree => {
                ws.focus_target = FocusTarget::Pane;
                if let Some(&last) = ids.last() {
                    ws.focused_pane_id = last;
                }
            }
            FocusTarget::Preview => {
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
}

/// Walk a layout subtree and return the cwd that should be used for
/// the pane hosting that subtree's root position. For a bare `Pane`
/// leaf that's simply its own cwd; for a `Split` we inherit from the
/// `first` child so the parent pane (already present before the split)
/// and the subtree agree. Relative paths are joined onto `base`. A
/// `None` result means "use default" (inherit from parent pane or
/// process cwd).
pub(crate) fn subtree_root_cwd(node: &LayoutNodeSpec, base: &std::path::Path) -> Option<PathBuf> {
    match node {
        LayoutNodeSpec::Pane { cwd, .. } => cwd.as_deref().and_then(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                let p = PathBuf::from(t);
                Some(if p.is_absolute() { p } else { base.join(p) })
            }
        }),
        LayoutNodeSpec::Split { first, .. } => subtree_root_cwd(first, base),
    }
}

/// Pre-flight check: walk the layout tree and fail fast if any `cwd`
/// field doesn't point at an existing directory. Keeps the mutation
/// semantics consistent with `Request::Split` / `Request::NewTab` —
/// bad cwd = no partial layout.
pub(crate) fn validate_layout_cwds(node: &LayoutNodeSpec, base: &std::path::Path) -> Result<()> {
    match node {
        LayoutNodeSpec::Pane { cwd, id, .. } => {
            if let Some(raw) = cwd {
                let t = raw.trim();
                if !t.is_empty() {
                    let p = PathBuf::from(t);
                    let joined = if p.is_absolute() { p } else { base.join(p) };
                    let meta = std::fs::metadata(&joined).map_err(|e| {
                        anyhow::anyhow!(
                            "layout pane '{id}' cwd {} is not accessible: {e}",
                            joined.display()
                        )
                    })?;
                    if !meta.is_dir() {
                        return Err(anyhow::anyhow!(
                            "layout pane '{id}' cwd {} is not a directory",
                            joined.display()
                        ));
                    }
                }
            }
            Ok(())
        }
        LayoutNodeSpec::Split { first, second, .. } => {
            validate_layout_cwds(first, base)?;
            validate_layout_cwds(second, base)?;
            Ok(())
        }
    }
}

/// Resolve an optional `cwd` string from an IPC Split / NewTab request
/// into an absolute `PathBuf`. Relative paths are joined onto `base`
/// (the target pane's cwd for Split, the server's process cwd for
/// NewTab). Missing / non-directory paths surface as
/// [`ipc::err_code::CWD_INVALID`] so the caller can distinguish them
/// from other failure codes. Returns `Ok(None)` when the caller did
/// not supply a cwd (including empty / whitespace) — preserving the
/// pre-cwd default-inheritance behavior.
pub(crate) fn resolve_optional_cwd(
    cwd: Option<&str>,
    base: &std::path::Path,
) -> std::result::Result<Option<PathBuf>, ipc::CodedError> {
    let raw = match cwd {
        Some(s) => s.trim(),
        None => return Ok(None),
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let candidate = PathBuf::from(raw);
    let joined = if candidate.is_absolute() {
        candidate
    } else {
        base.join(candidate)
    };
    // Canonicalize so the stored cwd is stable and follows `..` /
    // symlinks deterministically. Canonicalize also implicitly
    // verifies existence — a missing path errors out here.
    let canon = std::fs::canonicalize(&joined).map_err(|e| {
        ipc::CodedError::new(
            ipc::err_code::CWD_INVALID,
            format!("cwd {} is not accessible: {e}", joined.display()),
        )
    })?;
    // Directory check goes against the canonical (verbatim-prefixed on
    // Windows) path, not the stripped one. Long paths / UNC shares can
    // fail `is_dir()` once the `\\?\` prefix is removed, so we verify
    // first and then strip purely for display/storage.
    if !canon.is_dir() {
        return Err(ipc::CodedError::new(
            ipc::err_code::CWD_INVALID,
            format!("cwd {} is not a directory", canon.display()),
        ));
    }
    // Windows' canonicalize returns a `\\?\C:\...` verbatim path,
    // which leaks into `PaneInfo.cwd` / MCP list output and looks
    // wrong in user-facing tooling. Strip the prefix for storage so
    // the PTY cwd string matches what a shell would show.
    Ok(Some(strip_verbatim_prefix(canon)))
}

/// Strip Windows `\\?\` (verbatim) and `\\?\UNC\` prefixes from a
/// canonicalized path. On non-Windows this is an identity function.
/// Kept as a free function so both the IPC cwd resolver and any
/// future code paths that serialize canonicalized paths can share one
/// definition. Prefers string manipulation over `dunce` so we don't
/// add a dependency for a few lines of path-prefix cleanup.
pub(crate) fn strip_verbatim_prefix(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().into_owned();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            // `\\?\UNC\server\share\...` → `\\server\share\...`
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
        PathBuf::from(s)
    }
    #[cfg(not(windows))]
    {
        p
    }
}

pub(crate) fn default_command_for_role(role: Option<&str>) -> Option<String> {
    match role {
        Some("claude") => Some(CLAUDE_PEER_LAUNCH_CMD.to_string()),
        _ => None,
    }
}

/// Extract directory name from a path for tab title.
pub(crate) fn dir_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}
