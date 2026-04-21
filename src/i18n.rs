//! UI language selection. Non-Japanese users shouldn't have to stare
//! at the JP-only status bar — Issue discussed in main session 2026-04-21.
//!
//! Precedence (same pattern as `[ime]`):
//! 1. `--lang` CLI flag
//! 2. `[ui] lang` in `config.toml`
//! 3. OS locale detected via `sys-locale`
//! 4. `Lang::En` fallback (the whole point is to help non-JP users, so
//!    default-to-JA on detection failure would miss the target audience)
//!
//! All user-facing strings live in one of two `Messages` constants so
//! forgetting a translation is a compile error rather than a runtime
//! surprise. Format-string variants (file/image size) are helper methods
//! on `Messages` because `format!` needs the values at call time.

use std::str::FromStr;

/// Resolved UI language. Stored on [`App`] after config + CLI + auto
/// detection collapse into a concrete choice. `messages()` returns the
/// static table that `ui.rs` / `preview.rs` read from.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ja,
    #[default]
    En,
}

impl Lang {
    pub fn messages(self) -> &'static Messages {
        match self {
            Lang::Ja => &MESSAGES_JA,
            Lang::En => &MESSAGES_EN,
        }
    }
}

/// Config/CLI-level language enum. `Auto` triggers locale detection at
/// resolve time; the explicit variants short-circuit it. Separate from
/// [`Lang`] so we can serialize the user's raw choice back out unchanged
/// and so a stale locale detection never silently downgrades their
/// explicit `ja`/`en` pick.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum UiLang {
    /// Detect from the OS locale (`sys-locale`). Default.
    #[default]
    Auto,
    Ja,
    En,
}

impl UiLang {
    pub fn resolve(self, detected: Option<&str>) -> Lang {
        match self {
            UiLang::Ja => Lang::Ja,
            UiLang::En => Lang::En,
            UiLang::Auto => detect_from_locale(detected),
        }
    }
}

impl FromStr for UiLang {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(UiLang::Auto),
            "ja" => Ok(UiLang::Ja),
            "en" => Ok(UiLang::En),
            other => Err(format!(
                "invalid ui lang: {other:?} (expected auto | ja | en)"
            )),
        }
    }
}

// Case-insensitive TOML parsing via `try_from`. Keeps config.toml
// forgiving of `"JA"` / `"Ja"` etc. without bloating the enum with
// one `#[serde(alias = …)]` per casing variant.
impl<'de> serde::Deserialize<'de> for UiLang {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        UiLang::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Narrow the sys-locale tag to one of our two supported languages.
/// `starts_with("ja")` alone would misclassify `javanese` (`jv-*` in
/// BCP 47 — the language code is `jv`, not `ja` — so in practice
/// this can't bite us, but being defensive here costs nothing) and
/// wouldn't normalize POSIX-style `ja_JP.UTF-8` that `nl_langinfo`
/// can still emit on some Unix hosts. Accept both forms.
pub fn detect_from_locale(detected: Option<&str>) -> Lang {
    let Some(raw) = detected else {
        return Lang::En;
    };
    let lower = raw.to_ascii_lowercase();
    if lower == "ja" || lower.starts_with("ja-") || lower.starts_with("ja_") {
        Lang::Ja
    } else {
        Lang::En
    }
}

/// Query the OS for the current user locale. Wraps `sys-locale` so
/// tests can exercise `resolve` / `detect_from_locale` with
/// synthetic values without touching the real OS state.
pub fn current_os_locale() -> Option<String> {
    sys_locale::get_locale()
}

/// All user-facing strings in one place. Every field is `&'static str`
/// so the struct is a `const` literal and lookups are a pointer read.
/// Format-string variants live as helper methods (`image_too_large`,
/// `file_too_large`) because the value substitution happens at call
/// time.
pub struct Messages {
    pub lang: Lang,
    // ── status bar: rename mode ─────────────────────────────
    pub rename_confirm: &'static str,
    pub rename_cancel: &'static str,
    pub rename_reset: &'static str,
    pub rename_empty_enter_label: &'static str,
    // ── status bar: preview focus ──────────────────────────
    pub preview_scroll: &'static str,
    pub preview_close: &'static str,
    pub preview_swap: &'static str,
    pub preview_quit: &'static str,
    // ── status bar: file-tree focus ────────────────────────
    pub tree_move: &'static str,
    pub tree_parent_child: &'static str,
    pub tree_open: &'static str,
    pub tree_hidden: &'static str,
    pub tree_back: &'static str,
    pub tree_close: &'static str,
    pub tree_quit: &'static str,
    // ── status bar: pane focus ─────────────────────────────
    pub pane_split_vertical: &'static str,
    pub pane_split_horizontal: &'static str,
    pub pane_close: &'static str,
    pub pane_new_tab: &'static str,
    pub pane_rename_tab: &'static str,
    pub pane_tree: &'static str,
    pub pane_swap: &'static str,
    pub pane_ime: &'static str,
    pub pane_peer_launch: &'static str,
    pub pane_quit: &'static str,
    // ── preview panel ──────────────────────────────────────
    pub preview_binary: &'static str,
    pub preview_read_failed: &'static str,
    pub preview_not_regular: &'static str,
}

impl Messages {
    /// "Image too large (2.3MB > 20MB)" style error. Size values are
    /// already in MB; `size_mb` uses 1-decimal precision (shows the
    /// user what they actually have), cap uses 0-decimal (it's a
    /// round number in code).
    pub fn image_too_large(&self, size_mb: f64, max_mb: f64) -> String {
        match self.lang {
            Lang::Ja => format!("画像が大きすぎます（{:.1}MB > {:.0}MB）", size_mb, max_mb),
            Lang::En => format!("Image too large ({:.1}MB > {:.0}MB)", size_mb, max_mb),
        }
    }

    pub fn file_too_large(&self, size_mb: f64, max_mb: f64) -> String {
        match self.lang {
            Lang::Ja => format!(
                "ファイルが大きすぎます（{:.1}MB > {:.0}MB）",
                size_mb, max_mb
            ),
            Lang::En => format!("File too large ({:.1}MB > {:.0}MB)", size_mb, max_mb),
        }
    }
}

pub static MESSAGES_JA: Messages = Messages {
    lang: Lang::Ja,
    rename_confirm: " 決定  ",
    rename_cancel: " 取消  ",
    rename_reset: " 元に戻す",
    rename_empty_enter_label: "空Enter",
    preview_scroll: " スクロール  ",
    preview_close: " 閉じる  ",
    preview_swap: " 配置替  ",
    preview_quit: " 終了",
    tree_move: " 移動  ",
    tree_parent_child: " 親/下へ  ",
    tree_open: " 開く  ",
    tree_hidden: " 隠しファイル  ",
    tree_back: " 戻る  ",
    tree_close: " 閉じる  ",
    tree_quit: " 終了",
    pane_split_vertical: " 縦分割  ",
    pane_split_horizontal: " 横分割  ",
    pane_close: " 閉じる  ",
    pane_new_tab: " 新タブ  ",
    pane_rename_tab: " タブ名  ",
    pane_tree: " ツリー  ",
    pane_swap: " 配置替  ",
    pane_ime: " IME入力  ",
    pane_peer_launch: " Claude Code起動  ",
    pane_quit: " 終了",
    preview_binary: "\u{2718} バイナリファイルです",
    preview_read_failed: "ファイルを読み込めませんでした",
    preview_not_regular: "通常ファイルではありません",
};

pub static MESSAGES_EN: Messages = Messages {
    lang: Lang::En,
    rename_confirm: " confirm  ",
    rename_cancel: " cancel  ",
    rename_reset: " revert",
    rename_empty_enter_label: "Empty Enter",
    preview_scroll: " scroll  ",
    preview_close: " close  ",
    preview_swap: " swap  ",
    preview_quit: " quit",
    tree_move: " move  ",
    tree_parent_child: " up/down  ",
    tree_open: " open  ",
    tree_hidden: " hidden  ",
    tree_back: " back  ",
    tree_close: " close  ",
    tree_quit: " quit",
    pane_split_vertical: " v-split  ",
    pane_split_horizontal: " h-split  ",
    pane_close: " close  ",
    pane_new_tab: " new tab  ",
    pane_rename_tab: " rename  ",
    pane_tree: " tree  ",
    pane_swap: " swap  ",
    pane_ime: " ime  ",
    pane_peer_launch: " claude code  ",
    pane_quit: " quit",
    preview_binary: "\u{2718} Binary file",
    preview_read_failed: "Failed to read file",
    preview_not_regular: "Not a regular file",
};

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_from_locale ────────────────────────────────
    #[test]
    fn detect_none_falls_back_to_en() {
        assert_eq!(detect_from_locale(None), Lang::En);
    }

    #[test]
    fn detect_bcp47_ja_jp() {
        assert_eq!(detect_from_locale(Some("ja-JP")), Lang::Ja);
    }

    #[test]
    fn detect_posix_ja_jp_utf8() {
        // `nl_langinfo` on some Linux hosts still returns POSIX-style
        // tags. We must still recognize these as JA.
        assert_eq!(detect_from_locale(Some("ja_JP.UTF-8")), Lang::Ja);
    }

    #[test]
    fn detect_bare_ja() {
        assert_eq!(detect_from_locale(Some("ja")), Lang::Ja);
    }

    #[test]
    fn detect_en_us_is_en() {
        assert_eq!(detect_from_locale(Some("en-US")), Lang::En);
    }

    #[test]
    fn detect_unknown_locale_is_en() {
        assert_eq!(detect_from_locale(Some("zh-CN")), Lang::En);
        assert_eq!(detect_from_locale(Some("C")), Lang::En);
        assert_eq!(detect_from_locale(Some("POSIX")), Lang::En);
    }

    #[test]
    fn detect_case_insensitive() {
        // Some Windows builds emit `ja-JP` with mixed case; the BCP 47
        // spec says tags are case-insensitive, and `sys-locale` is
        // documented to normalize but we defend anyway.
        assert_eq!(detect_from_locale(Some("JA-JP")), Lang::Ja);
        assert_eq!(detect_from_locale(Some("Ja")), Lang::Ja);
    }

    #[test]
    fn detect_javanese_is_not_misclassified() {
        // `jv-*` is Javanese; a naive `starts_with("ja")` would
        // accidentally flag it as Japanese. Guard against regressions.
        assert_eq!(detect_from_locale(Some("jv-ID")), Lang::En);
    }

    // ── UiLang::resolve ───────────────────────────────────
    #[test]
    fn resolve_explicit_ja_ignores_detection() {
        assert_eq!(UiLang::Ja.resolve(Some("en-US")), Lang::Ja);
    }

    #[test]
    fn resolve_explicit_en_ignores_detection() {
        assert_eq!(UiLang::En.resolve(Some("ja-JP")), Lang::En);
    }

    #[test]
    fn resolve_auto_delegates_to_detection() {
        assert_eq!(UiLang::Auto.resolve(Some("ja-JP")), Lang::Ja);
        assert_eq!(UiLang::Auto.resolve(Some("en-US")), Lang::En);
        assert_eq!(UiLang::Auto.resolve(None), Lang::En);
    }

    // ── FromStr / Deserialize (case-insensitive) ─────────
    #[test]
    fn from_str_accepts_lowercase() {
        assert_eq!(UiLang::from_str("auto").unwrap(), UiLang::Auto);
        assert_eq!(UiLang::from_str("ja").unwrap(), UiLang::Ja);
        assert_eq!(UiLang::from_str("en").unwrap(), UiLang::En);
    }

    #[test]
    fn from_str_accepts_uppercase_and_mixed() {
        assert_eq!(UiLang::from_str("AUTO").unwrap(), UiLang::Auto);
        assert_eq!(UiLang::from_str("JA").unwrap(), UiLang::Ja);
        assert_eq!(UiLang::from_str("Ja").unwrap(), UiLang::Ja);
        assert_eq!(UiLang::from_str("eN").unwrap(), UiLang::En);
    }

    #[test]
    fn from_str_rejects_garbage() {
        assert!(UiLang::from_str("banana").is_err());
        assert!(UiLang::from_str("").is_err());
        assert!(UiLang::from_str("zh").is_err());
    }

    // ── messages() accessors ────────────────────────────
    #[test]
    fn messages_are_distinct_per_lang() {
        assert_ne!(
            Lang::Ja.messages().pane_split_vertical,
            Lang::En.messages().pane_split_vertical
        );
    }

    #[test]
    fn format_helpers_respect_lang() {
        let en = Lang::En.messages();
        let ja = Lang::Ja.messages();
        assert!(en.image_too_large(25.3, 20.0).contains("Image"));
        assert!(ja.image_too_large(25.3, 20.0).contains("画像"));
        assert!(en.file_too_large(11.0, 10.0).contains("File"));
        assert!(ja.file_too_large(11.0, 10.0).contains("ファイル"));
    }
}
