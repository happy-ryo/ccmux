//! Input routing helpers split out of `src/app.rs`.
//!
//! First slice of Issue #66 — only the IME composition overlay lives
//! here so far. Other modal input paths (rename, mouse, key chords)
//! remain on `App` until follow-up slices move them.

pub mod overlay;
