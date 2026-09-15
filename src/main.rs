#![windows_subsystem = "windows"]

mod autostart;
mod debug_log;
mod hid_status;
mod tray_ui;

use eframe::egui;
use hid_status::DeviceStatus;
use hidapi::HidApi;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};

#[derive(Default)]
struct SharedState {
    mouse: DeviceStatus,
    headset: DeviceStatus,
}

fn caps_lock_on() -> bool {
    unsafe { (GetKeyState(VK_CAPITAL.0 as i32) & 1) != 0 }
}

const HID_ATTEMPTS: usize = 3;
const HID_RETRY_DELAY: Duration = Duration::from_millis(750);
const MOUSE_CHARGING_INTERVAL: Duration = Duration::from_secs(10);
const MOUSE_ACTIVE_INTERVAL: Duration = Duration::from_secs(15);
const MOUSE_SLEEP_INTERVAL: Duration = Duration::from_secs(60);
const HEADSET_INTERVAL: Duration = Duration::from_secs(30);

fn poll_with_retries(mut poll: impl FnMut() -> DeviceStatus) -> DeviceStatus {
    for attempt in 1..=HID_ATTEMPTS {
        let status = poll();
        if status.connected || attempt == HID_ATTEMPTS {
            return status;
        }
        std::thread::sleep(HID_RETRY_DELAY);
    }
    unreachable!("the final HID attempt always returns")
}

fn preserve_recent_status(
    fresh: DeviceStatus,
    cached: &mut Option<(DeviceStatus, Instant)>,
    now: Instant,
    sleeping_device_present: bool,
) -> DeviceStatus {
    if fresh.connected {
        *cached = Some((fresh, now));
        return fresh;
    }

    if sleeping_device_present {
        if let Some((last, _seen_at)) = cached {
            debug_log::log("mouse HID read failed while dongle remains present; reporting sleep");
            return DeviceStatus {
                sleeping: true,
                ..*last
            };
        }
        return DeviceStatus {
            connected: true,
            sleeping: true,
            ..DeviceStatus::default()
        };
    }
    DeviceStatus::default()
}

fn mouse_poll_interval(status: DeviceStatus) -> Duration {
    if status.sleeping {
        MOUSE_SLEEP_INTERVAL
    } else if status.charging {
        MOUSE_CHARGING_INTERVAL
    } else {
        MOUSE_ACTIVE_INTERVAL
    }
}

fn spawn_hid_poller(shared: Arc<Mutex<SharedState>>) {
    std::thread::spawn(move || {
        let mut api = match HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                eprintln!("hidapi init failed: {e}");
                return;
            }
        };
        let mut last_mouse = None;
        let mut last_headset = None;
        let mut next_mouse_poll = Instant::now();
        let mut next_headset_poll = Instant::now();
        loop {
            let now = Instant::now();
            if now >= next_mouse_poll {
                let _ = api.refresh_devices();
                hid_status::log_known_devices(&api);
                let mouse = preserve_recent_status(
                    poll_with_retries(|| hid_status::poll_mouse(&api)),
                    &mut last_mouse,
                    now,
                    hid_status::mouse_status_collection_present(&api),
                );
                next_mouse_poll = Instant::now() + mouse_poll_interval(mouse);
                if let Ok(mut s) = shared.lock() {
                    s.mouse = mouse;
                }
            }

            let now = Instant::now();
            if now >= next_headset_poll {
                let _ = api.refresh_devices();
                let headset = preserve_recent_status(
                    poll_with_retries(|| hid_status::poll_headset(&api)),
                    &mut last_headset,
                    now,
                    false,
                );
                next_headset_poll = Instant::now() + HEADSET_INTERVAL;
                if let Ok(mut s) = shared.lock() {
                    s.headset = headset;
                }
            }

            let wake_at = next_mouse_poll.min(next_headset_poll);
            std::thread::sleep(wake_at.saturating_duration_since(Instant::now()));
        }
    });
}

/// Parked position while "hidden". We deliberately do NOT use
/// `ViewportCommand::Visible(false)` to hide the overlay: on Windows, an
/// invisible window stops receiving `WM_PAINT`, and eframe's repaint loop is
/// driven by that — so the app would never wake up again to notice a tray
/// click asking to bring it back. Moving the window off-screen instead keeps
/// it "visible" as far as Windows is concerned (repaints keep flowing), while
/// being just as invisible to the user.
const OFFSCREEN_POS: egui::Pos2 = egui::pos2(-32000.0, -32000.0);

struct OverlayApp {
    shared: Arc<Mutex<SharedState>>,
    tray_menu: tray_ui::TrayMenu,
    hidden: bool,
    shown_pos: egui::Pos2,
}

impl OverlayApp {
    fn set_hidden(&mut self, ctx: &egui::Context, hidden: bool) {
        if hidden == self.hidden {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(if hidden {
            OFFSCREEN_POS
        } else {
            self.shown_pos
        }));
        self.hidden = hidden;
    }
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Remember the last on-screen position (e.g. after a drag) so
        // "Afficher" restores it there instead of the original spawn spot.
        if !self.hidden {
            if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
                self.shown_pos = rect.min;
            }
        }

        match self.tray_menu.poll() {
            Some(tray_ui::TrayAction::ToggleOverlay) => {
                let now_hidden = !self.hidden;
                self.set_hidden(ctx, now_hidden);
            }
            Some(tray_ui::TrayAction::ToggleAutostart) => {
                let requested = !autostart::is_enabled();
                let _ = autostart::set_enabled(requested);
                // Reflect the registry's actual state, not merely the state
                // we attempted to write (which can fail on locked-down PCs).
            }
            Some(tray_ui::TrayAction::Quit) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }

        // Alt+Click anywhere on the overlay: hide it.
        let alt_click = ctx.input(|i| {
            i.modifiers.alt && i.pointer.button_clicked(egui::PointerButton::Primary)
        });
        if alt_click && !self.hidden {
            self.set_hidden(ctx, true);
        }

        // Keep drawing (off-screen while hidden) rather than skip the frame:
        // this keeps eframe's normal repaint/viewport-info flow, which is
        // exactly what OFFSCREEN_POS relies on. See its doc comment.
        if self.hidden {
            debug_log::log("tick (hidden, off-screen)");
        }
        let caps = caps_lock_on();
        let (mouse, headset) = match self.shared.lock() {
            Ok(s) => (s.mouse, s.headset),
            Err(_) => (DeviceStatus::default(), DeviceStatus::default()),
        };

        let panel_frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgba_unmultiplied(18, 18, 22, 235))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::same(12));

        let panel_response = egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                row_caps(ui, caps);
                ui.separator();
                row_battery(ui, egui::include_image!("assets/mouse.svg"), "Saga Pro", &mouse);
                row_battery(ui, egui::include_image!("assets/headset.svg"), "Cloud III S", &headset);
            })
            .response;

        if panel_response.interact(egui::Sense::drag()).dragged() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

const ICON_SIZE: f32 = 22.0;

fn row_caps(ui: &mut egui::Ui, on: bool) {
    ui.horizontal(|ui| {
        let (icon, tint): (egui::ImageSource, egui::Color32) = if on {
            (
                egui::include_image!("assets/capslock_on.svg"),
                egui::Color32::from_rgb(90, 220, 120),
            )
        } else {
            (
                egui::include_image!("assets/capslock_off.svg"),
                egui::Color32::from_gray(130),
            )
        };
        ui.add(
            egui::Image::new(icon)
                .tint(tint)
                .fit_to_exact_size(egui::vec2(ICON_SIZE, ICON_SIZE)),
        );
        ui.label(egui::RichText::new("Verr. Maj").color(egui::Color32::WHITE));
    });
}

fn row_battery(ui: &mut egui::Ui, icon: egui::ImageSource, name: &str, status: &DeviceStatus) {
    ui.horizontal(|ui| {
        ui.add(
            egui::Image::new(icon)
                .tint(egui::Color32::from_gray(225))
                .fit_to_exact_size(egui::vec2(ICON_SIZE, ICON_SIZE)),
        );
        ui.label(egui::RichText::new(name).color(egui::Color32::WHITE));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (text, color) = battery_display(status);
            ui.label(egui::RichText::new(text).color(color).size(17.0).strong());
        });
    });
}

fn battery_display(status: &DeviceStatus) -> (String, egui::Color32) {
    if status.sleeping {
        let text = match status.battery_pct {
            Some(pct) => format!("{pct} % · Veille"),
            None => "Veille".to_string(),
        };
        return (text, egui::Color32::from_gray(150));
    }
    match (status.connected, status.battery_pct) {
        (true, Some(pct)) => {
            // Green deliberately means external power / charging. A normal,
            // sufficiently charged wireless device remains neutral.
            let color = if status.charging {
                egui::Color32::from_rgb(90, 220, 120)
            } else if pct <= 15 {
                egui::Color32::from_rgb(230, 90, 90)
            } else if pct <= 30 {
                egui::Color32::from_rgb(230, 180, 70)
            } else {
                egui::Color32::from_gray(225)
            };
            (pct.to_string(), color)
        }
        (true, None) => ("?".to_string(), egui::Color32::from_gray(150)),
        (false, _) => ("--".to_string(), egui::Color32::from_gray(90)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_battery_text_means_charging() {
        let on_battery = DeviceStatus {
            connected: true,
            battery_pct: Some(80),
            charging: false,
            sleeping: false,
        };
        let charging = DeviceStatus {
            charging: true,
            ..on_battery
        };

        assert_eq!(battery_display(&on_battery).1, egui::Color32::from_gray(225));
        assert_eq!(battery_display(&charging).1, egui::Color32::from_rgb(90, 220, 120));
    }

    #[test]
    fn recent_battery_value_survives_a_transient_hid_failure() {
        let valid = DeviceStatus {
            connected: true,
            battery_pct: Some(64),
            charging: true,
            sleeping: false,
        };
        let now = Instant::now();
        let mut cached = None;
        assert_eq!(
            preserve_recent_status(valid, &mut cached, now, false).battery_pct,
            Some(64)
        );

        let temporary_failure = DeviceStatus::default();
        let kept = preserve_recent_status(
            temporary_failure,
            &mut cached,
            now + Duration::from_secs(10),
            true,
        );
        assert_eq!(kept.battery_pct, Some(64));
        assert!(kept.sleeping);

        let still_sleeping = preserve_recent_status(
            temporary_failure,
            &mut cached,
            now + Duration::from_secs(300),
            true,
        );
        assert_eq!(still_sleeping.battery_pct, Some(64));

        let disconnected = preserve_recent_status(
            temporary_failure,
            &mut cached,
            now + Duration::from_secs(301),
            false,
        );
        assert!(!disconnected.connected);
    }

    #[test]
    fn sleeping_mouse_is_labeled_instead_of_disconnected() {
        let status = DeviceStatus {
            connected: true,
            battery_pct: Some(64),
            charging: false,
            sleeping: true,
        };
        assert_eq!(battery_display(&status).0, "64 % · Veille");
        assert_eq!(battery_display(&status).1, egui::Color32::from_gray(150));
    }

    #[test]
    fn polling_interval_adapts_to_mouse_state() {
        let active = DeviceStatus {
            connected: true,
            battery_pct: Some(80),
            charging: false,
            sleeping: false,
        };
        assert_eq!(mouse_poll_interval(active), MOUSE_ACTIVE_INTERVAL);
        assert_eq!(
            mouse_poll_interval(DeviceStatus { charging: true, ..active }),
            MOUSE_CHARGING_INTERVAL
        );
        assert_eq!(
            mouse_poll_interval(DeviceStatus { sleeping: true, ..active }),
            MOUSE_SLEEP_INTERVAL
        );
    }
}

fn main() -> eframe::Result<()> {
    let shared = Arc::new(Mutex::new(SharedState::default()));
    spawn_hid_poller(shared.clone());

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let win_w = 220.0;
    let margin = 16.0;
    let pos_x = if screen_w > 0 {
        (screen_w as f32) - win_w - margin
    } else {
        1600.0
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([win_w, 130.0])
            .with_position([pos_x, margin])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false),
        ..Default::default()
    };

    eframe::run_native(
        "HyperX Overlay",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            // Built here (inside the eframe/winit event loop setup), not earlier
            // in `main`: on Windows the tray/menu event pump must run on the
            // same thread as the actual winit event loop, and creating it too
            // early risked landing on the wrong one — matching tray-icon's own
            // documented eframe integration example.
            let tray_menu = tray_ui::TrayMenu::build(autostart::is_enabled());
            Ok(Box::new(OverlayApp {
                shared,
                tray_menu,
                hidden: false,
                shown_pos: egui::pos2(pos_x, margin),
            }))
        }),
    )
}
