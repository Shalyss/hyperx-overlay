//! Vendor HID protocol for reading battery status off HyperX wireless dongles.
//!
//! Reverse-engineered by the community (not documented by HyperX/HP):
//! - Pulsefire Saga Pro: <https://github.com/notwaterbtl/hyperx-saga-control>
//! - Cloud III S Wireless: <https://github.com/auto94/HyperX-Cloud-2-Battery-Monitor>

use hidapi::{HidApi, HidDevice};
use std::time::{Duration, Instant};

pub const VID_HP: u16 = 0x03F0;
/// Pulsefire Saga Pro: wired dock PID and 2.4GHz wireless dongle PID.
pub const MOUSE_PIDS: [u16; 2] = [0x04BF, 0x06BF];
/// Cloud III S Wireless dongle.
pub const HEADSET_PID: u16 = 0x06BE;
/// HID usage page/usage of the vendor status collection exposed by the
/// Cloud III S dongle (confirmed from the reference implementation above).
const HEADSET_USAGE_PAGE: u16 = 448;
const HEADSET_USAGE: u16 = 1;
/// Lighting/LampArray usage page — present on the Saga Pro dongle but never
/// answers the status query, so we skip it to save a wasted round trip.
const LAMP_ARRAY_USAGE_PAGE: u16 = 0x59;

#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceStatus {
    pub connected: bool,
    pub battery_pct: Option<u8>,
    pub charging: bool,
}

fn pad64(data: &[u8]) -> [u8; 64] {
    let mut buf = [0u8; 64];
    buf[..data.len()].copy_from_slice(data);
    buf
}

fn drain(dev: &HidDevice) {
    let mut scratch = [0u8; 64];
    while matches!(dev.read_timeout(&mut scratch, 0), Ok(n) if n > 0) {}
}

/// Send `request` and wait up to ~350ms for a response starting with `match_prefix`.
fn request_matching(dev: &HidDevice, request: &[u8], match_prefix: &[u8]) -> Option<[u8; 64]> {
    drain(dev);
    dev.write(request).ok()?;

    let deadline = Instant::now() + Duration::from_millis(350);
    let mut resp = [0u8; 64];
    while Instant::now() < deadline {
        if let Ok(n) = dev.read_timeout(&mut resp, 100) {
            if n >= match_prefix.len() && resp[..match_prefix.len()] == *match_prefix {
                return Some(resp);
            }
        }
    }
    None
}

/// Pulsefire Saga Pro battery/power status.
///
/// Request `50 02` (padded to 64 bytes), response `51 02 PP SS TT 00 VV VV`:
/// PP = battery percent, SS = power state (0=on battery, 1=charging, 2=full),
/// TT = temperature, VV VV = voltage in mV (little-endian). Multiple HID
/// collections exist on the dongle (movement, lighting/LampArray, status); we
/// probe each non-lighting one since the exact collection can vary by firmware.
pub fn poll_mouse(api: &HidApi) -> DeviceStatus {
    let request = pad64(&[0x50, 0x02]);
    for info in api.device_list() {
        if info.vendor_id() != VID_HP
            || !MOUSE_PIDS.contains(&info.product_id())
            || info.usage_page() == LAMP_ARRAY_USAGE_PAGE
        {
            continue;
        }
        let Ok(dev) = info.open_device(api) else {
            continue;
        };
        if let Some(resp) = request_matching(&dev, &request, &[0x51, 0x02]) {
            let pct = resp[2];
            let state = resp[3];
            return DeviceStatus {
                connected: true,
                battery_pct: (pct <= 100).then_some(pct),
                charging: state == 0x01 || state == 0x02,
            };
        }
    }
    DeviceStatus::default()
}

/// Cloud III S Wireless battery status.
///
/// Request `0c 02 03 01 00 06` on the vendor collection (usage page 448,
/// usage 1); battery percent lands at byte 6 of the response.
pub fn poll_headset(api: &HidApi) -> DeviceStatus {
    let request = [0x0c, 0x02, 0x03, 0x01, 0x00, 0x06];
    for info in api.device_list() {
        if info.vendor_id() != VID_HP
            || info.product_id() != HEADSET_PID
            || info.usage_page() != HEADSET_USAGE_PAGE
            || info.usage() != HEADSET_USAGE
        {
            continue;
        }
        let Ok(dev) = info.open_device(api) else {
            continue;
        };
        drain(&dev);
        if dev.write(&request).is_err() {
            continue;
        }
        let mut resp = [0u8; 64];
        if let Ok(n) = dev.read_timeout(&mut resp, 1000) {
            if n > 6 {
                let pct = resp[6];
                return DeviceStatus {
                    connected: true,
                    battery_pct: (pct <= 100).then_some(pct),
                    charging: false,
                };
            }
        }
    }
    DeviceStatus::default()
}
