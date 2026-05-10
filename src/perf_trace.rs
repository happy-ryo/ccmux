//! Diagnostic-only perf trace gated behind the `RENGA_PERF_TRACE`
//! environment variable. When `RENGA_PERF_TRACE=1`, structured timing
//! records are appended to `/tmp/renga-perf-trace.log` for the
//! lifetime of the process. Default behavior (env var unset) is a
//! zero-cost no-op: every entry point checks `is_enabled()` first,
//! and that returns the cached `false` immediately.
//!
//! This module exists for the renga TUI selection-slowdown
//! investigation (Issue #234 follow-up) and is meant to live on a
//! diagnostic branch only. Not for release.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();
static SINK: OnceLock<Mutex<BufWriter<File>>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();

pub fn init() {
    let enabled = std::env::var("RENGA_PERF_TRACE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let _ = ENABLED.set(enabled);
    let _ = EPOCH.set(Instant::now());
    if enabled {
        if let Ok(f) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("/tmp/renga-perf-trace.log")
        {
            let _ = SINK.set(Mutex::new(BufWriter::new(f)));
            log("# renga perf trace started");
        }
    }
}

#[inline]
pub fn is_enabled() -> bool {
    *ENABLED.get().unwrap_or(&false)
}

#[inline]
pub fn elapsed_us() -> u64 {
    EPOCH
        .get()
        .map(|e| e.elapsed().as_micros() as u64)
        .unwrap_or(0)
}

pub fn log(line: &str) {
    if !is_enabled() {
        return;
    }
    if let Some(sink) = SINK.get() {
        if let Ok(mut s) = sink.lock() {
            let _ = writeln!(s, "[{:>10}us] {}", elapsed_us(), line);
            let _ = s.flush();
        }
    }
}

/// Scope guard: records elapsed micros from construction to drop as
/// a single trace line, prefixed by `tag`. Skips the formatting and
/// the file write entirely when tracing is disabled.
pub struct Section {
    tag: &'static str,
    extra: String,
    start: Instant,
}

impl Section {
    #[inline]
    pub fn new(tag: &'static str) -> Self {
        Self {
            tag,
            extra: String::new(),
            start: Instant::now(),
        }
    }

    #[inline]
    pub fn with_extra(tag: &'static str, extra: String) -> Self {
        Self {
            tag,
            extra,
            start: Instant::now(),
        }
    }

}

impl Drop for Section {
    fn drop(&mut self) {
        if !is_enabled() {
            return;
        }
        let us = self.start.elapsed().as_micros() as u64;
        if self.extra.is_empty() {
            log(&format!("{} cost_us={}", self.tag, us));
        } else {
            log(&format!("{} cost_us={} {}", self.tag, us, self.extra));
        }
    }
}
