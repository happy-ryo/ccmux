//! User-level configuration loaded from
//! `${XDG_CONFIG_HOME or ~/.config}/ccmux/config.toml` on Unix, or
//! `%APPDATA%/ccmux/config.toml` on Windows.
//!
//! Precedence: CLI flags override the config file, which overrides
//! built-in defaults. Missing or malformed files never fail startup —
//! a warning goes to stderr and defaults apply.

use serde::Deserialize;
use std::fmt;
use std::path::PathBuf;

/// Top-level config schema. Extra TOML keys are ignored so we can add
/// new sections in future releases without breaking older binaries
/// reading a newer user config.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ime: ImeConfig,
}

/// IME overlay settings. See Issue #39 for the full mode design;
/// `always_on` lives behind Issue #40 and is explicitly not accepted
/// here yet.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ImeConfig {
    pub mode: ImeMode,
    /// Main-loop `event::poll` timeout (ms) while the IME composition
    /// overlay is open (Issue #38). Higher values throttle redraws
    /// during composition at the cost of a slightly less responsive
    /// cancel/commit path on very slow typists. Input events (key /
    /// paste / mouse / resize) still interrupt the poll immediately,
    /// so only the idle redraw rate is affected. Clamped to
    /// `MIN_OVERLAY_POLL_MS` at apply time to avoid degenerate near-0
    /// timeouts. Default is 166 ms — preliminary, to be finalized by
    /// the PoC benchmark described in Issue #82.
    pub overlay_poll_ms: u64,
}

/// Preliminary default overlay `event::poll` timeout. See
/// [`ImeConfig::overlay_poll_ms`].
pub const DEFAULT_OVERLAY_POLL_MS: u64 = 166;

/// Floor for `overlay_poll_ms`. Values below this (including 0) are
/// clamped up so the main loop never spins on a 0-ms poll.
pub const MIN_OVERLAY_POLL_MS: u64 = 10;

impl Default for ImeConfig {
    fn default() -> Self {
        Self {
            mode: ImeMode::default(),
            overlay_poll_ms: DEFAULT_OVERLAY_POLL_MS,
        }
    }
}

/// How Ctrl+; behaves in a focused pane.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum ImeMode {
    /// Ctrl+; opens the IME composition overlay. Default.
    #[default]
    Hotkey,
    /// Ctrl+; is swallowed — the overlay is never opened and no
    /// keystroke is forwarded to the pane either, because terminals
    /// encode Ctrl+punctuation inconsistently and the bare `;` that
    /// would otherwise leak through isn't what the user asked for.
    /// For users who don't use IME or prefer their terminal's own
    /// IME handling.
    Off,
    /// The overlay is opened automatically whenever focus rests on
    /// a non-scrolled Claude pane, so IME (including JP) has an
    /// anchor from the first keystroke. Esc/Ctrl+C on an empty
    /// buffer dismisses and forwards the cancel key to the pane;
    /// dismissal clears when focus moves to another pane and back.
    /// A printable keystroke still auto-opens on a dismissed pane
    /// as a half-width convenience. See Issue #40 for full
    /// semantics.
    Always,
}

impl fmt::Display for ImeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImeMode::Hotkey => f.write_str("hotkey"),
            ImeMode::Off => f.write_str("off"),
            ImeMode::Always => f.write_str("always"),
        }
    }
}

impl std::str::FromStr for ImeMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hotkey" => Ok(ImeMode::Hotkey),
            "off" => Ok(ImeMode::Off),
            "always" => Ok(ImeMode::Always),
            other => Err(format!(
                "invalid ime mode: {other:?} (expected hotkey | off | always)"
            )),
        }
    }
}

/// Upper bound on config file size. A real user config is at most a
/// few hundred bytes; anything larger is either a mistake (wrong
/// file ended up at this path) or adversarial, and we don't want to
/// read it into memory.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

impl Config {
    /// Load the config file if present. Missing file returns
    /// defaults; malformed TOML returns defaults and prints a
    /// warning. The return value is always a usable Config so
    /// callers never have to decide what to do on I/O errors —
    /// `ccmux` must keep starting.
    pub fn load() -> Self {
        let path = match config_path() {
            Some(p) => p,
            None => return Self::default(),
        };
        Self::load_from(&path)
    }

    /// `load()` specialized to a caller-provided path, so tests can
    /// point at a temp file without touching `dirs::config_dir()`.
    pub(crate) fn load_from(path: &std::path::Path) -> Self {
        match std::fs::metadata(path) {
            Ok(meta) if meta.len() > MAX_CONFIG_BYTES => {
                eprintln!(
                    "ccmux: config {} exceeds {MAX_CONFIG_BYTES} bytes; using defaults",
                    path.display()
                );
                return Self::default();
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!("ccmux: config {} stat failed: {e}", path.display());
                return Self::default();
            }
        }
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!("ccmux: config {} unreadable: {e}", path.display());
                return Self::default();
            }
        };
        match toml::from_str::<Config>(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "ccmux: config {} has invalid TOML: {e}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Apply optional CLI overrides on top of the loaded config.
    /// `None` leaves the field untouched, mirroring the precedence
    /// "CLI > file > default". `overlay_poll_ms` is clamped to
    /// [`MIN_OVERLAY_POLL_MS`] whether it arrived via CLI, file, or
    /// default, so the main loop never sees a sub-floor value.
    pub fn apply_cli_overrides(&mut self, ime_mode: Option<ImeMode>, overlay_poll_ms: Option<u64>) {
        if let Some(mode) = ime_mode {
            self.ime.mode = mode;
        }
        if let Some(ms) = overlay_poll_ms {
            self.ime.overlay_poll_ms = ms;
        }
        if self.ime.overlay_poll_ms < MIN_OVERLAY_POLL_MS {
            self.ime.overlay_poll_ms = MIN_OVERLAY_POLL_MS;
        }
    }
}

/// Resolve the platform-appropriate config file path. Returns `None`
/// on environments where the base directory can't be determined
/// (e.g. a stripped-down sandbox with no `$HOME`); callers fall
/// through to defaults.
fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join("ccmux").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_hotkey_mode() {
        let cfg = Config::default();
        assert_eq!(cfg.ime.mode, ImeMode::Hotkey);
    }

    #[test]
    fn parses_minimal_ime_off() {
        let cfg: Config = toml::from_str(
            r#"
            [ime]
            mode = "off"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.ime.mode, ImeMode::Off);
    }

    #[test]
    fn parses_minimal_ime_hotkey() {
        let cfg: Config = toml::from_str(
            r#"
            [ime]
            mode = "hotkey"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.ime.mode, ImeMode::Hotkey);
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.ime.mode, ImeMode::Hotkey);
    }

    #[test]
    fn unknown_sections_are_ignored_for_forward_compat() {
        // A newer ccmux might add `[telemetry]`; the older binary
        // must continue to boot instead of erroring out.
        let cfg: Config = toml::from_str(
            r#"
            [telemetry]
            enabled = true
            [ime]
            mode = "off"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.ime.mode, ImeMode::Off);
    }

    #[test]
    fn parses_minimal_ime_always() {
        let cfg: Config = toml::from_str(
            r#"
            [ime]
            mode = "always"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.ime.mode, ImeMode::Always);
    }

    #[test]
    fn rejects_unknown_ime_mode_value() {
        let err = toml::from_str::<Config>(
            r#"
            [ime]
            mode = "banana"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("banana") || err.to_string().contains("variant"));
    }

    #[test]
    fn cli_override_beats_file() {
        let mut cfg: Config = toml::from_str(
            r#"
            [ime]
            mode = "hotkey"
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(Some(ImeMode::Off), None);
        assert_eq!(cfg.ime.mode, ImeMode::Off);
    }

    #[test]
    fn cli_none_leaves_file_value() {
        let mut cfg: Config = toml::from_str(
            r#"
            [ime]
            mode = "off"
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(None, None);
        assert_eq!(cfg.ime.mode, ImeMode::Off);
    }

    #[test]
    fn load_from_missing_file_returns_default() {
        let tmp = std::env::temp_dir().join(format!("ccmux-missing-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let cfg = Config::load_from(&tmp);
        assert_eq!(cfg.ime.mode, ImeMode::Hotkey);
    }

    #[test]
    fn load_from_valid_file_returns_parsed() {
        let tmp = std::env::temp_dir().join(format!("ccmux-valid-{}.toml", std::process::id()));
        std::fs::write(&tmp, "[ime]\nmode = \"off\"\n").unwrap();
        let cfg = Config::load_from(&tmp);
        std::fs::remove_file(&tmp).ok();
        assert_eq!(cfg.ime.mode, ImeMode::Off);
    }

    #[test]
    fn load_from_malformed_file_returns_default_and_does_not_panic() {
        let tmp = std::env::temp_dir().join(format!("ccmux-bad-{}.toml", std::process::id()));
        std::fs::write(&tmp, "this is = not { valid toml").unwrap();
        let cfg = Config::load_from(&tmp);
        std::fs::remove_file(&tmp).ok();
        assert_eq!(cfg.ime.mode, ImeMode::Hotkey);
    }

    #[test]
    fn load_from_oversized_file_returns_default() {
        let tmp = std::env::temp_dir().join(format!("ccmux-big-{}.toml", std::process::id()));
        // Write ~128 KB — above the 64 KB cap — of valid-looking TOML.
        // The cap should short-circuit before parsing.
        let big = format!("# {}\n[ime]\nmode = \"off\"\n", "x".repeat(130_000));
        std::fs::write(&tmp, &big).unwrap();
        let cfg = Config::load_from(&tmp);
        std::fs::remove_file(&tmp).ok();
        assert_eq!(cfg.ime.mode, ImeMode::Hotkey);
    }

    #[test]
    fn default_overlay_poll_ms_is_preliminary_166() {
        let cfg = Config::default();
        assert_eq!(cfg.ime.overlay_poll_ms, DEFAULT_OVERLAY_POLL_MS);
        assert_eq!(DEFAULT_OVERLAY_POLL_MS, 166);
    }

    #[test]
    fn parses_overlay_poll_ms_from_toml() {
        let cfg: Config = toml::from_str(
            r#"
            [ime]
            mode = "hotkey"
            overlay_poll_ms = 200
            "#,
        )
        .unwrap();
        assert_eq!(cfg.ime.overlay_poll_ms, 200);
    }

    #[test]
    fn cli_overlay_poll_ms_beats_file() {
        let mut cfg: Config = toml::from_str(
            r#"
            [ime]
            overlay_poll_ms = 200
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(None, Some(333));
        assert_eq!(cfg.ime.overlay_poll_ms, 333);
    }

    #[test]
    fn overlay_poll_ms_clamps_below_floor() {
        // Sub-floor values from config must be clamped up, even when no
        // CLI override is provided, so the main loop never sees a 0 ms
        // poll.
        let mut cfg: Config = toml::from_str(
            r#"
            [ime]
            overlay_poll_ms = 0
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(None, None);
        assert_eq!(cfg.ime.overlay_poll_ms, MIN_OVERLAY_POLL_MS);

        let mut cfg2 = Config::default();
        cfg2.apply_cli_overrides(None, Some(1));
        assert_eq!(cfg2.ime.overlay_poll_ms, MIN_OVERLAY_POLL_MS);
    }

    #[test]
    fn ime_mode_from_str_roundtrips() {
        use std::str::FromStr;
        assert_eq!(ImeMode::from_str("hotkey").unwrap(), ImeMode::Hotkey);
        assert_eq!(ImeMode::from_str("off").unwrap(), ImeMode::Off);
        assert_eq!(ImeMode::from_str("always").unwrap(), ImeMode::Always);
        assert!(ImeMode::from_str("banana").is_err());
    }
}
