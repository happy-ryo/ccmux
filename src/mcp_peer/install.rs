//! `renga mcp install / uninstall / status` — thin wrappers around the
//! `claude` CLI's MCP management commands.
//!
//! We intentionally do **not** edit Claude Code's config files
//! directly. Claude Code stores MCP servers alongside unrelated user
//! settings and its on-disk schema has moved before; using `claude mcp
//! add-json` etc. delegates format ownership to Claude Code itself and
//! keeps us out of the business of tracking its config evolution.
//!
//! The tradeoff is a hard dependency on the `claude` binary being on
//! PATH. We surface a clear error in that case rather than silently
//! falling back to file-editing: the user installed renga with MCP
//! features, so Claude Code being available is a reasonable
//! expectation to assert.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::cli::McpAction;

const SERVER_NAME: &str = "renga-peers";

/// Entry point from `main.rs` for the `renga mcp <action>` subcommand.
pub fn run(action: &McpAction) -> Result<()> {
    match action {
        McpAction::Install { force } => install(*force),
        McpAction::Uninstall => uninstall(),
        McpAction::Status => status(),
    }
}

// ── install ────────────────────────────────────────────────────

fn install(force: bool) -> Result<()> {
    ensure_claude_cli_available()?;
    let exe = current_renga_exe()?;

    // Validate the exe path can even be registered BEFORE we touch
    // any existing state. If we did this after `remove_silent()` on
    // the --force path, a non-UTF-8 path would leave the user with
    // neither the old registration nor a new one. Refuse early so
    // the existing entry stays intact when we can't replace it.
    let exe_str = exe.to_str().ok_or_else(|| {
        anyhow!(
            "renga binary path is not valid UTF-8 ({}); cannot register as an MCP command. \
             Move the binary to a UTF-8 path and re-run `renga mcp install`.",
            exe.display()
        )
    })?;

    if let Some(existing) = find_existing_entry()? {
        if !force {
            println!(
                "renga-peers is already registered → {existing}\n\
                 Re-run with `renga mcp install --force` to overwrite with: {exe_str}"
            );
            return Ok(());
        }
        // Force path: drop the old entry first so `add-json` doesn't error
        // on the name already being taken.
        remove_silent()?;
    }

    let payload = serde_json::json!({
        "type": "stdio",
        "command": exe_str,
        "args": ["mcp-peer"],
    });
    let payload_str = serde_json::to_string(&payload).context("serialize mcp config payload")?;

    let status = Command::new("claude")
        .args([
            "mcp",
            "add-json",
            SERVER_NAME,
            &payload_str,
            "--scope",
            "user",
        ])
        .status()
        .context("spawn `claude mcp add-json`")?;
    if !status.success() {
        bail!("`claude mcp add-json` exited with status {status}");
    }

    println!(
        "Registered {SERVER_NAME} → {exe_str}\n\
         Next: launch Claude Code with \
         `claude --dangerously-load-development-channels server:{SERVER_NAME}` \
         from inside a renga pane (or press Alt+P in a pane to insert the \
         same command)."
    );
    Ok(())
}

// ── uninstall ──────────────────────────────────────────────────

fn uninstall() -> Result<()> {
    ensure_claude_cli_available()?;
    if find_existing_entry()?.is_none() {
        println!("{SERVER_NAME} is not registered; nothing to do.");
        return Ok(());
    }
    remove_silent()?;
    println!("Removed {SERVER_NAME} from Claude Code's MCP config.");
    Ok(())
}

fn remove_silent() -> Result<()> {
    let status = Command::new("claude")
        .args(["mcp", "remove", SERVER_NAME, "--scope", "user"])
        .status()
        .context("spawn `claude mcp remove`")?;
    if !status.success() {
        bail!("`claude mcp remove` exited with status {status}");
    }
    Ok(())
}

// ── status ─────────────────────────────────────────────────────

fn status() -> Result<()> {
    ensure_claude_cli_available()?;
    match find_existing_entry()? {
        Some(line) => {
            println!("{SERVER_NAME} is registered:\n  {line}");
            Ok(())
        }
        None => {
            println!(
                "{SERVER_NAME} is NOT registered.\n\
                 Run `renga mcp install` to register it."
            );
            Ok(())
        }
    }
}

// ── helpers ────────────────────────────────────────────────────

fn ensure_claude_cli_available() -> Result<()> {
    // `claude --version` exits fast and is non-destructive.
    let probe = Command::new("claude").arg("--version").output();
    match probe {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(anyhow!(
            "`claude` is on PATH but `claude --version` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(anyhow!(
            "`claude` CLI not found on PATH ({e}). Install Claude Code first, \
             or add it to PATH, then re-run `renga mcp install / uninstall / status`."
        )),
    }
}

fn current_renga_exe() -> Result<PathBuf> {
    std::env::current_exe()
        .context("resolve path to the running renga binary (needed for mcp_servers.json)")
}

/// Run `claude mcp list` and return the line mentioning our server, if
/// any. `claude mcp list` output is human-readable text, not JSON, so
/// we grep rather than parse structurally.
fn find_existing_entry() -> Result<Option<String>> {
    let out = Command::new("claude")
        .args(["mcp", "list"])
        .output()
        .context("spawn `claude mcp list`")?;
    if !out.status.success() {
        bail!(
            "`claude mcp list` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        if line.contains(SERVER_NAME) {
            return Ok(Some(line.trim().to_string()));
        }
    }
    Ok(None)
}
