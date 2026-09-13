//! "Lancer au démarrage": per-user autostart via the standard HKCU Run key.
//! No admin rights needed — this key is user-writable by design.

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "HyperXOverlay";

fn open_run_key() -> std::io::Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY_PATH, KEY_READ | KEY_WRITE)
}

pub fn is_enabled() -> bool {
    open_run_key()
        .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
        .is_ok()
}

pub fn set_enabled(enabled: bool) {
    let Ok(key) = open_run_key() else {
        return;
    };
    if enabled {
        if let Ok(exe) = std::env::current_exe() {
            let _ = key.set_value(VALUE_NAME, &format!("\"{}\"", exe.display()));
        }
    } else {
        let _ = key.delete_value(VALUE_NAME);
    }
}
