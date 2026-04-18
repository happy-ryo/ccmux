//! IPC client: opens a short-lived connection to the running ccmux
//! instance, performs the [`Request::Hello`] handshake, then sends
//! exactly one [`Request`] and reads exactly one [`Response`].
//!
//! Connection lifecycle matches the server in [`super::server`]: one
//! request per connection, closed by the client dropping the stream.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc;
use std::thread;

use anyhow::{anyhow, Context, Result};
use interprocess::local_socket::{prelude::*, Stream};
use subtle::ConstantTimeEq;

use super::endpoint::{EndpointName, ENV_TOKEN};
use super::{Request, Response, RESPONSE_TIMEOUT};

/// Send a single request to the endpoint and return the response.
///
/// The blocking read on `interprocess::Stream` has no portable timeout
/// API in 2.x, so we run `converse` on a helper thread and wait on a
/// channel with [`RESPONSE_TIMEOUT`]. If the server deadlocks, the main
/// thread unblocks and returns an error instead of hanging forever —
/// the helper thread is detached and cleaned up by the OS when the
/// client process exits.
pub fn send_request(endpoint: &EndpointName, request: &Request) -> Result<Response> {
    let name_string = endpoint.as_str().to_string();
    let endpoint_clone = endpoint.clone();
    let request_clone = request.clone();
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("ccmux-ipc-client".into())
        .spawn(move || {
            let result = (|| -> Result<Response> {
                let name = make_connection_name(&endpoint_clone)?;
                let conn = Stream::connect(name)
                    .with_context(|| format!("connect to {}", endpoint_clone.as_str()))?;
                converse(conn, &request_clone)
            })();
            let _ = tx.send(result);
        })
        .context("spawn IPC client thread")?;

    match rx.recv_timeout(RESPONSE_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow!(
            "no response from ccmux within {:?} (endpoint: {})",
            RESPONSE_TIMEOUT,
            name_string
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!("IPC client thread panicked")),
    }
}

fn make_connection_name(endpoint: &EndpointName) -> Result<interprocess::local_socket::Name<'_>> {
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

fn converse(conn: Stream, request: &Request) -> Result<Response> {
    let mut reader = BufReader::new(conn);

    // Handshake
    let hello = Request::Hello {
        client_pid: std::process::id(),
    };
    write_request_line(reader.get_mut(), &hello)?;
    let hello_resp = read_response_line(&mut reader)?;
    match hello_resp {
        Response::Hello { session_token, .. } => {
            verify_session_token(&session_token, std::env::var(ENV_TOKEN).ok().as_deref())?;
        }
        Response::Err { message } => {
            return Err(anyhow!("server refused hello: {message}"));
        }
        Response::Ok { .. } => {
            return Err(anyhow!("unexpected ok response to hello"));
        }
    }

    // Actual command
    write_request_line(reader.get_mut(), request)?;
    let resp = read_response_line(&mut reader)?;
    Ok(resp)
}

fn write_request_line<W: Write>(w: &mut W, req: &Request) -> Result<()> {
    let mut json = serde_json::to_string(req)?;
    json.push('\n');
    w.write_all(json.as_bytes())?;
    w.flush()?;
    Ok(())
}

/// Compare the server-provided session token with the expected one
/// that the parent ccmux published to `CCMUX_TOKEN`.
///
/// A mismatch means the `CCMUX_SOCKET` path we connected through points
/// to a ccmux instance that doesn't own the current shell — most likely
/// the PID got re-used and a stale socket path was inherited. Refuse
/// rather than silently deliver the command to the wrong process.
///
/// Uses a constant-time comparison; same-user tokens are not a secrecy
/// boundary (see the crate-level threat model), but comparing byte-by-
/// byte in constant time is the cheap hardening default.
fn verify_session_token(server_token: &str, expected: Option<&str>) -> Result<()> {
    match expected {
        Some(e) => {
            let a = server_token.as_bytes();
            let b = e.as_bytes();
            if a.len() == b.len() && bool::from(a.ct_eq(b)) {
                Ok(())
            } else {
                Err(anyhow!(
                    "session token mismatch; {ENV_SOCKET} likely points to a different ccmux instance",
                    ENV_SOCKET = super::endpoint::ENV_SOCKET
                ))
            }
        }
        None => Err(anyhow!(
            "{ENV_TOKEN} not set; are you running inside ccmux?"
        )),
    }
}

fn read_response_line<R: BufRead>(r: &mut R) -> Result<Response> {
    let mut buf = String::new();
    let n = r.read_line(&mut buf)?;
    if n == 0 {
        return Err(anyhow!("server closed connection before replying"));
    }
    let resp: Response = serde_json::from_str(buf.trim())
        .with_context(|| format!("parse response json: {buf:?}"))?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{Direction, PaneRef};

    #[test]
    fn write_request_line_is_newline_terminated() {
        let mut out: Vec<u8> = Vec::new();
        let req = Request::List;
        write_request_line(&mut out, &req).unwrap();
        assert!(out.ends_with(b"\n"));
        // The line without the trailing newline must parse back to the
        // original request — protects against accidentally emitting
        // multi-line JSON.
        let line = std::str::from_utf8(&out).unwrap().trim_end();
        let parsed: Request = serde_json::from_str(line).unwrap();
        assert_eq!(parsed, Request::List);
    }

    #[test]
    fn read_response_line_parses_ok() {
        let input: &[u8] = b"{\"status\":\"ok\",\"data\":null}\n";
        let mut reader = std::io::BufReader::new(input);
        let resp = read_response_line(&mut reader).unwrap();
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[test]
    fn verify_session_token_matches() {
        assert!(verify_session_token("abc-123", Some("abc-123")).is_ok());
    }

    #[test]
    fn verify_session_token_rejects_mismatch() {
        let err = verify_session_token("abc-123", Some("xyz-999")).unwrap_err();
        assert!(err.to_string().contains("mismatch"), "got: {err}");
    }

    #[test]
    fn verify_session_token_rejects_missing_env() {
        let err = verify_session_token("abc-123", None).unwrap_err();
        assert!(err.to_string().contains("CCMUX_TOKEN"), "got: {err}");
    }

    #[test]
    fn verify_session_token_rejects_length_mismatch() {
        let err = verify_session_token("short", Some("much-longer-token")).unwrap_err();
        assert!(err.to_string().contains("mismatch"), "got: {err}");
    }

    #[test]
    fn verify_session_token_rejects_whitespace_wrap() {
        // Whitespace is not trimmed; comparison is exact.
        assert!(verify_session_token("abc", Some(" abc")).is_err());
        assert!(verify_session_token("abc", Some("abc\n")).is_err());
    }

    #[test]
    fn verify_session_token_unicode_roundtrip() {
        assert!(verify_session_token("トークン", Some("トークン")).is_ok());
        assert!(verify_session_token("トークン", Some("トーケン")).is_err());
    }

    #[test]
    fn verify_session_token_rejects_empty_server_token() {
        assert!(verify_session_token("", Some("nonempty")).is_err());
    }

    #[test]
    fn read_response_line_eof_is_error() {
        let input: &[u8] = b"";
        let mut reader = std::io::BufReader::new(input);
        assert!(read_response_line(&mut reader).is_err());
    }

    #[test]
    fn write_request_line_roundtrips_split() {
        let req = Request::Split {
            target: PaneRef::Focused,
            direction: Direction::Horizontal,
            command: Some("echo".into()),
            id: Some("foo".into()),
        };
        let mut out: Vec<u8> = Vec::new();
        write_request_line(&mut out, &req).unwrap();
        let line = std::str::from_utf8(&out).unwrap().trim_end();
        let parsed: Request = serde_json::from_str(line).unwrap();
        assert_eq!(parsed, req);
    }
}
