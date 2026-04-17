use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::easy::HighlightLines;

const MAX_PREVIEW_LINES: usize = 500;
const BINARY_CHECK_BYTES: usize = 8192;
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

/// A styled text span for rendering.
#[derive(Debug, Clone)]
pub struct StyledSpan {
    pub text: String,
    pub fg: (u8, u8, u8),
}

/// File preview state.
pub struct Preview {
    pub file_path: Option<PathBuf>,
    pub lines: Vec<String>,
    pub highlighted_lines: Vec<Vec<StyledSpan>>,
    /// Vertical scroll position (line index of the top visible line).
    pub scroll_offset: usize,
    /// Horizontal scroll position (char count dropped from the left of
    /// each rendered line). Enables viewing long lines that exceed the
    /// preview panel width.
    pub h_scroll_offset: usize,
    pub is_binary: bool,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Preview {
    pub fn new() -> Self {
        Self {
            file_path: None,
            lines: Vec::new(),
            highlighted_lines: Vec::new(),
            scroll_offset: 0,
            h_scroll_offset: 0,
            is_binary: false,
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Load a file for preview.
    pub fn load(&mut self, path: &Path) {
        if self.file_path.as_deref() == Some(path) {
            return;
        }

        self.file_path = Some(path.to_path_buf());
        self.scroll_offset = 0;
        self.h_scroll_offset = 0;
        self.lines.clear();
        self.highlighted_lines.clear();
        self.is_binary = false;

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => {
                self.lines = vec!["ファイルを読み込めませんでした".to_string()];
                return;
            }
        };

        if !metadata.is_file() {
            self.lines = vec!["通常ファイルではありません".to_string()];
            return;
        }

        if metadata.len() > MAX_FILE_SIZE {
            self.lines = vec![format!(
                "ファイルが大きすぎます（{:.1}MB > {:.0}MB）",
                metadata.len() as f64 / 1024.0 / 1024.0,
                MAX_FILE_SIZE as f64 / 1024.0 / 1024.0
            )];
            return;
        }

        if is_binary_file(path) {
            self.is_binary = true;
            return;
        }

        // Read text file
        match File::open(path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                self.lines = reader
                    .lines()
                    .take(MAX_PREVIEW_LINES)
                    .filter_map(|l| l.ok())
                    .collect();
            }
            Err(_) => {
                self.lines = vec!["ファイルを読み込めませんでした".to_string()];
                return;
            }
        }

        // Apply syntax highlighting
        self.highlight(path);
    }

    /// Apply syntax highlighting to loaded lines.
    fn highlight(&mut self, path: &Path) {
        let syntax = self
            .syntax_set
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = &self.theme_set.themes["base16-eighties.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        self.highlighted_lines.clear();

        for line in &self.lines {
            let line_with_newline = format!("{}\n", line);
            match highlighter.highlight_line(&line_with_newline, &self.syntax_set) {
                Ok(ranges) => {
                    let spans: Vec<StyledSpan> = ranges
                        .into_iter()
                        .map(|(style, text)| {
                            let fg = style.foreground;
                            StyledSpan {
                                text: text.trim_end_matches('\n').to_string(),
                                fg: (fg.r, fg.g, fg.b),
                            }
                        })
                        .filter(|s| !s.text.is_empty())
                        .collect();
                    self.highlighted_lines.push(spans);
                }
                Err(_) => {
                    // Fallback: plain text
                    self.highlighted_lines.push(vec![StyledSpan {
                        text: line.clone(),
                        fg: (0xe6, 0xed, 0xf3),
                    }]);
                }
            }
        }
    }

    /// Close the preview.
    pub fn close(&mut self) {
        self.file_path = None;
        self.lines.clear();
        self.highlighted_lines.clear();
        self.scroll_offset = 0;
        self.h_scroll_offset = 0;
        self.is_binary = false;
    }

    /// Check if preview is active.
    pub fn is_active(&self) -> bool {
        self.file_path.is_some()
    }

    /// Get the filename for display.
    pub fn filename(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// Scroll up by amount.
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll down by amount.
    pub fn scroll_down(&mut self, amount: usize) {
        let max_offset = self.lines.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max_offset);
    }

    /// Scroll left by N chars. Clamps at column 0.
    pub fn scroll_left(&mut self, amount: usize) {
        self.h_scroll_offset = self.h_scroll_offset.saturating_sub(amount);
    }

    /// Scroll right by N chars. Clamped so there's always at least
    /// a bit of text visible (we stop when h_scroll equals the widest
    /// line minus 10 chars — keeps the user from scrolling off into
    /// blank territory).
    pub fn scroll_right(&mut self, amount: usize) {
        let widest = self
            .lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        let max_h = widest.saturating_sub(10);
        self.h_scroll_offset = (self.h_scroll_offset + amount).min(max_h);
    }
}

/// Check if a file is likely binary by reading only the first N bytes.
fn is_binary_file(path: &Path) -> bool {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut reader = BufReader::new(file);
    let mut buf = [0u8; BINARY_CHECK_BYTES];
    match reader.read(&mut buf) {
        Ok(n) => buf[..n].contains(&0),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_initial_state() {
        let preview = Preview::new();
        assert!(!preview.is_active());
        assert!(preview.lines.is_empty());
    }

    #[test]
    fn test_preview_load_text_file() {
        let mut preview = Preview::new();
        preview.load(Path::new("Cargo.toml"));
        assert!(preview.is_active());
        assert!(!preview.is_binary);
        assert!(!preview.lines.is_empty());
        assert!(!preview.highlighted_lines.is_empty());
    }

    #[test]
    fn test_preview_close() {
        let mut preview = Preview::new();
        preview.load(Path::new("Cargo.toml"));
        assert!(preview.is_active());

        preview.close();
        assert!(!preview.is_active());
        assert!(preview.lines.is_empty());
        assert!(preview.highlighted_lines.is_empty());
    }

    #[test]
    fn test_preview_scroll() {
        let mut preview = Preview::new();
        preview.lines = (0..100).map(|i| format!("line {}", i)).collect();
        preview.scroll_down(10);
        assert_eq!(preview.scroll_offset, 10);
        preview.scroll_up(5);
        assert_eq!(preview.scroll_offset, 5);
        preview.scroll_up(100);
        assert_eq!(preview.scroll_offset, 0);
    }

    #[test]
    fn test_preview_highlight_rust() {
        let mut preview = Preview::new();
        preview.load(Path::new("src/main.rs"));
        assert!(!preview.highlighted_lines.is_empty());
        // Highlighted lines should have colored spans
        let first = &preview.highlighted_lines[0];
        assert!(!first.is_empty());
    }
}
