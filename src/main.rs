#![windows_subsystem = "windows"]

mod autostart;
mod debug_log;
mod hid_status;
mod tray_ui;

use eframe::egui;
use hid_status::DeviceStatus;
use hidapi::HidApi;
use std::sync::{Arc, Mutex};
use std::time::Duration;
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

fn spawn_hid_poller(shared: Arc<Mutex<SharedState>>) {
    std::thread::spawn(move || {
        let mut api = match HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                eprintln!("hidapi init failed: {e}");
                return;
            }
        };
        loop {
            let _ = api.refresh_devices();
            hid_status::log_known_devices(&api);
            let mouse = hid_status::poll_mouse(&api);
            let headset = hid_status::poll_headset(&api);
            if let Ok(mut s) = shared.lock() {
                s.mouse = mouse;
                s.headset = headset;
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

struct OverlayApp {
    shared: Arc<Mutex<SharedState>>,
    tray_menu: tray_ui::TrayMenu,
    hidden: bool,
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        match self.tray_menu.poll() {
            Some(tray_ui::TrayAction::ToggleOverlay) => {
                self.hidden = !self.hidden;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(!self.hidden));
                self.tray_menu.set_hidden_label(self.hidden);
            }
            Some(tray_ui::TrayAction::ToggleAutostart) => {
                let enabled = !autostart::is_enabled();
                autostart::set_enabled(enabled);
                self.tray_menu.set_autostart_checked(enabled);
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
            self.hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.tray_menu.set_hidden_label(true);
        }

        // Keep polling (tray events, capslock) even while hidden and nothing is drawn.
        if self.hidden {
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
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
            let (text, color) = match (status.connected, status.battery_pct) {
                (true, Some(pct)) => {
                    let color = if status.charging {
                        egui::Color32::from_rgb(110, 180, 240)
                    } else if pct <= 15 {
                        egui::Color32::from_rgb(230, 90, 90)
                    } else if pct <= 30 {
                        egui::Color32::from_rgb(230, 180, 70)
                    } else {
                        egui::Color32::from_rgb(150, 220, 150)
                    };
                    (pct.to_string(), color)
                }
                (true, None) => ("?".to_string(), egui::Color32::from_gray(150)),
                (false, _) => ("--".to_string(), egui::Color32::from_gray(90)),
            };
            ui.label(egui::RichText::new(text).color(color).size(17.0).strong());
        });
    });
}

fn main() -> eframe::Result<()> {
    let shared = Arc::new(Mutex::new(SharedState::default()));
    spawn_hid_poller(shared.clone());

    // Must be created on this (main) thread: on Windows it relies on the same
    // Win32 message loop that winit/eframe pumps below to receive its events.
    let tray_menu = tray_ui::TrayMenu::build(false, autostart::is_enabled());

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
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(OverlayApp {
                shared,
                tray_menu,
                hidden: false,
            }))
        }),
    )
}
