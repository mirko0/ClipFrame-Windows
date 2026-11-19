//! The notification-area icon and its context menu.

use anyhow::Context as _;
use cf_core::hotkey::Hotkey;
use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::actions::Action;
use crate::tray_icon_image;

/// Menu layout; `None` is a separator.
const MENU_LAYOUT: &[Option<(&str, Action)>] = &[
    Some(("Capture", Action::Capture)),
    None,
    Some(("Open captures folder", Action::OpenCapturesFolder)),
    Some(("Edit settings file", Action::OpenConfig)),
    Some(("Reload settings", Action::ReloadConfig)),
    Some(("Open logs folder", Action::OpenLogs)),
    None,
    Some(("Quit ClipFrame", Action::Quit)),
];

pub struct Tray {
    icon: TrayIcon,
    items: Vec<(MenuItem, Action)>,
}

impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        let menu = Menu::new();
        let mut items = Vec::new();
        for entry in MENU_LAYOUT {
            if let Some((label, action)) = entry {
                let item = MenuItem::new(*label, true, None);
                menu.append(&item)?;
                items.push((item, *action));
            } else {
                menu.append(&PredefinedMenuItem::separator())?;
            }
        }
        let size = u32::try_from(tray_icon_image::SIZE).context("icon size")?;
        let image = Icon::from_rgba(tray_icon_image::rgba(), size, size)?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("ClipFrame")
            .with_icon(image)
            .build()?;
        Ok(Self { icon, items })
    }

    pub fn action_for(&self, id: &MenuId) -> Option<Action> {
        self.items
            .iter()
            .find(|(item, _)| item.id() == id)
            .map(|(_, action)| *action)
    }

    /// Shows the capture hotkey in the menu and tooltip, flagging it when registration failed.
    pub fn show_capture_hotkey(&self, hotkey: Hotkey, registered: bool) {
        let (menu_text, tooltip) = if registered {
            (
                format!("Capture\t{hotkey}"),
                format!("ClipFrame — {hotkey}"),
            )
        } else {
            (
                format!("Capture\t{hotkey} (unavailable)"),
                format!("ClipFrame — {hotkey} is taken by another app"),
            )
        };
        if let Some((item, _)) = self.items.iter().find(|(_, a)| *a == Action::Capture) {
            item.set_text(menu_text);
        }
        if let Err(err) = self.icon.set_tooltip(Some(tooltip)) {
            tracing::warn!("could not update tray tooltip: {err}");
        }
    }
}
