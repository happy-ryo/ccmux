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
    pub ui: UiConfig,
}

/// Top-level UI settings. Currently only carries the language pick;
/// future display-affecting options (theme overrides, etc.) can hang
/// off the same section.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// UI language for status bar hints and preview error messages.
    /// `auto` (default) picks based on the OS locale; `ja` / `en`
    /// force a specific language regardless of locale. Case-insensitive
    /// in TOML (`"JA"` / `"Ja"` / `"ja"` all work) because the existing
    /// `[ime] mode` convention is lowercase and we don't want a fat-
    /// finger to fail the whole config parse silently.
    pub lang: crate::i18n::UiLang,
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
    /// When [`ImeConfig::freeze_panes_on_overlay`] is true, optionally
    /// force a single repaint every `overlay_catchup_ms` milliseconds
    /// so the user sees body-content progress (Claude writing new
    /// lines, shell output scrolling) periodically without the
    /// continuous flicker of unthrottled repaints. `0` disables the
    /// periodic catch-up and gives a pure freeze. Non-zero values are
    /// clamped to [`MIN_OVERLAY_CATCHUP_MS`] at apply time.
    pub overlay_catchup_ms: u64,
}

/// Floor for `overlay_catchup_ms` when non-zero. Below this the
/// periodic repaint becomes a near-continuous storm that defeats the
/// point of freezing in the first place.
pub const MIN_OVERLAY_CATCHUP_MS: u64 = 100;

/// How Ctrl+; behaves in a focused pane.
///
/// The historical `always` variant (auto-open on Claude pane focus) is
/// gone — the implementation never worked reliably enough to be
/// recommended, and removing it cuts a non-trivial state machine
/// (`always_dismissed_pane`, printable-key auto-open, per-focus
/// dismissal tracking) that was carrying its own bugs. Users who
/// want IME ready from the first keystroke should just press
/// `Ctrl+;` once on focus.
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
        overlay_catchup_ms: Option<u64>,
        ui_lang: Option<crate::i18n::UiLang>,
    ) {
        if let Some(mode) = ime_mode {
            self.ime.mode = mode;
        }
        if let Some(freeze) = freeze_panes_on_overlay {
            self.ime.freeze_panes_on_overlay = freeze;
        }
        if let Some(ms) = overlay_catchup_ms {
            self.ime.overlay_catchup_ms = ms;
        }
        if let Some(lang) = ui_lang {
            self.ui.lang = lang;
        }
        // Clamp any non-zero value regardless of origin so the main
        // loop never sees a sub-floor interval.
        if self.ime.overlay_catchup_ms != 0 && self.ime.overlay_catchup_ms < MIN_OVERLAY_CATCHUP_MS
        {
            self.ime.overlay_catchup_ms = MIN_OVERLAY_CATCHUP_MS;
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
    fn rejects_always_mode_value() {
        // The legacy `always` variant was removed because the auto-open
        // behavior never worked reliably. A config file still pinning
        // `mode = "always"` must be rejected with a parse error, not
        // silently accepted, so users get a clear signal to migrate to
        // `hotkey`.
        let err = toml::from_str::<Config>(
            r#"
            [ime]
            mode = "always"
            "#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("always") || err.to_string().contains("variant"),
            "got: {err}"
        );
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
        cfg.apply_cli_overrides(Some(ImeMode::Off), None, None, None);
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
        cfg.apply_cli_overrides(None, None, None, None);
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
        cfg.apply_cli_overrides(None, Some(false), None, None);
        assert!(!cfg.ime.freeze_panes_on_overlay);

        let mut cfg2 = Config::default();
        cfg2.apply_cli_overrides(None, Some(true), None, None);
        assert!(cfg2.ime.freeze_panes_on_overlay);
    }

    #[test]
    fn overlay_catchup_ms_defaults_to_zero() {
        let cfg = Config::default();
        assert_eq!(cfg.ime.overlay_catchup_ms, 0);
    }

    #[test]
    fn parses_overlay_catchup_ms_from_toml() {
        let cfg: Config = toml::from_str(
            r#"
            [ime]
            overlay_catchup_ms = 2500
            "#,
        )
        .unwrap();
        assert_eq!(cfg.ime.overlay_catchup_ms, 2500);
    }

    #[test]
    fn cli_overlay_catchup_ms_beats_file_and_clamps_below_floor() {
        let mut cfg: Config = toml::from_str(
            r#"
            [ime]
            overlay_catchup_ms = 500
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(None, None, Some(3000), None);
        assert_eq!(cfg.ime.overlay_catchup_ms, 3000);

        let mut cfg2 = Config::default();
        // Non-zero sub-floor value must be clamped up.
        cfg2.apply_cli_overrides(None, None, Some(10), None);
        assert_eq!(cfg2.ime.overlay_catchup_ms, MIN_OVERLAY_CATCHUP_MS);

        // Zero must stay zero (means "disabled").
        let mut cfg3 = Config::default();
        cfg3.apply_cli_overrides(None, None, Some(0), None);
        assert_eq!(cfg3.ime.overlay_catchup_ms, 0);
    }

    #[test]
    fn ime_mode_from_str_roundtrips() {
        use std::str::FromStr;
        assert_eq!(ImeMode::from_str("hotkey").unwrap(), ImeMode::Hotkey);
        assert_eq!(ImeMode::from_str("off").unwrap(), ImeMode::Off);
        assert!(
            ImeMode::from_str("always").is_err(),
            "`always` was removed and must no longer parse"
        );
        assert!(ImeMode::from_str("banana").is_err());
    }

    // ── [ui] lang ────────────────────────────────────────

    #[test]
    fn ui_lang_defaults_to_auto() {
        let cfg = Config::default();
        assert_eq!(cfg.ui.lang, crate::i18n::UiLang::Auto);
    }

    #[test]
    fn parses_ui_lang_from_toml_lowercase() {
        let cfg: Config = toml::from_str(
            r#"
            [ui]
            lang = "ja"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.ui.lang, crate::i18n::UiLang::Ja);
    }

    #[test]
    fn parses_ui_lang_case_insensitive() {
        // TOML config is forgiving of casing — `"JA"` / `"En"` / `"Auto"`
        // all accepted so fat-fingers don't silently null out the pick.
        let cfg_ja: Config = toml::from_str(
            r#"
            [ui]
            lang = "JA"
            "#,
        )
        .unwrap();
        assert_eq!(cfg_ja.ui.lang, crate::i18n::UiLang::Ja);

        let cfg_en: Config = toml::from_str(
            r#"
            [ui]
            lang = "En"
            "#,
        )
        .unwrap();
        assert_eq!(cfg_en.ui.lang, crate::i18n::UiLang::En);

        let cfg_auto: Config = toml::from_str(
            r#"
            [ui]
            lang = "AUTO"
            "#,
        )
        .unwrap();
        assert_eq!(cfg_auto.ui.lang, crate::i18n::UiLang::Auto);
    }

    #[test]
    fn rejects_unknown_ui_lang_value() {
        // An unknown value must bubble up as a parse error instead of
        // silently falling through to a default — otherwise a typo
        // like `lang = "jp"` would masquerade as `lang = "auto"`.
        let err = toml::from_str::<Config>(
            r#"
            [ui]
            lang = "jp"
            "#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("jp") || err.to_string().contains("invalid"),
            "got: {err}"
        );
    }

    #[test]
    fn cli_ui_lang_beats_file() {
        let mut cfg: Config = toml::from_str(
            r#"
            [ui]
            lang = "ja"
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(None, None, None, Some(crate::i18n::UiLang::En));
        assert_eq!(cfg.ui.lang, crate::i18n::UiLang::En);
    }

    #[test]
    fn cli_ui_lang_none_leaves_file_value() {
        let mut cfg: Config = toml::from_str(
            r#"
            [ui]
            lang = "en"
            "#,
        )
        .unwrap();
        cfg.apply_cli_overrides(None, None, None, None);
        assert_eq!(cfg.ui.lang, crate::i18n::UiLang::En);
    }
}
