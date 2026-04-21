//! Spike: standalone stdio MCP server that proves Claude Code renders our
//! `notifications/claude/channel` as a `<channel source="ccmux-peers">` tag in
//! its context.
//!
//! Scope: loopback only. `send_message(to, message)` emits a channel
//! notification back to the **same** Claude instance that invoked it. No
//! cross-instance routing, no ccmux IPC. The whole point is to de-risk the
//! protocol/flag dance before wiring the real integration in issue #97.
//!
//! Known limitation: loopback cannot exercise the "interrupt mid-tool-chain"
//! UX that makes peer messaging feel live. The channel notification here is
//! always written *after* `send_message`'s own response, so there is no
//! concurrent tool execution to interrupt. Proving that Claude Code actually
//! pauses an in-flight tool chain to respond to a peer message is a job for
//! the Integration PR, where two real MCP subprocesses exchange frames.
//!
//! Manual test (loopback needs only one Claude):
//!   1. `cargo build --bin ccmux-mcp-peer-spike`
//!   2. Register the built binary as an MCP server in Claude Code (see the
//!      end-of-file block comment for the exact `claude mcp add-json` line).
//!   3. `claude --dangerously-load-development-channels server:ccmux-peers`
//!   4. Ask Claude to call `send_message(to_id="self", message="hello")`
//!      and confirm a `<channel source="ccmux-peers">` tag appears in the
//!      next turn.
//!
//! Everything in this file is spike-quality. Don't build on top of it; the
//! integration PR will replace it with a proper module under `src/mcp_peer/`.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const SERVER_NAME: &str = "ccmux-peers";
const SERVER_VERSION: &str = "0.0.1-spike";

fn log_stderr(msg: &str) {
    eprintln!("[ccmux-mcp-peer-spike] {msg}");
}

/// Serialise `value` + '\n' and write it as a single stdio frame. MCP stdio
/// transport is newline-delimited JSON-RPC; each message is exactly one line.
/// `Stdout` already handles interior locking, so we acquire a fresh lock for
/// each frame rather than holding one across the whole process lifetime.
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

fn tools_spec() -> Value {
    json!([
        {
            "name": "list_peers",
            "description": "Spike: returns a single fake peer so a calling Claude has a to_id to aim at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scope": {
                        "type": "string",
                        "enum": ["machine", "directory", "repo"],
                        "description": "Accepted for wire-compat with claude-peers-mcp. Ignored by the spike."
                    }
                }
            }
        },
        {
            "name": "send_message",
            "description": "Spike: loopback. Whatever message you send is pushed back into your own context as a channel notification, so you can see what channel delivery looks like without a second peer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to_id":   { "type": "string", "description": "Ignored by the spike; include anything." },
                    "message": { "type": "string", "description": "Text to loop back as a channel notification." }
                },
                "required": ["to_id", "message"]
            }
        },
        {
            "name": "set_summary",
            "description": "Spike: accepted and ignored. v1 surface uses pane name/role instead of summary.",
            "inputSchema": {
                "type": "object",
                "properties": { "summary": { "type": "string" } },
                "required": ["summary"]
            }
        },
        {
            "name": "check_messages",
            "description": "Spike: always reports an empty inbox. Channel push is the primary delivery path.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn instructions_blob() -> String {
    "You are connected to the ccmux-peers network (spike build). Other Claude Code instances \
running in the same ccmux tab can see you and send you messages.\n\n\
IMPORTANT: When you receive a <channel source=\"ccmux-peers\" ...> message, RESPOND IMMEDIATELY. \
Do not wait until your current task is finished. Pause what you are doing, reply to the sender \
using send_message, then resume your work. Treat incoming peer messages like a coworker tapping \
you on the shoulder — answer right away, even if you're in the middle of something.\n\n\
Read the from_id and from_summary attributes to understand who sent the message. Reply by \
calling send_message with their from_id.\n\n\
Available tools:\n\
- list_peers: Discover other Claude Code instances in the same ccmux tab.\n\
- send_message: Send a message to another instance by peer ID.\n\
- set_summary: (stub in the spike) Set a 1-2 sentence summary of what you're working on.\n\
- check_messages: Manually drain your inbox (fallback; channel push is the primary path)."
        .to_string()
}

/// Build a JSON-RPC 2.0 success response for request `id` carrying `result`.
fn ok_response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Build a JSON-RPC 2.0 error response. Method-not-found uses -32601 per spec.
fn err_response(id: &Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

/// Build the `notifications/claude/channel` push that makes a message show up
/// as `<channel source="ccmux-peers">...</channel>` in the receiver's context.
/// Schema mirrors claude-peers-mcp (server.ts:447-458). Claude Code derives
/// the channel tag's `source=` attribute from `serverInfo.name` returned at
/// initialize, not from the notification payload, so we do not duplicate it
/// into `params.meta`.
fn channel_notification(content: &str, from_id: &str, from_summary: &str) -> Value {
    let sent_at = chrono_like_now();
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/claude/channel",
        "params": {
            "content": content,
            "meta": {
                "from_id": from_id,
                "from_summary": from_summary,
                "from_cwd": "",
                "sent_at": sent_at
            }
        }
    })
}

/// Cheap ISO-8601-ish timestamp without pulling `chrono` into the dep tree.
/// The spike only needs a deterministic string; Claude Code doesn't parse it.
fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("spike-ts-{}.{:09}", d.as_secs(), d.subsec_nanos())
}

fn handle_initialize(id: &Value, params: &Value) -> Value {
    let client_protocol = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("2025-06-18");
    log_stderr(&format!(
        "initialize: client protocolVersion={client_protocol}"
    ));
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

/// Wraps a tool result's plain text into the MCP `content` envelope
/// expected by `tools/call` responses.
fn tool_text_result(text: &str) -> Value {
    json!({
        "content": [ { "type": "text", "text": text } ],
        "isError": false
    })
}

/// Returns `(response_frame, maybe_notification_frame)`. Separating the two
/// keeps the dispatch loop the sole writer to stdout — the handler never
/// touches stdout itself, so there is no ordering race to reason about.
fn handle_tools_call(id: &Value, params: &Value) -> Result<(Value, Option<Value>)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("tools/call missing 'name'"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match name {
        "list_peers" => {
            let peers_text =
                "ID: self\nName: spike-loopback\nRole: dev\nPosition: same\nSummary: loopback target for the ccmux-peers spike — send_message(to_id=\"self\", ...) echoes into your own channel.";
            Ok((ok_response(id, tool_text_result(peers_text)), None))
        }
        "send_message" => {
            let to_id = args.get("to_id").and_then(|v| v.as_str()).unwrap_or("self");
            let message = args
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(empty message)");
            let ack = ok_response(
                id,
                tool_text_result(&format!("Loopback delivery queued to {to_id}.")),
            );
            let note = channel_notification(
                message,
                "self",
                "spike loopback (ccmux-peers) — no real peer, this is your own message coming back",
            );
            Ok((ack, Some(note)))
        }
        "set_summary" => Ok((
            ok_response(
                id,
                tool_text_result("Spike: summary accepted and dropped on the floor."),
            ),
            None,
        )),
        "check_messages" => Ok((
            ok_response(
                id,
                tool_text_result(
                    "No queued messages. Channel push is the primary delivery path in this spike.",
                ),
            ),
            None,
        )),
        other => Ok((
            err_response(id, -32601, &format!("unknown tool: {other}")),
            None,
        )),
    }
}

/// Returns the frames (0-2) to emit in reply to this inbound frame.
/// Notifications produce `[]`; normal requests produce `[response]`; a
/// `send_message` call produces `[response, channel_push]`.
fn dispatch(req: &Value) -> Result<Vec<Value>> {
    let is_notification = req.get("id").is_none();
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    // Missing or non-string method is a malformed request per JSON-RPC 2.0.
    // Surface that as -32600 (Invalid Request) on the same id so the client
    // can correlate, rather than bubbling up as a generic -32603.
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

    log_stderr(&format!(
        "dispatch: method={method} id={} notif={is_notification}",
        req.get("id").cloned().unwrap_or(Value::Null)
    ));

    if is_notification {
        match method {
            "notifications/initialized"
            | "initialized"
            | "notifications/cancelled"
            | "$/cancel" => {}
            other => log_stderr(&format!("ignored unknown notification: {other}")),
        }
        return Ok(Vec::new());
    }

    let frames = match method {
        "initialize" => vec![handle_initialize(&id, &params)],
        "tools/list" => vec![handle_tools_list(&id)],
        "tools/call" => {
            let (resp, maybe_note) = handle_tools_call(&id, &params)?;
            match maybe_note {
                Some(note) => vec![resp, note],
                None => vec![resp],
            }
        }
        "ping" => vec![ok_response(&id, json!({}))],
        other => vec![err_response(
            &id,
            -32601,
            &format!("method not found: {other}"),
        )],
    };
    Ok(frames)
}

fn main() -> Result<()> {
    log_stderr(&format!("starting {SERVER_NAME} v{SERVER_VERSION}"));
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
        match dispatch(&value) {
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

/*
 * REGISTRATION SNIPPET — drop this into `~/.claude/mcp_servers.json` (or run
 * the equivalent `claude mcp add-json` command) so Claude Code knows to spawn
 * this binary as a stdio MCP server.
 *
 * Unix / macOS:
 *   "ccmux-peers": {
 *     "command": "/absolute/path/to/target/debug/ccmux-mcp-peer-spike",
 *     "args": []
 *   }
 *
 * Windows (absolute path must include the `.exe` suffix):
 *   "ccmux-peers": {
 *     "command": "C:/absolute/path/to/target/debug/ccmux-mcp-peer-spike.exe",
 *     "args": []
 *   }
 *
 * Then launch Claude Code with:
 *
 *   claude --dangerously-load-development-channels server:ccmux-peers
 *
 * Ask it to call send_message(to_id="self", message="ping"). You should see a
 * `<channel source="ccmux-peers">ping</channel>` tag in the next turn —
 * that's the confirmation that the channel wiring is correct.
 */
