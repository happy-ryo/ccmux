//! `ccmux mcp-peer` — the stdio MCP server Claude Code spawns per pane.
//!
//! Stage 3 of issue #97: the real implementation that replaces
//! `src/bin/ccmux-mcp-peer-spike.rs`. Where the spike looped messages
//! back to the same Claude, this module routes them through ccmux's
//! existing IPC server so a message sent from pane A shows up in pane
//! B's context as a `<channel source="ccmux-peers">` tag — provided
//! both panes live in the same ccmux tab.
//!
//! # Lifecycle
//!
//! 1. Claude Code spawns `ccmux mcp-peer` as a stdio subprocess. The
//!    PTY env published by ccmux (`CCMUX_PANE_ID`, `CCMUX_SOCKET`,
//!    `CCMUX_TOKEN`) is inherited all the way down.
//! 2. [`run`] negotiates the MCP `initialize` handshake, declares the
//!    `claude/channel` experimental capability, and spawns a background
//!    thread that subscribes to ccmux's event bus.
//! 3. Inbound `Request::PeerSend` deliveries land on the event bus as
//!    [`crate::ipc::Event::PeerInbox`]. The background thread filters
//!    on `target_pane == our CCMUX_PANE_ID` and pushes a
//!    `notifications/claude/channel` frame to stdout — the only thing
//!    that makes peer messages show up as a channel tag instead of an
//!    ordinary tool result.
//!
//! # Outside-ccmux fallback
//!
//! If `CCMUX_PANE_ID` is absent (Claude was launched from a terminal
//! ccmux didn't spawn), the module still handshakes and advertises the
//! tools — they just return empty/no-op results. This keeps the stdio
//! MCP installed globally in `~/.claude/mcp_servers.json` from erroring
//! out every time Claude starts outside ccmux.

pub mod install;

use std::io::{self, BufRead, Write};
use std::thread;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::ipc::endpoint::{endpoint_from_env, EndpointName, ENV_SOCKET};
use crate::ipc::{self, client, PaneRef, PeerInfo, Request, Response};

const SERVER_NAME: &str = "ccmux-peers";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const ENV_PANE_ID: &str = "CCMUX_PANE_ID";

fn log_stderr(msg: &str) {
    eprintln!("[ccmux-mcp-peer] {msg}");
}

/// Entry point called by `ccmux mcp-peer`. Blocks on stdin until EOF
/// or an unrecoverable error.
pub fn run() -> Result<()> {
    log_stderr(&format!("starting {SERVER_NAME} v{SERVER_VERSION}"));

    let ctx = PeerCtx::load();
    match &ctx.mode {
        Mode::Connected { pane_id, .. } => {
            log_stderr(&format!("connected mode: pane_id={pane_id}"));
            spawn_inbox_subscriber(ctx.clone());
        }
        Mode::Detached { reason } => {
            log_stderr(&format!("detached mode: {reason}"));
        }
    }

    stdio_loop(&ctx)
}

/// Runtime context shared between the main stdio loop and the inbox
/// subscriber thread. Cloneable because both halves read the same
/// `(pane_id, endpoint)` pair to contact the ccmux server.
#[derive(Clone)]
struct PeerCtx {
    mode: Mode,
}

#[derive(Clone)]
enum Mode {
    /// Running inside a ccmux pane with a reachable IPC endpoint.
    Connected {
        pane_id: usize,
        endpoint: EndpointName,
    },
    /// Missing `CCMUX_PANE_ID` or `CCMUX_SOCKET`. Tools still respond
    /// but with empty/no-op payloads so `claude` launched outside
    /// ccmux doesn't log MCP errors on startup.
    Detached { reason: String },
}

impl PeerCtx {
    fn load() -> Self {
        let pane_id = match std::env::var(ENV_PANE_ID) {
            Ok(s) => match s.parse::<usize>() {
                Ok(v) => v,
                Err(_) => {
                    return PeerCtx {
                        mode: Mode::Detached {
                            reason: format!("{ENV_PANE_ID} is set but not a valid usize: {s:?}"),
                        },
                    };
                }
            },
            Err(_) => {
                return PeerCtx {
                    mode: Mode::Detached {
                        reason: format!("{ENV_PANE_ID} not set — Claude was not launched by ccmux"),
                    },
                };
            }
        };
        match endpoint_from_env() {
            Ok(endpoint) => PeerCtx {
                mode: Mode::Connected { pane_id, endpoint },
            },
            Err(e) => PeerCtx {
                mode: Mode::Detached {
                    reason: format!("{ENV_SOCKET} missing or invalid: {e}"),
                },
            },
        }
    }
}

// ── stdio JSON-RPC frame plumbing ─────────────────────────────

fn write_frame(value: &Value) -> Result<()> {
    let mut line = serde_json::to_string(value).context("serialize frame")?;
    line.push('\n');
    let out = io::stdout();
    let mut guard = out.lock();
    guard
        .write_all(line.as_bytes())
        .context("write frame to stdout")?;
    guard.flush().context("flush stdout")?;
    Ok(())
}

fn ok_response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err_response(id: &Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn tool_text_result(text: &str) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ], "isError": false })
}

// ── channel notification (the whole point of #97) ─────────────

/// Build the `notifications/claude/channel` push that makes a peer
/// message show up as `<channel source="ccmux-peers">...</channel>`
/// in the receiver's context. The `source=` attribute is derived by
/// Claude Code from our `serverInfo.name`, not from this payload, so
/// `params.meta` here only carries sender metadata.
fn channel_notification(body: &str, from_id: &str, from_name: Option<&str>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/claude/channel",
        "params": {
            "content": body,
            "meta": {
                "from_id": from_id,
                "from_name": from_name.unwrap_or(""),
                "sent_at": now_ts_string(),
            }
        }
    })
}

fn now_ts_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // ISO-ish with second precision — Claude Code doesn't parse this,
    // it's only shown to the receiving Claude for context.
    format!("{}.{:09}", d.as_secs(), d.subsec_nanos())
}

// ── MCP method handlers ───────────────────────────────────────

fn instructions_blob() -> String {
    "You are connected to the ccmux-peers network. Other Claude Code instances \
running in the same ccmux tab can see you and send you messages.\n\n\
IMPORTANT: When you receive a <channel source=\"ccmux-peers\" ...> message, RESPOND IMMEDIATELY. \
Do not wait until your current task is finished. Pause what you are doing, reply to the sender \
using send_message, then resume your work. Treat incoming peer messages like a coworker tapping \
you on the shoulder — answer right away, even if you're in the middle of something.\n\n\
Read the from_id and from_name attributes to understand who sent the message. Reply by \
calling send_message with their from_id.\n\n\
Available tools:\n\
- list_peers: Discover other Claude Code instances in the same ccmux tab.\n\
- send_message: Send a message to another instance by peer ID or name.\n\
- set_summary: (stub in v1) Set a 1-2 sentence summary of what you're working on.\n\
- check_messages: Manually drain your inbox (fallback; channel push is the primary path)."
        .to_string()
}

fn tools_spec() -> Value {
    json!([
        {
            "name": "list_peers",
            "description": "List other Claude Code / shell panes in the same ccmux tab. Each peer includes id, name (if assigned), role, and cwd.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scope": {
                        "type": "string",
                        "enum": ["machine", "directory", "repo"],
                        "description": "Accepted for wire-compat with claude-peers-mcp. ccmux always treats scope as the current tab; this parameter is ignored."
                    }
                }
            }
        },
        {
            "name": "send_message",
            "description": "Send a message to another pane in the same ccmux tab. The recipient Claude sees it as a <channel source=\"ccmux-peers\"> tag, distinct from user input.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to_id":   { "type": "string", "description": "Recipient pane id (from list_peers) or stable name." },
                    "message": { "type": "string", "description": "Text to deliver." }
                },
                "required": ["to_id", "message"]
            }
        },
        {
            "name": "set_summary",
            "description": "Stub in v1 — accepted and dropped. ccmux uses pane name/role as the summary substitute. Kept on the tool list so the same claude-peers-mcp skill / prompt works here.",
            "inputSchema": {
                "type": "object",
                "properties": { "summary": { "type": "string" } },
                "required": ["summary"]
            }
        },
        {
            "name": "check_messages",
            "description": "Manually drain the inbox. In v1 channel push is the only delivery path, so this currently returns 'no messages' — kept for wire-compat with claude-peers-mcp.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn handle_initialize(id: &Value, params: &Value) -> Value {
    let client_protocol = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("2025-06-18");
    ok_response(
        id,
        json!({
            "protocolVersion": client_protocol,
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            "capabilities": {
                "experimental": { "claude/channel": {} },
                "tools": {}
            },
            "instructions": instructions_blob()
        }),
    )
}

fn handle_tools_list(id: &Value) -> Value {
    ok_response(id, json!({ "tools": tools_spec() }))
}

fn handle_list_peers(id: &Value, ctx: &PeerCtx) -> Value {
    let (pane_id, endpoint) = match &ctx.mode {
        Mode::Connected { pane_id, endpoint } => (*pane_id, endpoint),
        Mode::Detached { reason } => {
            return ok_response(
                id,
                tool_text_result(&format!(
                    "(no peers — ccmux not reachable from this Claude instance: {reason})"
                )),
            );
        }
    };
    match client::send_request(endpoint, &Request::PeerList { from_pane: pane_id }) {
        Ok(Response::Ok { data }) => match serde_json::from_value::<Vec<PeerInfo>>(data) {
            Ok(peers) => ok_response(id, tool_text_result(&format_peer_list(&peers))),
            Err(e) => err_response(id, -32603, &format!("decode peer list: {e}")),
        },
        Ok(Response::Err { message, code }) => err_response(
            id,
            -32603,
            &format!("ccmux refused list_peers: {}", fmt_code(&message, &code)),
        ),
        Ok(other) => err_response(id, -32603, &format!("unexpected ccmux response: {other:?}")),
        Err(e) => err_response(id, -32603, &format!("ccmux call failed: {e}")),
    }
}

fn format_peer_list(peers: &[PeerInfo]) -> String {
    if peers.is_empty() {
        return "No peers in this tab.".to_string();
    }
    let mut out = String::from("Peers in this tab:\n\n");
    for p in peers {
        out.push_str(&format!("- id={}", p.id));
        if let Some(name) = &p.name {
            out.push_str(&format!(" name={name}"));
        }
        if let Some(role) = &p.role {
            out.push_str(&format!(" role={role}"));
        }
        if let Some(cwd) = &p.cwd {
            out.push_str(&format!("\n  cwd: {cwd}"));
        }
        out.push('\n');
    }
    out
}

fn handle_send_message(id: &Value, args: &Value, ctx: &PeerCtx) -> Value {
    let to_id = args.get("to_id").and_then(|v| v.as_str()).unwrap_or("");
    let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if to_id.is_empty() {
        return err_response(id, -32602, "send_message requires a non-empty to_id");
    }
    let (pane_id, endpoint) = match &ctx.mode {
        Mode::Connected { pane_id, endpoint } => (*pane_id, endpoint),
        Mode::Detached { reason } => {
            return ok_response(
                id,
                tool_text_result(&format!(
                    "(message dropped — ccmux not reachable: {reason})"
                )),
            );
        }
    };
    let target = match to_id.parse::<usize>() {
        Ok(n) => PaneRef::Id(n),
        Err(_) => PaneRef::Name(to_id.to_string()),
    };
    match client::send_request(
        endpoint,
        &Request::PeerSend {
            from_pane: pane_id,
            target,
            body: message.to_string(),
        },
    ) {
        Ok(Response::Ok { .. }) => {
            ok_response(id, tool_text_result(&format!("Delivered to {to_id}.")))
        }
        Ok(Response::Err { message, code }) => err_response(
            id,
            -32603,
            &format!("ccmux refused send: {}", fmt_code(&message, &code)),
        ),
        Ok(other) => err_response(id, -32603, &format!("unexpected ccmux response: {other:?}")),
        Err(e) => err_response(id, -32603, &format!("ccmux call failed: {e}")),
    }
}

fn fmt_code(message: &str, code: &Option<String>) -> String {
    match code {
        Some(c) => format!("[{c}] {message}"),
        None => message.to_string(),
    }
}

fn handle_tools_call(id: &Value, params: &Value, ctx: &PeerCtx) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("tools/call missing 'name'"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    Ok(match name {
        "list_peers" => handle_list_peers(id, ctx),
        "send_message" => handle_send_message(id, &args, ctx),
        "set_summary" => ok_response(
            id,
            tool_text_result("Summary accepted (v1 stub: ccmux displays pane name / role)."),
        ),
        "check_messages" => ok_response(
            id,
            tool_text_result(
                "No queued messages. Channel push is the primary delivery path in v1.",
            ),
        ),
        other => err_response(id, -32601, &format!("unknown tool: {other}")),
    })
}

// ── stdin dispatch loop ───────────────────────────────────────

fn dispatch(req: &Value, ctx: &PeerCtx) -> Result<Vec<Value>> {
    let is_notification = req.get("id").is_none();
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = match req.get("method").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => {
            if is_notification {
                log_stderr("dropping malformed notification with no method");
                return Ok(Vec::new());
            }
            return Ok(vec![err_response(
                &id,
                -32600,
                "invalid request: missing or non-string 'method'",
            )]);
        }
    };
    let params = req.get("params").cloned().unwrap_or(json!({}));
    if is_notification {
        // Lifecycle notifications are accepted silently; unknown ones logged.
        if !matches!(
            method,
            "notifications/initialized" | "initialized" | "notifications/cancelled" | "$/cancel"
        ) {
            log_stderr(&format!("ignored unknown notification: {method}"));
        }
        return Ok(Vec::new());
    }
    let frames = match method {
        "initialize" => vec![handle_initialize(&id, &params)],
        "tools/list" => vec![handle_tools_list(&id)],
        "tools/call" => vec![handle_tools_call(&id, &params, ctx)?],
        "ping" => vec![ok_response(&id, json!({}))],
        other => vec![err_response(
            &id,
            -32601,
            &format!("method not found: {other}"),
        )],
    };
    Ok(frames)
}

fn stdio_loop(ctx: &PeerCtx) -> Result<()> {
    let stdin = io::stdin();
    let reader = stdin.lock();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log_stderr(&format!("stdin read error: {e}"));
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                log_stderr(&format!(
                    "malformed JSON frame dropped: {e} — raw={trimmed}"
                ));
                continue;
            }
        };
        match dispatch(&value, ctx) {
            Ok(frames) => {
                for f in &frames {
                    if let Err(e) = write_frame(f) {
                        log_stderr(&format!("failed to write frame: {e}"));
                    }
                }
            }
            Err(e) => {
                log_stderr(&format!("dispatch error: {e}"));
                if let Some(id) = value.get("id") {
                    let payload = err_response(id, -32603, &format!("internal error: {e}"));
                    let _ = write_frame(&payload);
                }
            }
        }
    }
    log_stderr("stdin closed; exiting");
    Ok(())
}

// ── event bus subscriber (background thread) ──────────────────

/// Subscribe to ccmux's event bus and push any [`ipc::Event::PeerInbox`]
/// whose `target_pane` matches our own pane id as a
/// `notifications/claude/channel` frame on stdout. The thread is
/// detached — it dies naturally when the IPC stream closes (ccmux
/// exited) or when the subprocess is killed.
fn spawn_inbox_subscriber(ctx: PeerCtx) {
    let Mode::Connected { pane_id, endpoint } = ctx.mode else {
        return;
    };
    let endpoint_clone = endpoint.clone();
    thread::Builder::new()
        .name("ccmux-mcp-peer-inbox".into())
        .spawn(move || {
            let result = client::subscribe_events(&endpoint_clone, |event| {
                if let ipc::Event::PeerInbox {
                    target_pane,
                    from_pane,
                    from_name,
                    body,
                    ..
                } = event
                {
                    if target_pane == pane_id {
                        let note = channel_notification(
                            &body,
                            &from_pane.to_string(),
                            from_name.as_deref(),
                        );
                        if let Err(e) = write_frame(&note) {
                            log_stderr(&format!("failed to push channel notification: {e}"));
                        }
                    }
                }
                true
            });
            match result {
                Ok(()) => log_stderr("event stream closed"),
                Err(e) => log_stderr(&format!("event subscription ended: {e}")),
            }
        })
        .expect("spawn inbox subscriber thread");
}
