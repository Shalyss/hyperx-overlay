//! Native Windows system tray running on its own message-loop thread.
//!
//! `eframe` owns its winit event loop and does not expose the event-loop proxy
//! required by `tray-icon`. Keeping the tray's hidden window and its Win32
//! message dispatch here makes menu commands reliable even while the overlay
//! is unfocused or parked off-screen.

use resvg::tiny_skia;
use resvg::usvg::{self, TreeParsing};
use std::sync::mpsc::{self, Receiver};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG};

const TRAY_SVG: &[u8] = include_bytes!("assets/tray.svg");

fn rasterize_tray_icon() -> Icon {
    let opt = usvg::Options::default();
    let usvg_tree = usvg::Tree::from_data(TRAY_SVG, &opt).expect("invalid tray icon svg");
    let size = usvg_tree.size.to_int_size();
    let (w, h) = (size.width().max(1), size.height().max(1));
    let render_tree = resvg::Tree::from_usvg(&usvg_tree);

    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("tray icon pixmap alloc");
    render_tree.render(tiny_skia::Transform::default(), &mut pixmap.as_mut());

    // tiny_skia stores premultiplied alpha; Icon::from_rgba wants straight alpha.
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 {
            px[0] = ((px[0] as u32 * 255) / a).min(255) as u8;
            px[1] = ((px[1] as u32 * 255) / a).min(255) as u8;
            px[2] = ((px[2] as u32 * 255) / a).min(255) as u8;
        }
    }

    Icon::from_rgba(rgba, w, h).expect("tray icon from rgba")
}

pub enum TrayAction {
    ToggleOverlay,
    ToggleAutostart,
    Quit,
}

pub struct TrayMenu {
    actions: Receiver<TrayAction>,
}

impl TrayMenu {
    pub fn build(autostart_enabled: bool) -> Self {
        let (sender, actions) = mpsc::channel();
        std::thread::spawn(move || run_tray(sender, autostart_enabled));
        Self { actions }
    }

    /// Drain the actions emitted by the tray's independent Win32 loop.
    pub fn poll(&self) -> Option<TrayAction> {
        let mut action = None;
        while let Ok(next) = self.actions.try_recv() {
            action = Some(next);
        }
        action
    }
}

fn run_tray(sender: mpsc::Sender<TrayAction>, autostart_enabled: bool) {
    let toggle_item = MenuItem::new("Afficher / masquer l'overlay", true, None);
    let autostart_item = CheckMenuItem::new("Lancer au démarrage", true, autostart_enabled, None);
    let quit_item = MenuItem::new("Quitter", true, None);

    let menu = Menu::new();
    let _ = menu.append(&toggle_item);
    let _ = menu.append(&autostart_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);

    // Menu items themselves are thread-affine, but their IDs are safe to
    // capture in tray-icon's global, Send + Sync event callback.
    let toggle_id = toggle_item.id().clone();
    let autostart_id = autostart_item.id().clone();
    let quit_id = quit_item.id().clone();

    let tray_sender = sender.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        crate::debug_log::log(format!("tray event: {event:?}"));
        if matches!(event, TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }) {
            let _ = tray_sender.send(TrayAction::ToggleOverlay);
        }
    }));

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        crate::debug_log::log(format!("menu event: id={:?}", event.id));
        let action = if event.id == toggle_id {
            Some(TrayAction::ToggleOverlay)
        } else if event.id == autostart_id {
            Some(TrayAction::ToggleAutostart)
        } else if event.id == quit_id {
            Some(TrayAction::Quit)
        } else {
            None
        };
        if let Some(action) = action {
            let _ = sender.send(action);
        }
    }));

    let tray = TrayIconBuilder::new()
        .with_icon(rasterize_tray_icon())
        .with_tooltip("HyperX Overlay")
        .with_menu(Box::new(menu))
        .build()
        .expect("failed to create tray icon");
    crate::debug_log::log(format!("tray icon rect: {:?}", tray.rect()));

    // The icon owns a hidden Win32 window on this thread. Dispatch its
    // messages here for the entire life of the application.
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    drop(tray);
}
