//! Background npm version check.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Shared state for the latest version fetched from npm registry.
#[derive(Clone, Default)]
pub struct VersionInfo {
    inner: Arc<Mutex<Option<String>>>,
}

impl VersionInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the latest version if a newer one is available.
    pub fn update_available(&self) -> Option<String> {
        let latest = self.inner.lock().ok()?.clone()?;
        if is_newer(&latest, CURRENT_VERSION) {
            Some(latest)
        } else {
            None
        }
    }

    fn set(&self, version: String) {
        if let Ok(mut lock) = self.inner.lock() {
            *lock = Some(version);
        }
    }
}

/// Spawn a background thread to check npm for a newer version.
pub fn spawn_check(info: VersionInfo) {
    thread::spawn(move || {
        let _ = thread::sleep(Duration::from_secs(1)); // delay so it doesn't compete with startup
        match fetch_latest() {
            Ok(version) => info.set(version),
            Err(_) => {}
        }
    });
}

fn fetch_latest() -> Result<String, Box<dyn std::error::Error>> {
    let response = ureq::get("https://registry.npmjs.org/ccmux-cli/latest")
        .timeout(Duration::from_secs(5))
        .call()?;
    let json: serde_json::Value = response.into_json()?;
    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or("no version field")?
        .to_string();
    Ok(version)
}

/// Compare semver-like versions (simple major.minor.patch).
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    parse(latest) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.4.0", "0.3.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.0", "0.3.0"));
    }
}
