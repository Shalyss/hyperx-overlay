//! Vendor HID protocol for reading battery status off HyperX wireless dongles.
//!
//! Reverse-engineered by the community (not documented by HyperX/HP), and not
//! validated here against real hardware — see `debug_log` for how to collect
//! diagnostics from a machine that actually has the devices:
//! - Pulsefire Saga Pro: <https://github.com/notwaterbtl/hyperx-saga-control>
//! - Cloud III S Wireless: <https://github.com/auto94/HyperX-Cloud-2-Battery-Monitor>

use crate::debug_log;
use hidapi::{DeviceInfo, HidApi, HidDevice};
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
    /// The wireless dongle is still enumerated, but the mouse did not answer
    /// its status request (normally because it entered power-saving sleep).
    pub sleeping: bool,
}

fn pad64(data: &[u8]) -> [u8; 64] {
    let mut buf = [0u8; 64];
    buf[..data.len()].copy_from_slice(data);
    buf
}

fn product_matches(info: &DeviceInfo, needle: &str) -> bool {
    info.product_string()
        .is_some_and(|s| s.to_lowercase().contains(needle))
}

fn drain(dev: &HidDevice) {
    let mut scratch = [0u8; 64];
    while matches!(dev.read_timeout(&mut scratch, 0), Ok(n) if n > 0) {}
}

/// Log every HP/Kingston HID collection currently enumerated, so that if the
/// known PIDs above are wrong for someone's specific unit/region, we can see
/// the real vendor/product IDs and usage pages in their debug log.
pub fn log_known_devices(api: &HidApi) {
    debug_log::log("---- HID device enumeration (VID 0x03F0 / 0x0951) ----");
    for info in api.device_list() {
        if info.vendor_id() != VID_HP && info.vendor_id() != 0x0951 {
            continue;
        }
        debug_log::log(format!(
            "vid={:#06x} pid={:#06x} usage_page={} usage={} product={:?} path={:?}",
            info.vendor_id(),
            info.product_id(),
            info.usage_page(),
            info.usage(),
            info.product_string(),
            info.path()
        ));
    }
}

/// Send `request` and wait up to ~350ms for a response starting with `match_prefix`.
fn request_matching(
    dev: &HidDevice,
    request: &[u8],
    match_prefix: &[u8],
    log_label: &str,
) -> Option<[u8; 64]> {
    drain(dev);
    debug_log::log_bytes(&format!("{log_label} >>"), request);
    dev.write(request).ok()?;

    let deadline = Instant::now() + Duration::from_millis(350);
    let mut resp = [0u8; 64];
    while Instant::now() < deadline {
        if let Ok(n) = dev.read_timeout(&mut resp, 100) {
            if n > 0 {
                debug_log::log_bytes(&format!("{log_label} <<"), &resp[..n]);
            }
            if n >= match_prefix.len() && resp[..match_prefix.len()] == *match_prefix {
                return Some(resp);
            }
        }
    }
    None
}

/// Parse a Pulsefire Saga Pro status response: `51 02 PP SS TT 00 VV VV`.
/// PP = battery percent, SS = power state (0=on battery, 1=charging, 2=full).
/// Pure function (no I/O) so it can be unit-tested against captured bytes
/// without needing the mouse plugged in.
fn parse_mouse_response(resp: &[u8]) -> Option<DeviceStatus> {
    if resp.len() < 4 || resp[0] != 0x51 || resp[1] != 0x02 {
        return None;
    }
    let pct = resp[2];
    let state = resp[3];
    Some(DeviceStatus {
        connected: true,
        battery_pct: (pct <= 100).then_some(pct),
        charging: state == 0x01 || state == 0x02,
        sleeping: false,
    })
}

/// Parse a Cloud III S Wireless status response: battery percent at byte 6.
///
/// No charging bit is known for this model — the reference implementation
/// this protocol was reverse-engineered from
/// (auto94/HyperX-Cloud-2-Battery-Monitor) has no charging detection for
/// Cloud III S either, only for the plain (non-S) Cloud III. So `charging`
/// is always reported `false` here until someone captures the real byte via
/// `HYPERX_OVERLAY_DEBUG=1` while charging and we can add it.
fn parse_headset_response(resp: &[u8]) -> Option<DeviceStatus> {
    if resp.len() <= 6 {
        return None;
    }
    let pct = resp[6];
    Some(DeviceStatus {
        connected: true,
        battery_pct: (pct <= 100).then_some(pct),
        charging: false,
        sleeping: false,
    })
}

/// Pulsefire Saga Pro battery/power status. Multiple HID collections exist on
/// the dongle (movement, lighting/LampArray, status); we probe each
/// non-lighting one since the exact collection can vary by firmware.
pub fn poll_mouse(api: &HidApi) -> DeviceStatus {
    let request = pad64(&[0x50, 0x02]);
    for info in api.device_list() {
        let matches_id = info.vendor_id() == VID_HP && MOUSE_PIDS.contains(&info.product_id());
        let matches_name = info.vendor_id() == VID_HP && product_matches(info, "saga pro");
        if (!matches_id && !matches_name) || info.usage_page() == LAMP_ARRAY_USAGE_PAGE {
            continue;
        }
        let Ok(dev) = info.open_device(api) else {
            continue;
        };
        if let Some(resp) = request_matching(&dev, &request, &[0x51, 0x02], "mouse") {
            if let Some(status) = parse_mouse_response(&resp) {
                debug_log::log(format!("mouse: parsed {status:?}"));
                return status;
            }
        }
    }
    DeviceStatus::default()
}

/// The receiver is still connected to Windows, even if the mouse is asleep
/// and therefore does not answer its vendor status report. This distinction
/// lets the UI show "Veille" instead of pretending the mouse disappeared.
pub fn mouse_status_collection_present(api: &HidApi) -> bool {
    api.device_list().any(|info| {
        let matches_id = info.vendor_id() == VID_HP && MOUSE_PIDS.contains(&info.product_id());
        let matches_name = info.vendor_id() == VID_HP && product_matches(info, "saga pro");
        (matches_id || matches_name) && info.usage_page() != LAMP_ARRAY_USAGE_PAGE
    })
}

/// Cloud III S Wireless battery status: request `0c 02 03 01 00 06` on the
/// vendor collection (usage page 448, usage 1).
pub fn poll_headset(api: &HidApi) -> DeviceStatus {
    let request = [0x0c, 0x02, 0x03, 0x01, 0x00, 0x06];
    for info in api.device_list() {
        let matches_id = info.vendor_id() == VID_HP && info.product_id() == HEADSET_PID;
        let matches_name = info.vendor_id() == VID_HP && product_matches(info, "cloud iii s");
        if !matches_id && !matches_name {
            continue;
        }
        if info.usage_page() != HEADSET_USAGE_PAGE || info.usage() != HEADSET_USAGE {
            debug_log::log(format!(
                "headset: skipping non-matching collection usage_page={} usage={} (want {}/{})",
                info.usage_page(),
                info.usage(),
                HEADSET_USAGE_PAGE,
                HEADSET_USAGE
            ));
            continue;
        }
        let Ok(dev) = info.open_device(api) else {
            continue;
        };
        drain(&dev);
        debug_log::log_bytes("headset >>", &request);
        if dev.write(&request).is_err() {
            continue;
        }
        let mut resp = [0u8; 64];
        if let Ok(n) = dev.read_timeout(&mut resp, 1000) {
            if n > 0 {
                debug_log::log_bytes("headset <<", &resp[..n]);
            }
            if let Some(status) = parse_headset_response(&resp[..n]) {
                debug_log::log(format!("headset: parsed {status:?}"));
                return status;
            }
        }
    }
    DeviceStatus::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_on_battery() {
        let resp = [0x51, 0x02, 73, 0x00, 25, 0x00, 0x10, 0x0f];
        let status = parse_mouse_response(&resp).unwrap();
        assert!(status.connected);
        assert_eq!(status.battery_pct, Some(73));
        assert!(!status.charging);
    }

    #[test]
    fn mouse_charging() {
        let resp = [0x51, 0x02, 40, 0x01, 25, 0x00, 0x10, 0x0f];
        let status = parse_mouse_response(&resp).unwrap();
        assert_eq!(status.battery_pct, Some(40));
        assert!(status.charging, "state byte 0x01 must be reported as charging");
    }

    #[test]
    fn mouse_full_charge_counts_as_charging() {
        let resp = [0x51, 0x02, 100, 0x02, 25, 0x00, 0x00, 0x00];
        let status = parse_mouse_response(&resp).unwrap();
        assert!(status.charging, "state byte 0x02 (full/on USB power) must be reported as charging");
    }

    #[test]
    fn mouse_wrong_prefix_is_rejected() {
        // some other HID collection answering with an unrelated report
        let resp = [0x01, 0x02, 3, 4, 5, 6, 7, 8];
        assert!(parse_mouse_response(&resp).is_none());
    }

    #[test]
    fn mouse_too_short_is_rejected() {
        let resp = [0x51, 0x02, 50];
        assert!(parse_mouse_response(&resp).is_none());
    }

    #[test]
    fn mouse_invalid_percent_is_none_but_still_connected() {
        let resp = [0x51, 0x02, 0xff, 0x00, 0, 0, 0, 0];
        let status = parse_mouse_response(&resp).unwrap();
        assert!(status.connected);
        assert_eq!(status.battery_pct, None);
    }

    #[test]
    fn headset_battery_at_byte_6() {
        let mut resp = [0u8; 12];
        resp[6] = 62;
        let status = parse_headset_response(&resp).unwrap();
        assert_eq!(status.battery_pct, Some(62));
        assert!(
            !status.charging,
            "no charging bit is known for Cloud III S yet — must stay false, not guessed"
        );
    }

    #[test]
    fn headset_too_short_is_rejected() {
        let resp = [0u8; 6]; // exactly 6 bytes: no index 6 exists
        assert!(parse_headset_response(&resp).is_none());
    }
}
