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
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct ImeConfig {
    pub mode: ImeMode,
    /// When `true`, pane repaints driven by PTY output are suppressed
    /// while the IME composition overlay is open (Issue #37 / #82
    /// Phase 2). vt100 parsers keep advancing in the background, so
    /// panes catch up instantly when the overlay closes. Off by
    /// default because it's a user-visible behavior change (the
    /// screen literally stops updating during composition) even
    /// though it's the intended way to kill overlay flicker on hosts
    /// where `overlay_poll_ms` throttling alone isn't enough.
    pub freeze_panes_on_overlay: bool,
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

    /// Apply an optional CLI override on top of the loaded config.
    /// `None` leaves the field untouched, mirroring the precedence
    /// "CLI > file > default".
    pub fn apply_cli_overrides(
        &mut self,
        ime_mode: Option<ImeMode>,
        freeze_panes_on_overlay: Option<bool>,
    ) {
        if let Some(mode) = ime_mode {
            self.ime.mode = mode;
        }
        if let Some(freeze) = freeze_panes_on_overlay {
            self.ime.freeze_panes_on_overlay = freeze;
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
    fn freeze_panes_defaults_to_false() {
        let cfg = Config::default();
        assert!(!cfg.ime.freeze_panes_on_overlay);
    }

    #[test]
    fn parses_freeze_panes_from_toml() {
        let cfg: Config = toml::from_str(
            r#"
            [ime]
            freeze_panes_on_overlay = true
            "#,
        )
        .unwrap();
        assert!(cfg.ime.freeze_panes_on_overlay);
    }

    #[test]
    fn cli_freeze_panes_beats_file() {
        let mut cfg: Config = toml::from_str(
            r#"
            [ime]
            freeze_panes_on_overlay = true
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(None, Some(false));
        assert!(!cfg.ime.freeze_panes_on_overlay);

        let mut cfg2 = Config::default();
        cfg2.apply_cli_overrides(None, Some(true));
        assert!(cfg2.ime.freeze_panes_on_overlay);
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
