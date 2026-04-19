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
}

/// How Ctrl+; behaves in a focused pane.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum ImeMode {
    /// Ctrl+; opens the IME composition overlay. Default.
    #[default]
    Hotkey,
    /// Ctrl+; is passed through to the pane's PTY as a normal key,
    /// and the overlay is never opened. For users who don't use IME
    /// or prefer their terminal's own IME handling.
    Off,
}

impl fmt::Display for ImeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImeMode::Hotkey => f.write_str("hotkey"),
            ImeMode::Off => f.write_str("off"),
        }
    }
}

impl std::str::FromStr for ImeMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hotkey" => Ok(ImeMode::Hotkey),
            "off" => Ok(ImeMode::Off),
            other => Err(format!(
                "invalid ime mode: {other:?} (expected hotkey | off)"
            )),
        }
    }
}

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
        let text = match std::fs::read_to_string(&path) {
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
    pub fn apply_cli_overrides(&mut self, ime_mode: Option<ImeMode>) {
        if let Some(mode) = ime_mode {
            self.ime.mode = mode;
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
    fn rejects_unknown_ime_mode_value() {
        let err = toml::from_str::<Config>(
            r#"
            [ime]
            mode = "always"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("always") || err.to_string().contains("variant"));
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
        cfg.apply_cli_overrides(Some(ImeMode::Off));
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
        cfg.apply_cli_overrides(None);
        assert_eq!(cfg.ime.mode, ImeMode::Off);
    }

    #[test]
    fn ime_mode_from_str_roundtrips() {
        use std::str::FromStr;
        assert_eq!(ImeMode::from_str("hotkey").unwrap(), ImeMode::Hotkey);
        assert_eq!(ImeMode::from_str("off").unwrap(), ImeMode::Off);
        assert!(ImeMode::from_str("always").is_err());
    }
}
