//! Cross-platform IPC endpoint naming.
//!
//! Endpoints are scoped per running ccmux instance using its PID, so
//! multiple ccmux processes on one machine don't collide. The endpoint
//! name is also published to child PTYs via the `CCMUX_SOCKET`
//! environment variable so in-pane clients (the secretary, etc.) can
//! find the right server.

#[cfg(unix)]
use anyhow::Context;
use anyhow::{anyhow, Result};
#[cfg(unix)]
use std::path::PathBuf;

pub const ENV_SOCKET: &str = "CCMUX_SOCKET";
pub const ENV_TOKEN: &str = "CCMUX_TOKEN";

/// Compute the IPC endpoint name for a ccmux instance with the given PID.
///
/// On Windows this is a Named Pipe path (`\\.\pipe\ccmux-<pid>`).
/// On Unix this is a filesystem path under `$XDG_RUNTIME_DIR/ccmux/`,
/// falling back to `$TMPDIR/ccmux-<uid>/` and then `/tmp/ccmux-<uid>/`.
/// The uid is the **real** OS uid of the current process (via
/// `libc::getuid`), never the `$UID` environment variable. `$UID` is a
/// bash-internal shell variable that may not be exported in minimal
/// init systems or non-bash login shells; if we trusted it we'd
/// happily collide with another user's directory or fall back to a
/// hardcoded `1000`.
pub fn endpoint_for_pid(pid: u32) -> Result<EndpointName> {
    #[cfg(windows)]
    {
        Ok(EndpointName::pipe(format!(r"\\.\pipe\ccmux-{pid}")))
    }
    #[cfg(unix)]
    {
        let dir = unix_socket_dir()?;
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        // Restrict directory to 0o700. Owner-only is the *only* access
        // control that protects the endpoint from other local users;
        // if we can't establish it the trust model is broken, so fail
        // closed rather than continue with potentially world-reachable
        // permissions.
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o700);
            std::fs::set_permissions(&dir, perms)
                .with_context(|| format!("failed to restrict {} to owner-only", dir.display()))?;
        }
        let path = dir.join(format!("ccmux-{pid}.sock"));
        Ok(EndpointName::socket(path))
    }
}

#[cfg(unix)]
fn unix_socket_dir() -> Result<PathBuf> {
    // XDG_RUNTIME_DIR (set by systemd-logind and many others) is the
    // canonical location. The OS already guarantees it's owner-only
    // and cleaned up on logout, so we just nest `ccmux/` under it.
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        if !d.is_empty() {
            return Ok(PathBuf::from(d).join("ccmux"));
        }
    }
    // Fall back to $TMPDIR / /tmp, in which case we have to include
    // the uid so separate users don't collide.
    let uid = unix_real_uid();
    if let Ok(d) = std::env::var("TMPDIR") {
        if !d.is_empty() {
            return Ok(PathBuf::from(d).join(format!("ccmux-{uid}")));
        }
    }
    Ok(PathBuf::from("/tmp").join(format!("ccmux-{uid}")))
}

/// Real OS uid of the current process.
///
/// Uses `libc::getuid` directly. This is a plain syscall with no
/// arguments, no failure mode, and no memory safety concerns, so the
/// `unsafe` block is purely ceremony — wrapping it in `nix` or
/// `rustix` would add a dependency for no practical gain.
#[cfg(unix)]
fn unix_real_uid() -> u32 {
    // SAFETY: `getuid` is defined on every supported Unix, takes no
    // arguments, has no error path, and returns a simple uid_t.
    unsafe { libc::getuid() as u32 }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndpointName {
    repr: String,
    kind: EndpointKind,
}

/// Both variants exist on every platform so `kind()` / pattern matches
/// compile everywhere, but only one is actually constructed per target
/// (Pipe on Windows, Socket on Unix). Allow dead_code so the idle side
/// doesn't fail clippy.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum EndpointKind {
    Pipe,
    Socket,
}

impl EndpointName {
    #[cfg(windows)]
    pub fn pipe(name: impl Into<String>) -> Self {
        Self {
            repr: name.into(),
            kind: EndpointKind::Pipe,
        }
    }
    #[cfg(unix)]
    pub fn socket(path: PathBuf) -> Self {
        Self {
            repr: path.display().to_string(),
            kind: EndpointKind::Socket,
        }
    }
    pub fn as_str(&self) -> &str {
        &self.repr
    }
    pub fn kind(&self) -> EndpointKind {
        self.kind
    }
}

/// Look up the endpoint a child process should connect to. Returns an
/// error if the parent ccmux did not publish `CCMUX_SOCKET`.
pub fn endpoint_from_env() -> Result<EndpointName> {
    let s = std::env::var(ENV_SOCKET)
        .map_err(|_| anyhow!("{ENV_SOCKET} not set; are you running inside ccmux?"))?;
    if s.is_empty() {
        return Err(anyhow!("{ENV_SOCKET} is empty"));
    }
    #[cfg(windows)]
    {
        Ok(EndpointName::pipe(s))
    }
    #[cfg(unix)]
    {
        Ok(EndpointName::socket(PathBuf::from(s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests in this module mutate process-global environment variables
    // (`UID`, `XDG_RUNTIME_DIR`, `TMPDIR`, `CCMUX_SOCKET`). `cargo test`
    // runs test functions on multiple threads inside a single process,
    // which makes concurrent env reads/writes racy. Guard every env-
    // touching test with this Mutex so they serialize among themselves
    // and — implicitly via the lock — against each other's restore
    // paths. Tests that don't touch env vars don't need the lock.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn endpoint_for_pid_includes_pid() {
        let ep = endpoint_for_pid(12345).unwrap();
        assert!(ep.as_str().contains("12345"), "{}", ep.as_str());
    }

    #[cfg(windows)]
    #[test]
    fn windows_endpoint_uses_pipe_prefix() {
        let ep = endpoint_for_pid(1).unwrap();
        assert!(ep.as_str().starts_with(r"\\.\pipe\"), "{}", ep.as_str());
        assert_eq!(ep.kind(), EndpointKind::Pipe);
    }

    #[cfg(unix)]
    #[test]
    fn unix_endpoint_is_socket_path() {
        let ep = endpoint_for_pid(1).unwrap();
        assert!(ep.as_str().ends_with(".sock"), "{}", ep.as_str());
        assert_eq!(ep.kind(), EndpointKind::Socket);
    }

    #[cfg(unix)]
    #[test]
    fn unix_real_uid_is_independent_of_env() {
        // If a hostile / unusual shell exports UID=1337, the socket
        // directory must still be named after the *OS* uid — that's
        // the whole point of #58.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let before = unix_real_uid();
        let prev = std::env::var("UID").ok();
        std::env::set_var("UID", "1337");
        let after = unix_real_uid();
        match prev {
            Some(v) => std::env::set_var("UID", v),
            None => std::env::remove_var("UID"),
        }
        assert_eq!(before, after, "getuid() must not read $UID");
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_dir_fallback_uses_real_uid_not_env() {
        // Force the /tmp fallback by clearing both XDG_RUNTIME_DIR
        // and TMPDIR, then confirm the resulting path contains the
        // OS uid even if $UID is lying.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_xdg = std::env::var("XDG_RUNTIME_DIR").ok();
        let prev_tmp = std::env::var("TMPDIR").ok();
        let prev_uid = std::env::var("UID").ok();
        std::env::remove_var("XDG_RUNTIME_DIR");
        std::env::remove_var("TMPDIR");
        std::env::set_var("UID", "9999");

        let dir = unix_socket_dir().unwrap();
        let real = unix_real_uid();

        // restore before asserting so a failure doesn't poison the
        // test harness.
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
        match prev_tmp {
            Some(v) => std::env::set_var("TMPDIR", v),
            None => std::env::remove_var("TMPDIR"),
        }
        match prev_uid {
            Some(v) => std::env::set_var("UID", v),
            None => std::env::remove_var("UID"),
        }

        let expected_suffix = format!("ccmux-{real}");
        assert!(
            dir.display().to_string().ends_with(&expected_suffix),
            "{} should end with {}",
            dir.display(),
            expected_suffix
        );
    }

    #[test]
    fn endpoint_from_env_fails_without_var() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var(ENV_SOCKET).ok();
        std::env::remove_var(ENV_SOCKET);
        let result = endpoint_from_env();
        if let Some(v) = prev {
            std::env::set_var(ENV_SOCKET, v);
        }
        assert!(result.is_err());
    }
}
