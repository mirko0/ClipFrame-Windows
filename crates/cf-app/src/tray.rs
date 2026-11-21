//! The notification-area icon and its context menu.

use std::cell::RefCell;
use std::path::PathBuf;

use anyhow::Context as _;
use cf_core::capture_request::MAX_RECENT;
use cf_core::history;
use cf_core::hotkey::Hotkey;
use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::actions::Action;
use crate::capture::history as store;
use crate::tray_icon_image;

/// Recent captures listed in the menu.
const RECENT_SHOWN: usize = 10;

/// Menu layout after the "Recent captures" submenu; `None` is a separator.
const MENU_LAYOUT: &[Option<(&str, Action)>] = &[
    None,
    Some(("Settings…", Action::OpenSettings)),
    Some(("Open captures folder", Action::OpenCapturesFolder)),
    Some(("Edit settings file", Action::OpenConfig)),
    Some(("Reload settings", Action::ReloadConfig)),
    Some(("Open logs folder", Action::OpenLogs)),
    None,
    Some(("Quit ClipFrame", Action::Quit)),
];

thread_local! {
    /// The "Recent captures" submenu. Tray events arrive synchronously on this (the main)
    /// thread just before the menu opens, which is when it is refreshed.
    static RECENT: RefCell<Option<RecentMenu>> = const { RefCell::new(None) };
}

pub struct Tray {
    icon: TrayIcon,
    menu: Menu,
    items: Vec<(MenuItem, Action)>,
}

impl Tray {
    pub fn new(history_dir: PathBuf) -> anyhow::Result<Self> {
        let menu = Menu::new();
        let mut items = Vec::new();
        let capture = MenuItem::new("Capture", true, None);
        menu.append(&capture)?;
        items.push((capture, Action::Capture));
        let recent = Submenu::new("Recent captures", true);
        menu.append(&recent)?;
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
            .with_menu(Box::new(menu.clone()))
            .with_menu_on_left_click(false)
            .with_tooltip("ClipFrame")
            .with_icon(image)
            .build()?;
        RECENT.with(|cell| {
            *cell.borrow_mut() = Some(RecentMenu {
                submenu: recent,
                dir: history_dir,
                items: Vec::new(),
            });
        });
        refresh_recent();
        Ok(Self { icon, menu, items })
    }

    pub fn action_for(&self, id: &MenuId) -> Option<Action> {
        let fixed = self
            .items
            .iter()
            .find(|(item, _)| item.id() == id)
            .map(|(_, action)| *action);
        fixed.or_else(|| {
            RECENT.with(|cell| cell.borrow().as_ref().and_then(|menu| menu.action_for(id)))
        })
    }

    /// Adds "Restart to install update" at the top of the menu (once).
    pub fn offer_update(&mut self, version: &str) {
        if self.items.iter().any(|(_, a)| *a == Action::InstallUpdate) {
            return;
        }
        let item = MenuItem::new(
            format!("Restart to install ClipFrame {version}"),
            true,
            None,
        );
        if let Err(err) = self.menu.prepend(&item) {
            tracing::warn!("could not offer the update in the menu: {err}");
            return;
        }
        self.items.push((item, Action::InstallUpdate));
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

/// Re-reads the history folder into the "Recent captures" submenu (call right before the
/// menu opens; only lists file names and reads 24-byte PNG headers).
pub fn refresh_recent() {
    RECENT.with(|cell| {
        if let Some(menu) = cell.borrow_mut().as_mut() {
            if let Err(err) = menu.refresh() {
                tracing::warn!("could not list recent captures: {err}");
            }
        }
    });
}

struct RecentMenu {
    submenu: Submenu,
    dir: PathBuf,
    /// Current entries and the request index each one opens.
    items: Vec<(MenuItem, Option<u8>)>,
}

impl RecentMenu {
    fn refresh(&mut self) -> anyhow::Result<()> {
        for (item, _) in self.items.drain(..) {
            self.submenu.remove(&item)?;
        }
        let now = cf_platform::time::local_now();
        let shown = RECENT_SHOWN.min(usize::from(MAX_RECENT));
        for (index, entry) in store::entries(&self.dir).iter().take(shown).enumerate() {
            let label = history::label(entry, now, store::size_of(entry));
            let item = MenuItem::new(label, true, None);
            self.submenu.append(&item)?;
            self.items.push((item, u8::try_from(index).ok()));
        }
        if self.items.is_empty() {
            let item = MenuItem::new("No recent captures", false, None);
            self.submenu.append(&item)?;
            self.items.push((item, None));
        }
        Ok(())
    }

    fn action_for(&self, id: &MenuId) -> Option<Action> {
        self.items
            .iter()
            .find(|(item, _)| item.id() == id)
            .and_then(|(_, index)| index.map(Action::OpenRecent))
    }
}
