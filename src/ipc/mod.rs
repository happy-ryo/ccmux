//! IPC protocol for controlling a running ccmux instance from outside.
//!
//! Wire format: newline-delimited JSON. Each line is one [`Request`] from
//! client to server, followed by exactly one [`Response`] from server to
//! client. Connections are short-lived in the v1 protocol — clients open,
//! send one request, read one response, close.
//!
//! # Threat model
//!
//! IPC is a local-only control channel between processes running as
//! the same user. OS-level isolation handles the cross-user boundary;
//! IPC itself is **not** a secrecy or authentication boundary against
//! other processes running as that same user.
//!
//! - On Unix, the socket lives under an owner-only directory
//!   (`$XDG_RUNTIME_DIR/ccmux/` or `/tmp/ccmux-UID/` with mode `0700`).
//!   A different UID on the same host cannot reach it.
//! - On Windows, the Named Pipe is named `\\.\pipe\ccmux-<pid>` and
//!   inherits default session-scoped permissions from the OS.
//!
//! The `CCMUX_TOKEN` env var is **not** a secret. It exists only to
//! detect PID re-use: if a child shell inherited a stale `CCMUX_SOCKET`
//! whose PID now belongs to a different ccmux instance, the token on
//! the wire won't match the child's `CCMUX_TOKEN` and the client
//! refuses the command. Any same-user process that can read
//! `/proc/<pid>/environ` can also read the token — on the same-user
//! trust model that's already inside the boundary.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Server-side budget for waking the App event loop with a single
/// command. The App drains commands every frame (~30Hz), so 5s is
/// orders of magnitude more than the expected latency — a timeout
/// here means the App is genuinely wedged.
pub(crate) const APP_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Client margin for connect + JSON write/read + scheduling on top of
/// the server's [`APP_REPLY_TIMEOUT`]. Kept small so `ccmux send` in
/// a shell script aborts within a few seconds if something is wrong.
pub(crate) const CLIENT_MARGIN: Duration = Duration::from_secs(5);

/// Total time the client waits for a full response before erroring out.
/// Derived so the two timeouts stay in sync if one is tuned later.
pub(crate) const RESPONSE_TIMEOUT: Duration =
    Duration::from_secs(APP_REPLY_TIMEOUT.as_secs() + CLIENT_MARGIN.as_secs());

pub mod client;
pub mod endpoint;
pub mod events;
pub mod server;

pub use events::EventBus;

/// One IPC call from a client to the running ccmux instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// First message after connecting — exchanges PIDs and a session
    /// token so a stale socket file with a re-used PID cannot be
    /// silently mistaken for a live instance.
    Hello { client_pid: u32 },
    /// List all panes in the active workspace.
    List,
    /// Write `data` to the target pane's PTY. If `append_enter` is true,
    /// a newline is appended so the shell executes the command.
    Send {
        target: PaneRef,
        data: String,
        #[serde(default)]
        append_enter: bool,
    },
    /// Split the target pane and (optionally) run a command in the new
    /// pane. The new pane is named `id` if provided.
    Split {
        target: PaneRef,
        direction: Direction,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        id: Option<String>,
        /// Free-form role label (see [`PaneInfo::role`]).
        #[serde(default)]
        role: Option<String>,
    },
    /// Move keyboard focus to the target pane.
    Focus { target: PaneRef },
    /// Close the target pane. Terminates its underlying process, drops
    /// it from the layout, and emits a `PaneExited` event. If the pane
    /// is the only leaf in its workspace and other workspaces exist,
    /// the whole tab is closed. Fails with `last_pane` if it's the last
    /// pane of the only remaining tab.
    Close { target: PaneRef },
    /// Create a new tab with a fresh single pane. The server switches
    /// focus to the new tab (matching the existing Alt+T keybinding).
    NewTab {
        /// Startup command for the new pane.
        #[serde(default)]
        command: Option<String>,
        /// Stable name to register for the new pane so it can be
        /// addressed via `PaneRef::Name` later.
        #[serde(default)]
        id: Option<String>,
        /// Override the tab label (otherwise derived from the cwd).
        #[serde(default)]
        label: Option<String>,
        /// Free-form role label (see [`PaneInfo::role`]).
        #[serde(default)]
        role: Option<String>,
    },
    /// Switch the connection to live event stream mode. After the
    /// server acknowledges with [`Response::Subscribed`], it emits
    /// [`Event`] JSON Lines until the client disconnects. No further
    /// [`Request`]s are accepted on this connection.
    Subscribe,
    /// Snapshot the current visible screen of the target pane.
    /// Returns the plain-text contents in row-addressable form so
    /// orchestrators can detect prompts like "Allow this tool use?",
    /// error banners, or mode indicators without relying on worker
    /// self-reports.
    ///
    /// `lines = Some(N)` limits the response to the bottom `N` rows
    /// of the screen grid (including blank rows — the row layout is
    /// preserved on purpose so callers can match against fixed
    /// positions like the status bar). `None` returns the full
    /// visible screen. `include_cursor = true` adds a `cursor`
    /// object to the payload.
    Inspect {
        target: PaneRef,
        #[serde(default)]
        lines: Option<usize>,
        #[serde(default)]
        include_cursor: bool,
    },
}

/// Identifies a pane in a request. Names are user-friendly and stable
/// across splits; numeric ids are stable across the session but assigned
/// internally; `Focused` is "whichever pane is focused right now".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneRef {
    Id(usize),
    Name(String),
    Focused,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Vertical,
    Horizontal,
}

/// One entry in the `List` response payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Free-form label. Set via layout TOML `role = ...`, `ccmux split
    /// --role ...`, or `ccmux new-tab --role ...`. Unlike `name`, not
    /// unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub focused: bool,
    /// Last known on-screen column of the pane's top-left corner
    /// (terminal origin = 0). `0` when the pane has not been drawn yet.
    #[serde(default)]
    pub x: u16,
    /// Last known on-screen row of the pane's top-left corner.
    /// `0` when the pane has not been drawn yet.
    #[serde(default)]
    pub y: u16,
    /// Last known rendered width in columns. `0` if not yet rendered.
    #[serde(default)]
    pub width: u16,
    /// Last known rendered height in rows. `0` if not yet rendered.
    #[serde(default)]
    pub height: u16,
}

/// Server reply to one [`Request`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    /// Successful call. `data` is request-specific (e.g. the pane list).
    Ok {
        #[serde(default)]
        data: serde_json::Value,
    },
    /// Hello reply: server identifies itself with PID and a session
    /// token derived from its start time, so the client can detect
    /// PID re-use from a previous crashed instance.
    Hello {
        server_pid: u32,
        session_token: String,
    },
    /// Ack that the connection has entered event-stream mode. The
    /// server follows this with newline-delimited [`Event`] records
    /// until the client disconnects.
    Subscribed,
    /// Server-side failure. `message` is human-readable; `code` is a
    /// stable short identifier for programmatic matching (see the
    /// `err_code` module). `code` is optional for backwards
    /// compatibility with clients built against the pre-coded protocol.
    Err {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

/// Stable short identifiers for [`Response::Err::code`].
///
/// # Stability
///
/// These string values are **wire ABI**, not internal symbols.
/// Changing an existing constant's value is a breaking protocol
/// change and requires a deprecation window:
///
/// 1. Introduce the new code alongside the old one (both servers emit
///    the old value; old clients keep matching).
/// 2. Flip servers to emit the new value in the next minor release,
///    keeping the old constant exported so clients still build.
/// 3. Remove the old constant only after external clients (including
///    `aainc-ops`) have migrated.
///
/// Adding a new code is additive and safe — clients must treat
/// unknown codes as generic errors (fall back to `message`).
///
/// Heartbeat events (`Event::Heartbeat`) follow the same rule: new
/// event variants are additive, and clients skip unknown `type`
/// tags instead of aborting the stream (see
/// [`crate::ipc::client::subscribe_events`]).
pub mod err_code {
    /// The server is shutting down and cannot accept new commands.
    pub const SHUTTING_DOWN: &str = "shutting_down";
    /// The App event loop did not respond within the server-side
    /// budget. Usually means the UI thread is wedged.
    pub const APP_TIMEOUT: &str = "app_timeout";
    /// Request JSON failed to parse.
    pub const PARSE: &str = "parse";
    /// Protocol violation (wrong message at wrong time, duplicate
    /// hello, Subscribe reaching the one-shot dispatcher, etc.).
    pub const PROTOCOL: &str = "protocol";
    /// A sibling server-side invariant was violated while serializing
    /// the response payload.
    pub const INTERNAL: &str = "internal";
    /// The referenced pane (by id, name, or Focused) does not exist
    /// in the active workspace.
    pub const PANE_NOT_FOUND: &str = "pane_not_found";
    /// A pane id resolved on lookup but disappeared before the App
    /// could act on it (close / exit race). Rare.
    pub const PANE_VANISHED: &str = "pane_vanished";
    /// The workspace cannot accept another split — either the
    /// MAX_PANES cap is reached or the target pane is already at
    /// the minimum geometry.
    pub const SPLIT_REFUSED: &str = "split_refused";
    /// PTY write / spawn / OS-level I/O failure surfaced to the
    /// client so it can distinguish "setup broken" from "request
    /// invalid".
    pub const IO_ERROR: &str = "io_error";
    /// `ccmux close` was asked to remove the only pane of the only
    /// remaining tab. Refused so the TUI doesn't end up with an empty
    /// layout; the caller should shut down ccmux instead.
    pub const LAST_PANE: &str = "last_pane";
}

/// App-side error carrying a free-form message plus an optional
/// stable code from [`err_code`]. Replaces the previous
/// `Result<T, String>` reply shape on [`crate::app::AppCommand`] so
/// ccmux clients (including `aainc-ops`) can match on the code
/// instead of grepping human-readable text.
///
/// Uncoded variants still work — older App paths or new cases that
/// don't warrant a stable code yet can call
/// [`CodedError::uncoded`], and the wire response still carries the
/// message so humans can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodedError {
    pub message: String,
    pub code: Option<&'static str>,
}

impl CodedError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
        }
    }

    pub fn uncoded(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    /// Convert into a wire [`Response::Err`], preserving the code
    /// when present.
    pub fn into_response(self) -> Response {
        match self.code {
            Some(c) => Response::err_coded(c, self.message),
            None => Response::err(self.message),
        }
    }
}

impl std::fmt::Display for CodedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(c) => write!(f, "[{c}] {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl From<String> for CodedError {
    fn from(s: String) -> Self {
        CodedError::uncoded(s)
    }
}

impl From<&str> for CodedError {
    fn from(s: &str) -> Self {
        CodedError::uncoded(s.to_string())
    }
}

/// Server-pushed lifecycle event on a subscribed connection. Emitted
/// as one JSON object per line after the server has acknowledged
/// [`Request::Subscribe`] with [`Response::Subscribed`].
///
/// Delivery is **best-effort**: slow subscribers may miss events,
/// in which case the server synthesizes an [`Event::EventsDropped`]
/// meta-event. Consumers that need exact state should reconcile
/// with [`Request::List`] after reacting to a gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Emitted when a pane has been added to the active workspace.
    /// `name` is populated if the pane was given a stable IPC name
    /// (layout `id` or `ccmux split --id`). `role` is the free-form
    /// label set via Phase 1 mechanisms.
    PaneStarted {
        id: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        ts_ms: u64,
    },
    /// Emitted exactly once per pane id when it is removed from the
    /// workspace (user-initiated close, tab close, or the underlying
    /// shell exiting).
    PaneExited {
        id: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        ts_ms: u64,
    },
    /// Meta-event synthesized by the server when a slow subscriber
    /// has caused real events to be dropped. `count` is the number of
    /// events discarded since the last delivered event.
    EventsDropped { count: u64, ts_ms: u64 },
    /// Periodic keep-alive emitted while no real events are in flight.
    /// Its only purpose is to trigger a wire write so the server can
    /// detect half-closed connections (client dead but OS buffer still
    /// accepting). Clients can safely ignore it, or surface it as a
    /// liveness indicator.
    Heartbeat { ts_ms: u64 },
}

impl Response {
    pub fn ok_unit() -> Self {
        Response::Ok {
            data: serde_json::Value::Null,
        }
    }
    pub fn ok_value(value: serde_json::Value) -> Self {
        Response::Ok { data: value }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Response::Err {
            message: message.into(),
            code: None,
        }
    }
    pub fn err_coded(code: &'static str, message: impl Into<String>) -> Self {
        Response::Err {
            message: message.into(),
            code: Some(code.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(req: &Request) -> Request {
        let s = serde_json::to_string(req).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn list_request_roundtrips() {
        assert_eq!(roundtrip(&Request::List), Request::List);
    }

    #[test]
    fn send_request_roundtrips_with_enter() {
        let r = Request::Send {
            target: PaneRef::Name("engineering".into()),
            data: "hello".into(),
            append_enter: true,
        };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn split_request_roundtrips() {
        let r = Request::Split {
            target: PaneRef::Focused,
            direction: Direction::Vertical,
            command: Some("cce".into()),
            id: Some("engineering".into()),
            role: None,
        };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn pane_ref_id_serializes_with_numeric() {
        let s = serde_json::to_string(&PaneRef::Id(7)).unwrap();
        assert!(s.contains("\"id\""), "{s}");
        assert!(s.contains("7"), "{s}");
    }

    #[test]
    fn pane_ref_focused_serializes_with_unit() {
        let s = serde_json::to_string(&PaneRef::Focused).unwrap();
        assert!(s.contains("focused"), "{s}");
    }

    #[test]
    fn unknown_command_fails_to_parse() {
        let bad = r#"{"cmd":"explode","target":{"focused":null}}"#;
        let parsed: Result<Request, _> = serde_json::from_str(bad);
        assert!(parsed.is_err());
    }

    #[test]
    fn response_err_serializes_message() {
        let r = Response::err("nope");
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"err\""), "{s}");
        assert!(s.contains("nope"), "{s}");
    }

    #[test]
    fn response_ok_unit_has_null_data() {
        let r = Response::ok_unit();
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"ok\""), "{s}");
        assert!(s.contains("null"), "{s}");
    }

    #[test]
    fn hello_request_carries_pid() {
        let r = Request::Hello { client_pid: 42 };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn new_tab_request_roundtrips() {
        let r = Request::NewTab {
            command: Some("cce".into()),
            id: Some("engineering".into()),
            label: Some("eng".into()),
            role: None,
        };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn new_tab_request_defaults_all_fields() {
        let minimal = r#"{"cmd":"new_tab"}"#;
        let parsed: Request = serde_json::from_str(minimal).unwrap();
        match parsed {
            Request::NewTab {
                command: None,
                id: None,
                label: None,
                role: None,
            } => {}
            other => panic!("expected empty NewTab, got {other:?}"),
        }
    }

    #[test]
    fn hello_response_carries_token() {
        let r = Response::Hello {
            server_pid: 100,
            session_token: "abc".into(),
        };
        let parsed: Response = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn pane_info_role_is_omitted_when_none() {
        let info = PaneInfo {
            id: 1,
            name: None,
            role: None,
            focused: false,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        let s = serde_json::to_string(&info).unwrap();
        assert!(!s.contains("role"), "unexpected role field: {s}");
    }

    #[test]
    fn pane_info_role_roundtrips_when_present() {
        let info = PaneInfo {
            id: 1,
            name: Some("president".into()),
            role: Some("leader".into()),
            focused: true,
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let parsed: PaneInfo =
            serde_json::from_str(&serde_json::to_string(&info).unwrap()).unwrap();
        assert_eq!(parsed, info);
    }

    #[test]
    fn pane_info_rect_fields_roundtrip() {
        let info = PaneInfo {
            id: 7,
            name: Some("editor".into()),
            role: None,
            focused: false,
            x: 3,
            y: 1,
            width: 120,
            height: 40,
        };
        let s = serde_json::to_string(&info).unwrap();
        assert!(s.contains("\"x\":3"), "missing x: {s}");
        assert!(s.contains("\"y\":1"), "missing y: {s}");
        assert!(s.contains("\"width\":120"), "missing width: {s}");
        assert!(s.contains("\"height\":40"), "missing height: {s}");
        let parsed: PaneInfo = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, info);
    }

    #[test]
    fn pane_info_without_rect_fields_deserializes_to_zero() {
        // Older clients may emit JSON without x/y/width/height. Serde
        // defaults should fill those with 0 so the type stays
        // backward-compatible with pre-#80 payloads.
        let legacy = r#"{"id":2,"focused":true}"#;
        let parsed: PaneInfo = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.id, 2);
        assert!(parsed.focused);
        assert_eq!(parsed.x, 0);
        assert_eq!(parsed.y, 0);
        assert_eq!(parsed.width, 0);
        assert_eq!(parsed.height, 0);
    }

    #[test]
    fn split_request_with_role_roundtrips() {
        let r = Request::Split {
            target: PaneRef::Focused,
            direction: Direction::Vertical,
            command: None,
            id: None,
            role: Some("worker".into()),
        };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn new_tab_request_with_role_roundtrips() {
        let r = Request::NewTab {
            command: None,
            id: None,
            label: None,
            role: Some("leader".into()),
        };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn subscribe_request_roundtrips() {
        assert_eq!(roundtrip(&Request::Subscribe), Request::Subscribe);
    }

    #[test]
    fn subscribed_response_roundtrips() {
        let parsed: Response =
            serde_json::from_str(&serde_json::to_string(&Response::Subscribed).unwrap()).unwrap();
        assert_eq!(parsed, Response::Subscribed);
    }

    #[test]
    fn pane_started_event_roundtrips() {
        let ev = Event::PaneStarted {
            id: 3,
            name: Some("foreman".into()),
            role: Some("foreman".into()),
            ts_ms: 1_700_000_000_000,
        };
        let parsed: Event = serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        assert_eq!(parsed, ev);
    }

    #[test]
    fn pane_exited_event_omits_optional_fields_when_none() {
        let ev = Event::PaneExited {
            id: 5,
            name: None,
            role: None,
            ts_ms: 42,
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(!s.contains("\"name\""), "should omit name: {s}");
        assert!(!s.contains("\"role\""), "should omit role: {s}");
    }

    #[test]
    fn heartbeat_event_roundtrips() {
        let ev = Event::Heartbeat { ts_ms: 123 };
        let parsed: Event = serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        assert_eq!(parsed, ev);
    }

    #[test]
    fn response_err_code_is_omitted_when_none() {
        let r = Response::err("plain");
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("\"code\""), "should omit code: {s}");
    }

    #[test]
    fn response_err_coded_roundtrips() {
        let r = Response::err_coded(err_code::PROTOCOL, "boom");
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"code\""), "{s}");
        assert!(s.contains("protocol"), "{s}");
        let parsed: Response = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn response_err_without_code_parses_into_none() {
        // Pre-coded-protocol clients / servers don't emit `code` at
        // all. Confirm we round-trip their payload into Err.code = None
        // without rejecting the message.
        let legacy = r#"{"status":"err","message":"older peer"}"#;
        let parsed: Response = serde_json::from_str(legacy).unwrap();
        match parsed {
            Response::Err { message, code } => {
                assert_eq!(message, "older peer");
                assert_eq!(code, None);
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[test]
    fn coded_error_display_includes_code_prefix() {
        let e = CodedError::new(err_code::PANE_NOT_FOUND, "pane not found: Id(3)");
        let s = e.to_string();
        assert!(s.contains("[pane_not_found]"), "{s}");
        assert!(s.contains("pane not found"), "{s}");
    }

    #[test]
    fn coded_error_uncoded_display_has_no_prefix() {
        let e = CodedError::uncoded("plain message");
        assert_eq!(e.to_string(), "plain message");
    }

    #[test]
    fn coded_error_from_string_is_uncoded() {
        let e: CodedError = String::from("boom").into();
        assert_eq!(e.code, None);
        assert_eq!(e.message, "boom");
    }

    #[test]
    fn coded_error_from_str_is_uncoded() {
        let e: CodedError = "boom".into();
        assert_eq!(e.code, None);
        assert_eq!(e.message, "boom");
    }

    #[test]
    fn coded_error_into_response_preserves_code() {
        let e = CodedError::new(err_code::SPLIT_REFUSED, "too small");
        match e.into_response() {
            Response::Err { message, code } => {
                assert_eq!(message, "too small");
                assert_eq!(code.as_deref(), Some(err_code::SPLIT_REFUSED));
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[test]
    fn coded_error_into_response_omits_missing_code() {
        let e = CodedError::uncoded("no code");
        match e.into_response() {
            Response::Err { message, code } => {
                assert_eq!(message, "no code");
                assert_eq!(code, None);
            }
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[test]
    fn events_dropped_meta_event_roundtrips() {
        let ev = Event::EventsDropped {
            count: 17,
            ts_ms: 1,
        };
        let parsed: Event = serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        assert_eq!(parsed, ev);
    }

    #[test]
    fn inspect_request_roundtrips_with_defaults() {
        let r = Request::Inspect {
            target: PaneRef::Focused,
            lines: None,
            include_cursor: false,
        };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn inspect_request_roundtrips_with_lines_and_cursor() {
        let r = Request::Inspect {
            target: PaneRef::Name("worker-foo".into()),
            lines: Some(4),
            include_cursor: true,
        };
        assert_eq!(roundtrip(&r), r);
    }

    #[test]
    fn inspect_request_accepts_minimal_json() {
        // `lines` and `include_cursor` default so a minimal
        // `{cmd: inspect, target: ...}` must still parse.
        let minimal = r#"{"cmd":"inspect","target":{"focused":null}}"#;
        let parsed: Request = serde_json::from_str(minimal).unwrap();
        match parsed {
            Request::Inspect {
                target: PaneRef::Focused,
                lines: None,
                include_cursor: false,
            } => {}
            other => panic!("expected Inspect defaults, got {other:?}"),
        }
    }
}
