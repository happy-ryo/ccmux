//! `ccmux mcp install / uninstall / status` — thin wrappers around the
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
//! falling back to file-editing: the user installed ccmux with MCP
//! features, so Claude Code being available is a reasonable
//! expectation to assert.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::cli::McpAction;

const SERVER_NAME: &str = "ccmux-peers";

/// Entry point from `main.rs` for the `ccmux mcp <action>` subcommand.
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
    let exe = current_ccmux_exe()?;

    if let Some(existing) = find_existing_entry()? {
        if !force {
            println!(
                "ccmux-peers is already registered → {existing}\n\
                 Re-run with `ccmux mcp install --force` to overwrite with: {}",
                exe.display()
            );
            return Ok(());
        }
        // Force path: drop the old entry first so `add-json` doesn't error
        // on the name already being taken.
        remove_silent()?;
    }

    let payload = serde_json::json!({
        "type": "stdio",
        "command": exe.to_string_lossy().to_string(),
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
        "Registered {SERVER_NAME} → {}\n\
         Next: launch Claude with \
         `claude --dangerously-load-development-channels server:{SERVER_NAME}` \
         from inside a ccmux pane.",
        exe.display()
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
    println!("Removed {SERVER_NAME} from Claude's MCP config.");
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
                 Run `ccmux mcp install` to register it."
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
             or add it to PATH, then re-run `ccmux mcp install / uninstall / status`."
        )),
    }
}

fn current_ccmux_exe() -> Result<PathBuf> {
    std::env::current_exe()
        .context("resolve path to the running ccmux binary (needed for mcp_servers.json)")
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
