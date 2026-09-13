//! Persistent system tray icon: right-click for a menu (toggle overlay,
//! autostart, quit), double-click to toggle the overlay directly.

use resvg::tiny_skia;
use resvg::usvg::{self, TreeParsing};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

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
    _tray: TrayIcon,
    toggle_item: MenuItem,
    autostart_item: CheckMenuItem,
    quit_item: MenuItem,
}

impl TrayMenu {
    pub fn build(hidden: bool, autostart_enabled: bool) -> Self {
        let toggle_item = MenuItem::new(toggle_label(hidden), true, None);
        let autostart_item = CheckMenuItem::new("Lancer au démarrage", true, autostart_enabled, None);
        let quit_item = MenuItem::new("Quitter", true, None);

        let menu = Menu::new();
        let _ = menu.append(&toggle_item);
        let _ = menu.append(&autostart_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&quit_item);

        let tray = TrayIconBuilder::new()
            .with_icon(rasterize_tray_icon())
            .with_tooltip("HyperX Overlay")
            .with_menu(Box::new(menu))
            .build()
            .expect("failed to create tray icon");

        Self {
            _tray: tray,
            toggle_item,
            autostart_item,
            quit_item,
        }
    }

    pub fn set_hidden_label(&self, hidden: bool) {
        self.toggle_item.set_text(toggle_label(hidden));
    }

    pub fn set_autostart_checked(&self, checked: bool) {
        self.autostart_item.set_checked(checked);
    }

    /// Drains every pending tray/menu event this frame and reports the last
    /// meaningful action (there's realistically at most one per frame).
    pub fn poll(&self) -> Option<TrayAction> {
        let mut action = None;

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                action = Some(TrayAction::ToggleOverlay);
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == *self.toggle_item.id() {
                action = Some(TrayAction::ToggleOverlay);
            } else if event.id == *self.autostart_item.id() {
                action = Some(TrayAction::ToggleAutostart);
            } else if event.id == *self.quit_item.id() {
                action = Some(TrayAction::Quit);
            }
        }

        action
    }
}

fn toggle_label(hidden: bool) -> &'static str {
    if hidden {
        "Afficher l'overlay"
    } else {
        "Masquer l'overlay"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_reflects_hidden_state() {
        assert_eq!(toggle_label(true), "Afficher l'overlay");
        assert_eq!(toggle_label(false), "Masquer l'overlay");
    }
}
