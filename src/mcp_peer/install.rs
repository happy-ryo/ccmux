//! `renga mcp install / uninstall / status` — thin wrappers around
//! client MCP management commands.
//!
//! We intentionally do **not** edit Claude Code's or Codex's config
//! files directly. Both CLIs already own their MCP configuration
//! formats, so we delegate registration through `claude mcp ...` or
//! `codex mcp ...` instead of tracking on-disk schema details here.
//!
//! The tradeoff is a hard dependency on the target client binary being
//! on PATH. We surface a clear error in that case rather than silently
//! falling back to file-editing.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;

use anyhow::{anyhow, bail, Context, Result};

use super::ENV_CLIENT_KIND;
use crate::cli::{McpAction, McpClient};

const SERVER_NAME: &str = "renga-peers";
const CODEX_PASSTHROUGH_ENV_VARS: &[&str] = &[
    "RENGA_PANE_ID",
    "RENGA_SOCKET",
    "RENGA_TOKEN",
    "RENGA_CODEX_AUTO_NUDGE",
];

/// Entry point from `main.rs` for the `renga mcp <action>` subcommand.
pub fn run(action: &McpAction) -> Result<()> {
    match action {
        McpAction::Install { force, client } => install(*force, *client),
        McpAction::Uninstall { client } => uninstall(*client),
        McpAction::Status { client } => status(*client),
    }
}

// ── install ────────────────────────────────────────────────────

fn install(force: bool, client: McpClient) -> Result<()> {
    ensure_client_cli_available(client)?;
    let exe = current_renga_exe()?;

    if let Some(existing) = find_existing_entry(client)? {
        if !force {
            if client == McpClient::Codex {
                ensure_codex_env_var_passthrough()?;
            }
            println!(
                "{SERVER_NAME} is already registered in {} → {existing}\n\
                 Re-run with `renga mcp install --client {client} --force` to overwrite with: {}",
                client_display_name(client),
                exe.display()
            );
            return Ok(());
        }
        remove_silent(client)?;
    }

    match client {
        McpClient::Claude => install_claude(&exe)?,
        McpClient::Codex => install_codex(&exe)?,
    }

    println!("{}", install_success_message(client, &exe));
    Ok(())
}

fn install_claude(exe: &Path) -> Result<()> {
    let exe_str = exe.to_str().ok_or_else(|| {
        anyhow!(
            "renga binary path is not valid UTF-8 ({}); cannot register as a Claude MCP command. \
             Move the binary to a UTF-8 path and re-run `renga mcp install --client claude`.",
            exe.display()
        )
    })?;

    let payload_str = claude_payload(exe_str)?;
    let status = client_command(McpClient::Claude)?
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
    Ok(())
}

fn install_codex(exe: &Path) -> Result<()> {
    let status = client_command(McpClient::Codex)?
        .args(codex_add_args(exe))
        .status()
        .context("spawn `codex mcp add`")?;
    if !status.success() {
        bail!("`codex mcp add` exited with status {status}");
    }
    ensure_codex_env_var_passthrough()?;
    Ok(())
}

fn install_success_message(client: McpClient, exe: &Path) -> String {
    match client {
        McpClient::Claude => format!(
            "Registered {SERVER_NAME} in Claude Code → {}\n\
             Next: launch Claude Code with \
             `claude --dangerously-load-development-channels server:{SERVER_NAME}` \
             from inside a renga pane (or press Alt+P in a pane to insert the \
             same command).",
            exe.display()
        ),
        McpClient::Codex => format!(
            "Registered {SERVER_NAME} in Codex → {}\n\
             Next: launch Codex from inside a renga pane. This registration \
             injects `{ENV_CLIENT_KIND}=codex`, so peer messages are received \
             through `check_messages` instead of Claude channels.",
            exe.display()
        ),
    }
}

// ── uninstall ──────────────────────────────────────────────────

fn uninstall(client: McpClient) -> Result<()> {
    ensure_client_cli_available(client)?;
    if find_existing_entry(client)?.is_none() {
        println!(
            "{SERVER_NAME} is not registered in {}; nothing to do.",
            client_display_name(client)
        );
        return Ok(());
    }
    remove_silent(client)?;
    println!(
        "Removed {SERVER_NAME} from {} MCP config.",
        client_display_name(client)
    );
    Ok(())
}

fn remove_silent(client: McpClient) -> Result<()> {
    let status = match client {
        McpClient::Claude => client_command(McpClient::Claude)?
            .args(["mcp", "remove", SERVER_NAME, "--scope", "user"])
            .status()
            .context("spawn `claude mcp remove`")?,
        McpClient::Codex => client_command(McpClient::Codex)?
            .args(["mcp", "remove", SERVER_NAME])
            .status()
            .context("spawn `codex mcp remove`")?,
    };
    if !status.success() {
        let cmd = match client {
            McpClient::Claude => "`claude mcp remove`",
            McpClient::Codex => "`codex mcp remove`",
        };
        bail!("{cmd} exited with status {status}");
    }
    Ok(())
}

// ── status ─────────────────────────────────────────────────────

fn status(client: McpClient) -> Result<()> {
    ensure_client_cli_available(client)?;
    match find_existing_entry(client)? {
        Some(line) => {
            println!(
                "{SERVER_NAME} is registered in {}:\n{line}",
                client_display_name(client)
            );
            Ok(())
        }
        None => {
            println!(
                "{SERVER_NAME} is NOT registered in {}.\n\
                 Run `renga mcp install --client {client}` to register it.",
                client_display_name(client)
            );
            Ok(())
        }
    }
}

// ── helpers ────────────────────────────────────────────────────

fn ensure_client_cli_available(client: McpClient) -> Result<()> {
    let binary = client_binary(client);
    let probe = client_command(client).and_then(|mut cmd| {
        cmd.arg("--version");
        cmd.output().context("spawn client `--version` probe")
    });
    match probe {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(anyhow!(
            "`{binary}` is on PATH but `{binary} --version` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(anyhow!(
            "`{binary}` CLI not found on PATH ({e}). Install {} first, \
             or add it to PATH, then re-run `renga mcp install / uninstall / status --client {client}`.",
            client_display_name(client)
        )),
    }
}

fn current_renga_exe() -> Result<PathBuf> {
    std::env::current_exe()
        .context("resolve path to the running renga binary (needed for client MCP registration)")
}

fn find_existing_entry(client: McpClient) -> Result<Option<String>> {
    match client {
        McpClient::Claude => find_existing_claude_entry(),
        McpClient::Codex => find_existing_codex_entry(),
    }
}

/// Run `claude mcp list` and return the line mentioning our server, if
/// any. `claude mcp list` output is human-readable text, not JSON, so
/// we grep rather than parse structurally.
fn find_existing_claude_entry() -> Result<Option<String>> {
    let out = client_command(McpClient::Claude)?
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

fn find_existing_codex_entry() -> Result<Option<String>> {
    let out = client_command(McpClient::Codex)?
        .args(["mcp", "get", SERVER_NAME, "--json"])
        .output()
        .context("spawn `codex mcp get`")?;
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(Some("<empty codex registration output>".to_string()));
        }
        return Ok(Some(stdout));
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if is_codex_missing_entry(&stderr) {
        return Ok(None);
    }
    bail!("`codex mcp get` failed: {stderr}");
}

fn claude_payload(exe_str: &str) -> Result<String> {
    serde_json::to_string(&serde_json::json!({
        "type": "stdio",
        "command": exe_str,
        "args": ["mcp-peer"],
    }))
    .context("serialize Claude MCP config payload")
}

fn codex_add_args(exe: &Path) -> Vec<OsString> {
    vec![
        OsString::from("mcp"),
        OsString::from("add"),
        OsString::from(SERVER_NAME),
        OsString::from("--env"),
        OsString::from(format!("{ENV_CLIENT_KIND}=codex")),
        OsString::from("--"),
        exe.as_os_str().to_owned(),
        OsString::from("mcp-peer"),
    ]
}

fn ensure_codex_env_var_passthrough() -> Result<()> {
    let path = codex_config_path()?;
    let current = fs::read_to_string(&path)
        .with_context(|| format!("read Codex config at {}", path.display()))?;
    let updated = upsert_codex_env_var_passthrough(&current).ok_or_else(|| {
        anyhow!(
            "Codex config at {} does not contain an [{section}] section after registration",
            path.display(),
            section = codex_server_section_name()
        )
    })?;
    if updated != current {
        fs::write(&path, updated)
            .with_context(|| format!("write Codex config at {}", path.display()))?;
    }
    Ok(())
}

fn codex_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow!("could not resolve the current user's home directory"))?;
    Ok(home.join(".codex").join("config.toml"))
}

fn codex_server_section_name() -> String {
    format!("mcp_servers.{SERVER_NAME}")
}

fn upsert_codex_env_var_passthrough(src: &str) -> Option<String> {
    let header = format!("[{}]", codex_server_section_name());
    let newline = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let had_trailing_newline = src.ends_with(newline);
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut found_section = false;
    let mut wrote_env_vars = false;

    for line in src.lines() {
        let trimmed = line.trim();
        if !in_section {
            if trimmed == header {
                in_section = true;
                found_section = true;
            }
            out.push(line.to_string());
            continue;
        }

        if trimmed.starts_with('[') {
            if !wrote_env_vars {
                while out.last().is_some_and(|line| line.trim().is_empty()) {
                    out.pop();
                }
                out.push(codex_env_vars_line());
                out.push(String::new());
                wrote_env_vars = true;
            }
            in_section = false;
            out.push(line.to_string());
            continue;
        }

        if trimmed.starts_with("env_vars") {
            out.push(codex_env_vars_line());
            wrote_env_vars = true;
        } else {
            out.push(line.to_string());
        }
    }

    if !found_section {
        return None;
    }
    if in_section && !wrote_env_vars {
        out.push(codex_env_vars_line());
    }

    let mut rebuilt = out.join(newline);
    if had_trailing_newline {
        rebuilt.push_str(newline);
    }
    Some(rebuilt)
}

fn codex_env_vars_line() -> String {
    format!(
        "env_vars = [{}]",
        CODEX_PASSTHROUGH_ENV_VARS
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn is_codex_missing_entry(stderr: &str) -> bool {
    stderr.contains(&format!("No MCP server named '{SERVER_NAME}' found."))
}

fn client_binary(client: McpClient) -> &'static str {
    match client {
        McpClient::Claude => "claude",
        McpClient::Codex => "codex",
    }
}

fn client_display_name(client: McpClient) -> &'static str {
    match client {
        McpClient::Claude => "Claude Code",
        McpClient::Codex => "Codex",
    }
}

fn client_command(client: McpClient) -> Result<Command> {
    let path = resolve_client_binary(client)?;
    if cfg!(windows) && is_cmd_script(&path) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(path);
        return Ok(cmd);
    }
    Ok(Command::new(path))
}

fn resolve_client_binary(client: McpClient) -> Result<PathBuf> {
    let binary = client_binary(client);
    find_binary_on_path(binary).ok_or_else(|| {
        anyhow!(
            "`{binary}` CLI not found on PATH (program not found). Install {} first, \
             or add it to PATH, then re-run `renga mcp install / uninstall / status --client {client}`.",
            client_display_name(client)
        )
    })
}

fn find_binary_on_path(binary: &str) -> Option<PathBuf> {
    let binary_path = Path::new(binary);
    if binary_path.components().count() > 1 || binary_path.is_absolute() {
        return is_launchable_file(binary_path).then(|| binary_path.to_path_buf());
    }

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for candidate in candidate_filenames(binary_path.as_os_str()) {
            let full = dir.join(&candidate);
            if is_launchable_file(&full) {
                return Some(full);
            }
        }
    }
    None
}

fn candidate_filenames(binary: &OsStr) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let path = Path::new(binary);
        if path.extension().is_some() {
            return vec![binary.to_os_string()];
        }

        let mut names = Vec::with_capacity(5);
        for ext in [".exe", ".com", ".cmd", ".bat"] {
            let mut candidate = binary.to_os_string();
            candidate.push(ext);
            names.push(candidate);
        }
        names.push(binary.to_os_string());
        names
    }

    #[cfg(not(windows))]
    {
        vec![binary.to_os_string()]
    }
}

fn is_launchable_file(path: &Path) -> bool {
    path.is_file()
}

fn is_cmd_script(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_payload_uses_stdio_command_shape() {
        let payload = claude_payload("C:/Program Files/renga/renga.exe").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!({
                "type": "stdio",
                "command": "C:/Program Files/renga/renga.exe",
                "args": ["mcp-peer"],
            })
        );
    }

    #[test]
    fn codex_add_args_includes_pull_client_env() {
        let args = codex_add_args(Path::new("C:/Program Files/renga/renga.exe"));
        let rendered: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            rendered,
            vec![
                "mcp",
                "add",
                SERVER_NAME,
                "--env",
                "RENGA_PEER_CLIENT_KIND=codex",
                "--",
                "C:/Program Files/renga/renga.exe",
                "mcp-peer",
            ]
        );
    }

    #[test]
    fn codex_missing_entry_detection_matches_cli_message() {
        assert!(is_codex_missing_entry(
            "Error: No MCP server named 'renga-peers' found."
        ));
        assert!(!is_codex_missing_entry(
            "Error: failed to load configuration"
        ));
    }

    #[test]
    fn candidate_filenames_prioritize_windows_launchable_extensions() {
        let rendered: Vec<String> = candidate_filenames(OsStr::new("codex"))
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        #[cfg(windows)]
        assert_eq!(
            rendered,
            vec!["codex.exe", "codex.com", "codex.cmd", "codex.bat", "codex",]
        );
        #[cfg(not(windows))]
        assert_eq!(rendered, vec!["codex"]);
    }

    #[test]
    fn cmd_script_detection_matches_batch_extensions() {
        assert!(is_cmd_script(Path::new("C:/Users/example/codex.cmd")));
        assert!(is_cmd_script(Path::new("C:/Users/example/codex.BAT")));
        assert!(!is_cmd_script(Path::new("C:/Users/example/codex.exe")));
    }

    #[test]
    fn upsert_codex_env_var_passthrough_inserts_before_env_subtable() {
        let input = concat!(
            "[mcp_servers.renga-peers]\n",
            "command = 'C:\\Users\\iwama\\.cargo\\bin\\renga.exe'\n",
            "args = [\"mcp-peer\"]\n",
            "\n",
            "[mcp_servers.renga-peers.env]\n",
            "RENGA_PEER_CLIENT_KIND = \"codex\"\n"
        );
        let output = upsert_codex_env_var_passthrough(input).unwrap();
        assert!(output.contains(&format!(
            "{}\n\n[mcp_servers.renga-peers.env]",
            codex_env_vars_line()
        )));
    }

    #[test]
    fn upsert_codex_env_var_passthrough_replaces_existing_line() {
        let input = concat!(
            "[mcp_servers.renga-peers]\n",
            "command = 'renga'\n",
            "args = [\"mcp-peer\"]\n",
            "env_vars = [\"OLD\"]\n"
        );
        let output = upsert_codex_env_var_passthrough(input).unwrap();
        assert!(output.contains(&codex_env_vars_line()));
        assert!(!output.contains("env_vars = [\"OLD\"]"));
    }

    #[test]
    fn upsert_codex_env_var_passthrough_returns_none_when_server_missing() {
        let input = "[mcp_servers.other]\ncommand = 'foo'\n";
        assert!(upsert_codex_env_var_passthrough(input).is_none());
    }
}
