//! IPC server: accepts connections on a named endpoint and forwards
//! each request to the App's command channel.
//!
//! Wire protocol: newline-delimited JSON. A connection must start with
//! a `Hello` request; the server replies with a [`Response::Hello`]
//! carrying its PID and a per-instance session token. The client then
//! sends exactly one command and reads exactly one response before the
//! server closes its side.
//!
//! Threading model:
//! - One accept thread lives for the process lifetime and blocks on
//!   `listener.incoming()`.
//! - Each connection is handed to a short-lived worker thread so a slow
//!   client can't starve the accept loop.
//! - Workers communicate with the App by pushing an [`AppCommand`] into
//!   the shared `Sender<AppCommand>` and blocking on a [`oneshot`] reply
//!   with a timeout, so an unresponsive App can never hang a worker
//!   indefinitely.

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use interprocess::local_socket::{prelude::*, ListenerOptions, Stream};

use super::endpoint::{EndpointKind, EndpointName};
use super::events::EventBus;
use super::{Request, Response, APP_REPLY_TIMEOUT};
use crate::app::AppCommand;

/// Upper bound for waiting on the accept thread during shutdown.
/// `Drop` must not hang on an uncooperative accept thread — if the
/// self-connect wakeup somehow fails and the thread stays blocked in
/// `listener.incoming()`, we'd rather leak the thread (the OS reaps
/// it on process exit) than stall the whole process from teardown.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub struct IpcServer {
    pub endpoint: EndpointName,
    stop: Arc<AtomicBool>,
    /// Signaled once the accept thread returns. Using a channel rather
    /// than `JoinHandle::join` so Drop can wait with a timeout.
    done_rx: Option<mpsc::Receiver<()>>,
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Orderly shutdown so the accept thread exits before we remove
        // the socket file, avoiding the "stale listener, new path"
        // rebinding race: (1) flip the stop flag, (2) self-connect to
        // unblock the blocked `accept()` call, (3) wait for the thread
        // to signal completion (bounded), then (4) unlink on Unix.
        self.stop.store(true, Ordering::Release);
        unblock_accept(&self.endpoint);
        if let Some(rx) = self.done_rx.take() {
            let _ = rx.recv_timeout(SHUTDOWN_TIMEOUT);
        }
        if self.endpoint.kind() == EndpointKind::Socket {
            let _ = std::fs::remove_file(self.endpoint.as_str());
        }
    }
}

/// Open and immediately drop a client connection to the server's own
/// endpoint. This wakes the blocked `Listener::incoming()` call so the
/// accept thread can observe the stop flag and exit. Any error is
/// ignored — the endpoint may already be torn down from an earlier
/// Drop pass.
fn unblock_accept(endpoint: &EndpointName) {
    let name = match endpoint_to_name(endpoint) {
        Ok(n) => n,
        Err(_) => return,
    };
    let _ = Stream::connect(name);
}

fn endpoint_to_name(endpoint: &EndpointName) -> Result<interprocess::local_socket::Name<'_>> {
    #[cfg(windows)]
    {
        use interprocess::os::windows::local_socket::NamedPipe;
        Ok(endpoint.as_str().to_fs_name::<NamedPipe>()?)
    }
    #[cfg(unix)]
    {
        use interprocess::local_socket::GenericFilePath;
        Ok(endpoint.as_str().to_fs_name::<GenericFilePath>()?)
    }
}

impl IpcServer {
    /// Bind the listener and start accepting in a background thread.
    pub fn spawn(
        endpoint: EndpointName,
        command_tx: Sender<AppCommand>,
        session_token: String,
        event_bus: EventBus,
    ) -> Result<Self> {
        let listener = bind_listener(&endpoint)
            .with_context(|| format!("bind IPC endpoint {}", endpoint.as_str()))?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let token_for_thread = session_token.clone();
        let endpoint_for_log = endpoint.as_str().to_string();
        let (done_tx, done_rx) = mpsc::channel();
        thread::Builder::new()
            .name("ccmux-ipc-accept".into())
            .spawn(move || {
                accept_loop(
                    listener,
                    command_tx,
                    token_for_thread,
                    endpoint_for_log,
                    stop_for_thread,
                    event_bus,
                );
                // Signal Drop that the accept loop has returned. If the
                // receiver is already gone (Drop finished first because
                // of the timeout) the send errors out silently; we
                // don't care.
                let _ = done_tx.send(());
            })
            .context("spawn IPC accept thread")?;

        // Token is consumed by the accept thread via `token_for_thread`;
        // we don't need to keep a copy on the struct.
        drop(session_token);
        Ok(Self {
            endpoint,
            stop,
            done_rx: Some(done_rx),
        })
    }
}

fn bind_listener(endpoint: &EndpointName) -> Result<interprocess::local_socket::Listener> {
    // `to_fs_name` lets us pass an OS-native path (both the Windows
    // pipe name `\\.\pipe\…` and a Unix socket path are absolute file
    // names). `try_overwrite(true)` replaces a stale Unix socket file
    // left behind by a crashed previous instance — on Windows the
    // equivalent is a no-op because Named Pipes don't leak files.
    #[cfg(windows)]
    let name = {
        use interprocess::os::windows::local_socket::NamedPipe;
        endpoint.as_str().to_fs_name::<NamedPipe>()?
    };
    #[cfg(unix)]
    let name = {
        use interprocess::local_socket::GenericFilePath;
        endpoint.as_str().to_fs_name::<GenericFilePath>()?
    };

    let listener = ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()?;
    Ok(listener)
}

fn accept_loop(
    listener: interprocess::local_socket::Listener,
    command_tx: Sender<AppCommand>,
    session_token: String,
    endpoint_for_log: String,
    stop: Arc<AtomicBool>,
    event_bus: EventBus,
) {
    for conn in listener.incoming() {
        // The self-connect triggered by IpcServer::drop returns here;
        // observing the stop flag before handle_connection lets us
        // exit cleanly instead of serving one last spurious request.
        if stop.load(Ordering::Acquire) {
            return;
        }
        let conn = match conn {
            Ok(c) => c,
            Err(e) => {
                // Accept failures on a shutdown path are expected (the
                // listener got unlinked under us); on a normal path
                // they're transient and shouldn't kill the server.
                if stop.load(Ordering::Acquire) {
                    return;
                }
                eprintln!("ccmux IPC: accept failed on {endpoint_for_log}: {e}");
                continue;
            }
        };

        let tx = command_tx.clone();
        let token = session_token.clone();
        let bus = event_bus.clone();
        if let Err(e) = thread::Builder::new()
            .name("ccmux-ipc-worker".into())
            .spawn(move || {
                if let Err(e) = handle_connection(conn, tx, &token, bus) {
                    eprintln!("ccmux IPC: connection error: {e}");
                }
            })
        {
            // Thread spawn failures are extremely rare (EAGAIN under
            // system pressure). Dropping the connection is safe — the
            // client sees EOF and can retry. We deliberately don't fall
            // back to inline handling because that would block the
            // accept loop behind a slow request.
            eprintln!("ccmux IPC: worker spawn failed, dropping connection: {e}");
        }
    }
}

fn handle_connection(
    conn: Stream,
    command_tx: Sender<AppCommand>,
    session_token: &str,
    event_bus: EventBus,
) -> Result<()> {
    // The stream is split by wrapping in BufReader for line-buffered
    // reads; writes go through BufReader::get_mut. We can't construct
    // two BufReader clones without a split, so we borrow mutably.
    let mut reader = BufReader::new(conn);
    let mut line = String::new();

    // ── 1. Handshake ───────────────────────────────────────
    if read_line_or_eof(&mut reader, &mut line)?.is_none() {
        return Ok(());
    }
    let req: Request = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            return write_response_line(
                reader.get_mut(),
                &Response::err(format!("parse error on hello: {e}")),
            );
        }
    };
    match req {
        Request::Hello { client_pid: _ } => {
            let hello = Response::Hello {
                server_pid: std::process::id(),
                session_token: session_token.to_string(),
            };
            write_response_line(reader.get_mut(), &hello)?;
        }
        _ => {
            write_response_line(
                reader.get_mut(),
                &Response::err("first message must be hello"),
            )?;
            return Ok(());
        }
    }

    // ── 2. One command ─────────────────────────────────────
    line.clear();
    if read_line_or_eof(&mut reader, &mut line)?.is_none() {
        return Ok(());
    }
    let req: Request = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            return write_response_line(
                reader.get_mut(),
                &Response::err(format!("parse error: {e}")),
            );
        }
    };
    if matches!(req, Request::Subscribe) {
        // Switch this connection into event-stream mode: send the ack,
        // then push events until the client disconnects.
        write_response_line(reader.get_mut(), &Response::Subscribed)?;
        return stream_events(reader.into_inner(), event_bus);
    }
    let resp = dispatch_request(req, &command_tx);
    write_response_line(reader.get_mut(), &resp)?;
    Ok(())
}

/// Drain events from the bus into the wire until the connection dies
/// or the subscriber is unregistered. Called after the Subscribe ack
/// has been written.
fn stream_events(mut conn: Stream, event_bus: EventBus) -> Result<()> {
    let (sub_id, rx) = event_bus.subscribe();
    // The recv loop is bounded only by the connection's lifetime.
    // The client can stop the stream by closing the socket, which
    // makes the next write fail and we bail out.
    while let Ok(event) = rx.recv() {
        let mut json = match serde_json::to_string(&event) {
            Ok(s) => s,
            Err(_) => continue,
        };
        json.push('\n');
        if conn.write_all(json.as_bytes()).is_err() || conn.flush().is_err() {
            break;
        }
    }
    event_bus.unsubscribe(sub_id);
    Ok(())
}

fn read_line_or_eof<R: BufRead>(reader: &mut R, buf: &mut String) -> Result<Option<()>> {
    let n = reader.read_line(buf)?;
    Ok(if n == 0 { None } else { Some(()) })
}

fn write_response_line<W: Write>(w: &mut W, resp: &Response) -> Result<()> {
    let mut json = serde_json::to_string(resp)?;
    json.push('\n');
    w.write_all(json.as_bytes())?;
    w.flush()?;
    Ok(())
}

fn dispatch_request(req: Request, command_tx: &Sender<AppCommand>) -> Response {
    match req {
        Request::Hello { .. } => Response::err("unexpected duplicate hello"),
        Request::List => {
            let (reply_tx, reply_rx) = oneshot::channel();
            if command_tx
                .send(AppCommand::List { reply: reply_tx })
                .is_err()
            {
                return Response::err("app shutting down");
            }
            match reply_rx.recv_timeout(APP_REPLY_TIMEOUT) {
                Ok(list) => match serde_json::to_value(&list) {
                    Ok(v) => Response::ok_value(v),
                    Err(e) => Response::err(format!("serialize pane list: {e}")),
                },
                Err(e) => Response::err(format!("app did not respond: {e}")),
            }
        }
        Request::Send {
            target,
            data,
            append_enter,
        } => forward_unit(command_tx, |reply| AppCommand::Send {
            target,
            data: data.into_bytes(),
            append_enter,
            reply,
        }),
        Request::Focus { target } => {
            forward_unit(command_tx, |reply| AppCommand::Focus { target, reply })
        }
        Request::Split {
            target,
            direction,
            command,
            id,
            role,
        } => {
            let (reply_tx, reply_rx) = oneshot::channel();
            if command_tx
                .send(AppCommand::Split {
                    target,
                    direction,
                    command,
                    name: id,
                    role,
                    reply: reply_tx,
                })
                .is_err()
            {
                return Response::err("app shutting down");
            }
            match reply_rx.recv_timeout(APP_REPLY_TIMEOUT) {
                Ok(Ok(new_id)) => Response::ok_value(serde_json::json!({ "id": new_id })),
                Ok(Err(msg)) => Response::err(msg),
                Err(e) => Response::err(format!("app did not respond: {e}")),
            }
        }
        Request::NewTab {
            command,
            id,
            label,
            role,
        } => {
            let (reply_tx, reply_rx) = oneshot::channel();
            if command_tx
                .send(AppCommand::NewTab {
                    command,
                    name: id,
                    label,
                    role,
                    reply: reply_tx,
                })
                .is_err()
            {
                return Response::err("app shutting down");
            }
            match reply_rx.recv_timeout(APP_REPLY_TIMEOUT) {
                Ok(Ok(new_id)) => Response::ok_value(serde_json::json!({ "id": new_id })),
                Ok(Err(msg)) => Response::err(msg),
                Err(e) => Response::err(format!("app did not respond: {e}")),
            }
        }
        // Subscribe is handled by the connection handler directly — it
        // switches the wire into event-stream mode rather than
        // round-tripping through App commands. If we see it here, the
        // handler called us by mistake; refuse rather than hang.
        Request::Subscribe => Response::err("subscribe should be handled inline"),
    }
}

/// Forward a command whose success result is `()` and translate the
/// reply into a [`Response`]. Factored out because three of the four
/// variants share this exact shape.
fn forward_unit(
    command_tx: &Sender<AppCommand>,
    build: impl FnOnce(oneshot::Sender<std::result::Result<(), String>>) -> AppCommand,
) -> Response {
    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx.send(build(reply_tx)).is_err() {
        return Response::err("app shutting down");
    }
    match reply_rx.recv_timeout(APP_REPLY_TIMEOUT) {
        Ok(Ok(_)) => Response::ok_unit(),
        Ok(Err(msg)) => Response::err(msg),
        Err(e) => Response::err(format!("app did not respond: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{Direction, PaneRef, Request};
    use std::sync::mpsc;

    #[test]
    fn dispatch_list_ok_when_app_replies() {
        // Pretend to be the App: spawn a thread that pulls a List
        // command off the channel and replies with an empty list.
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::List { reply }) = rx.recv() {
                reply.send(Vec::new()).unwrap();
            }
        });

        let resp = dispatch_request(Request::List, &tx);
        handle.join().unwrap();

        match resp {
            Response::Ok { data } => {
                // An empty Vec<PaneInfo> serializes to a JSON array.
                assert!(data.is_array(), "expected array, got {data:?}");
                assert_eq!(data.as_array().map(|a| a.len()), Some(0));
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_focus_ok_when_app_replies_ok() {
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::Focus { reply, .. }) = rx.recv() {
                reply.send(Ok(())).unwrap();
            }
        });
        let resp = dispatch_request(
            Request::Focus {
                target: PaneRef::Focused,
            },
            &tx,
        );
        handle.join().unwrap();
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[test]
    fn dispatch_focus_err_when_app_replies_err() {
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::Focus { reply, .. }) = rx.recv() {
                reply.send(Err("pane not found".into())).unwrap();
            }
        });
        let resp = dispatch_request(
            Request::Focus {
                target: PaneRef::Id(999),
            },
            &tx,
        );
        handle.join().unwrap();
        match resp {
            Response::Err { message } => assert!(message.contains("pane not found")),
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_split_returns_new_id() {
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::Split { reply, .. }) = rx.recv() {
                reply.send(Ok(42)).unwrap();
            }
        });
        let resp = dispatch_request(
            Request::Split {
                target: PaneRef::Focused,
                direction: Direction::Vertical,
                command: None,
                id: None,
                role: None,
            },
            &tx,
        );
        handle.join().unwrap();
        match resp {
            Response::Ok { data } => {
                assert_eq!(data.get("id").and_then(|v| v.as_u64()), Some(42));
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_send_forwards_data_and_enter() {
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::Send {
                data,
                append_enter,
                reply,
                ..
            }) = rx.recv()
            {
                assert_eq!(data, b"hello");
                assert!(append_enter);
                reply.send(Ok(())).unwrap();
            }
        });
        let resp = dispatch_request(
            Request::Send {
                target: PaneRef::Name("engineering".into()),
                data: "hello".into(),
                append_enter: true,
            },
            &tx,
        );
        handle.join().unwrap();
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[test]
    fn dispatch_new_tab_returns_new_id() {
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::NewTab { reply, .. }) = rx.recv() {
                reply.send(Ok(11)).unwrap();
            }
        });
        let resp = dispatch_request(
            Request::NewTab {
                command: Some("cce".into()),
                id: Some("engineering".into()),
                label: None,
                role: None,
            },
            &tx,
        );
        handle.join().unwrap();
        match resp {
            Response::Ok { data } => {
                assert_eq!(data.get("id").and_then(|v| v.as_u64()), Some(11));
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_refuses_second_hello() {
        // Duplicate hello after handshake should be an error path.
        let (tx, _rx) = mpsc::channel::<AppCommand>();
        let resp = dispatch_request(Request::Hello { client_pid: 1 }, &tx);
        match resp {
            Response::Err { message } => assert!(message.contains("hello")),
            other => panic!("expected Err, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_split_forwards_role() {
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::Split { role, reply, .. }) = rx.recv() {
                assert_eq!(role.as_deref(), Some("worker"));
                reply.send(Ok(7)).unwrap();
            }
        });
        let resp = dispatch_request(
            Request::Split {
                target: PaneRef::Focused,
                direction: Direction::Vertical,
                command: None,
                id: None,
                role: Some("worker".into()),
            },
            &tx,
        );
        handle.join().unwrap();
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[test]
    fn dispatch_new_tab_forwards_role() {
        let (tx, rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            if let Ok(AppCommand::NewTab { role, reply, .. }) = rx.recv() {
                assert_eq!(role.as_deref(), Some("leader"));
                reply.send(Ok(9)).unwrap();
            }
        });
        let resp = dispatch_request(
            Request::NewTab {
                command: None,
                id: None,
                label: None,
                role: Some("leader".into()),
            },
            &tx,
        );
        handle.join().unwrap();
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn drop_removes_unix_socket_file() {
        use std::path::PathBuf;

        // Bind on a unique temp path so the test doesn't race with a
        // real ccmux instance or other tests.
        let dir = std::env::temp_dir().join(format!(
            "ccmux-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let sock_path: PathBuf = dir.join("ccmux-test.sock");
        let endpoint = EndpointName::socket(sock_path.clone());

        let (tx, _rx) = mpsc::channel::<AppCommand>();
        let server = IpcServer::spawn(endpoint, tx, "test-token".into()).unwrap();

        // Socket file should exist after binding.
        assert!(sock_path.exists(), "socket file not created");

        // Dropping IpcServer should remove it.
        drop(server);
        assert!(!sock_path.exists(), "socket file not removed on drop");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
