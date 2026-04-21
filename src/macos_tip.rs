//! First-launch tip that steers macOS users to the Option-as-Meta
//! terminal setup (see README § "macOS: Option as Meta"). macOS
//! terminals bind `Option+<key>` to Unicode input by default so
//! `Alt+T` / `Alt+P` / `Alt+1..9` / `Alt+Left/Right` never reach
//! ccmux. Users who don't know the fix read it as a ccmux bug.
//!
//! The tip surfaces as a transient banner on first launch and is
//! dismissed either by any key press or a 20 s timeout. Dismissal
//! writes a zero-byte marker file alongside `config.toml` so the
//! banner never appears again on that host. Users who want it back
//! can `rm` the marker file.
//!
//! No `state.toml` / serde layer on purpose: persisting a single
//! bool does not justify dragging in a new file format or the
//! round-trip cost. If more state ever needs to live here, revisit.

use std::fs;
use std::path::{Path, PathBuf};

/// Env-var twin of `--no-macos-tip`. Any non-empty value suppresses
/// the banner without touching the marker file, so scripted sessions
/// that don't want the banner don't accidentally mark a real macOS
/// user "dismissed" on their behalf.
pub const ENV_SUPPRESS: &str = "CCMUX_NO_MACOS_TIP";

/// How long the banner stays up before self-dismissing.
pub const AUTO_DISMISS: std::time::Duration = std::time::Duration::from_secs(20);

/// Resolve the marker-file path alongside `config.toml`. Returns
/// `None` on environments where the platform config dir can't be
/// determined (sandbox without `$HOME`); the caller treats that as
/// "can't persist dismissal, show in-memory for this run only".
pub fn marker_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join("ccmux").join(".macos_tip_dismissed"))
}

pub fn is_dismissed(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

/// Touch the marker file (zero-byte placeholder), creating the
/// parent directory if needed. Failures go to stderr rather than
/// surfacing as an error so a transient IO issue on dismiss doesn't
/// disrupt the run.
pub fn mark_dismissed(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!(
                "ccmux: couldn't create {} for macOS tip marker: {e}",
                parent.display()
            );
            return;
        }
    }
    if let Err(e) = fs::File::create(path) {
        eprintln!(
            "ccmux: couldn't touch macOS tip marker {}: {e}",
            path.display()
        );
    }
}

/// Decide whether this launch should surface the banner. Override
/// precedence (highest first):
///
/// 1. `--show-macos-tip` — force on (still macOS-gated; showing a
///    macOS-specific setup tip on Linux makes no sense).
/// 2. `--no-macos-tip` — force off.
/// 3. `CCMUX_NO_MACOS_TIP` env var set and non-empty — force off.
/// 4. Marker file exists — off.
/// 5. Non-macOS host — off.
/// 6. Otherwise — on.
pub fn should_show(cli_no_tip: bool, cli_show_tip: bool, marker: Option<&Path>) -> bool {
    if cli_show_tip {
        return cfg!(target_os = "macos");
    }
    if cli_no_tip {
        return false;
    }
    if std::env::var_os(ENV_SUPPRESS).is_some_and(|v| !v.is_empty()) {
        return false;
    }
    if !cfg!(target_os = "macos") {
        return false;
    }
    !matches!(marker, Some(p) if is_dismissed(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    // Tests that read or mutate CCMUX_NO_MACOS_TIP hold this lock so
    // they don't race each other. Cargo runs tests in parallel by
    // default, and env mutations are process-global.
    fn env_lock() -> &'static Mutex<()> {
        static L: OnceLock<Mutex<()>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(()))
    }

    fn unique_marker() -> PathBuf {
        let pid = std::process::id();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("ccmux-macos-tip-test-{pid}-{n}"))
    }

    struct EnvGuard {
        prev: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn clear() -> Self {
            let prev = std::env::var_os(ENV_SUPPRESS);
            std::env::remove_var(ENV_SUPPRESS);
            Self { prev }
        }

        fn set(value: &str) -> Self {
            let prev = std::env::var_os(ENV_SUPPRESS);
            std::env::set_var(ENV_SUPPRESS, value);
            Self { prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(v) => std::env::set_var(ENV_SUPPRESS, v),
                None => std::env::remove_var(ENV_SUPPRESS),
            }
        }
    }

    #[test]
    fn mark_then_dismissed() {
        let p = unique_marker();
        let _ = fs::remove_file(&p);
        assert!(!is_dismissed(&p));
        mark_dismissed(&p);
        assert!(is_dismissed(&p));
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn cli_no_tip_wins() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::clear();
        let p = unique_marker();
        let _ = fs::remove_file(&p);
        assert!(!should_show(true, false, Some(&p)));
    }

    #[test]
    fn cli_show_tip_overrides_marker_on_macos() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::clear();
        let p = unique_marker();
        mark_dismissed(&p);
        assert_eq!(
            should_show(false, true, Some(&p)),
            cfg!(target_os = "macos")
        );
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn default_follows_platform_when_no_marker() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::clear();
        let p = unique_marker();
        let _ = fs::remove_file(&p);
        assert_eq!(
            should_show(false, false, Some(&p)),
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn env_var_suppresses() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::set("1");
        let p = unique_marker();
        let _ = fs::remove_file(&p);
        assert!(!should_show(false, false, Some(&p)));
    }

    #[test]
    fn marker_present_suppresses_on_macos() {
        let _lock = env_lock().lock().unwrap();
        let _env = EnvGuard::clear();
        let p = unique_marker();
        mark_dismissed(&p);
        assert!(!should_show(false, false, Some(&p)));
        let _ = fs::remove_file(&p);
    }
}
