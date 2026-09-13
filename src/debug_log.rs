//! Opt-in diagnostic logging.
//!
//! The protocol in `hid_status` was reverse-engineered against *other
//! people's* hardware, not tested against a real Pulsefire Saga Pro / Cloud
//! III S here. If it doesn't work on someone's actual setup, this is how we
//! find out why without needing the hardware ourselves: set
//! `HYPERX_OVERLAY_DEBUG=1` before launching, reproduce, and send back
//! `%TEMP%\hyperx-overlay-debug.log`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("HYPERX_OVERLAY_DEBUG").is_some())
}

pub fn log_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("hyperx-overlay-debug.log");
    p
}

pub fn log(msg: impl AsRef<str>) {
    if !enabled() {
        return;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = writeln!(f, "{}", msg.as_ref());
    }
}

pub fn log_bytes(label: &str, bytes: &[u8]) {
    if !enabled() {
        return;
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02x} ")).collect();
    log(format!("{label}: {}", hex.trim_end()));
}
