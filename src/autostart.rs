//! "Lancer au démarrage": per-user autostart via the standard HKCU Run key.
//! No admin rights needed — this key is user-writable by design.

use std::io;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "HyperXOverlay";

fn open_run_key() -> io::Result<RegKey> {
    // `Run` normally exists, but create it defensively: opening it read/write
    // made the menu look successful on systems where the key was absent.
    RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY_PATH)
        .map(|(key, _)| key)
}

pub fn is_enabled() -> bool {
    open_run_key()
        .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
        .is_ok()
}

pub fn set_enabled(enabled: bool) -> bool {
    let Ok(key) = open_run_key() else { return false };
    if enabled {
        let Ok(exe) = std::env::current_exe() else { return false };
        key.set_value(VALUE_NAME, &format!("\"{}\"", exe.display()))
            .is_ok()
    } else {
        match key.delete_value(VALUE_NAME) {
            Ok(()) => true,
            Err(e) => e.kind() == io::ErrorKind::NotFound,
        }
    }
}
