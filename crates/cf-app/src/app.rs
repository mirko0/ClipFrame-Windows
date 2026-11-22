//! The tray process: tray icon, global hotkeys and settings. It never opens windows or
//! touches the GPU — captures are handed to the helper process (see `helper`). Idle cost is
//! zero: `ControlFlow::Wait` sleeps until the OS delivers a hotkey or tray event.

use anyhow::Context as _;
use cf_core::capture_request::CaptureRequest;
use cf_core::config::Config;
use cf_core::state::AppState;
use cf_platform::ipc::{self, RequestWindow};
use cf_platform::paths::AppPaths;
use cf_platform::shell;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::actions::Action;
use crate::helper_client;
use crate::hotkeys::HotkeyService;
use crate::settings::load_config;
use crate::tray::Tray;
use crate::{prefs, updates};

/// Events forwarded into the winit loop from the hotkey and tray crates.
#[derive(Debug)]
pub enum AppEvent {
    Hotkey(GlobalHotKeyEvent),
    Menu(MenuEvent),
    TrayClick(TrayIconEvent),
    /// A code posted by the capture helper (e.g. "settings changed").
    Remote(u32),
    /// Win+Shift+S, caught instead of the Snipping Tool.
    Snip,
    /// An update check finished; `manual` when asked for from the tray menu.
    UpdateChecked {
        outcome: updates::Outcome,
        manual: bool,
    },
}

/// Message code: open the main window (ClipFrame was started again).
pub const SHOW_HOME_CODE: u32 = 5;

pub fn run(paths: AppPaths, background: bool) -> anyhow::Result<()> {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("creating the event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    forward_native_events(&event_loop.create_proxy());

    let mut app = App::new(paths, event_loop.create_proxy(), background);
    event_loop
        .run_app(&mut app)
        .context("running the event loop")?;
    app.startup_error.map_or(Ok(()), Err)
}

/// Wakes the event loop only for events we act on (tray hover/move events are dropped here).
fn forward_native_events(proxy: &EventLoopProxy<AppEvent>) {
    // `send_event` only fails once the loop has exited, when the event no longer matters.
    let hotkey_proxy = proxy.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        let _ = hotkey_proxy.send_event(AppEvent::Hotkey(event));
    }));
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(AppEvent::Menu(event));
    }));
    let tray_proxy = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        // Sent synchronously just before the context menu opens: list recent captures now.
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Right,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            crate::tray::refresh_recent();
        }
        if matches!(event, TrayIconEvent::Click { .. }) {
            let _ = tray_proxy.send_event(AppEvent::TrayClick(event));
        }
    }));
}

struct Services {
    tray: Tray,
    hotkeys: HotkeyService,
    /// Catches Win+Shift+S while "Use Win+Shift+S" is on.
    snipping: Option<cf_platform::snipping::Takeover>,
}

struct App {
    paths: AppPaths,
    config: Config,
    services: Option<Services>,
    startup_error: Option<anyhow::Error>,
    proxy: EventLoopProxy<AppEvent>,
    /// Receives codes from the capture helper.
    remote: Option<RequestWindow>,
    update: Option<updates::Ready>,
    /// Started without opening the main window (`--background`).
    background: bool,
}

impl App {
    fn new(paths: AppPaths, proxy: EventLoopProxy<AppEvent>, background: bool) -> Self {
        let config = load_config(&paths.config_file);
        Self {
            paths,
            config,
            services: None,
            startup_error: None,
            proxy,
            remote: None,
            update: None,
            background,
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) {
        match create_services(&self.paths) {
            Ok(services) => {
                self.services = Some(services);
                self.apply_hotkeys();
                self.listen_to_helper();
                tracing::info!("ready");
                if !AppState::load(&self.paths.state_file).onboarded {
                    helper_client::request(CaptureRequest::Onboarding);
                } else if !self.background {
                    helper_client::request(CaptureRequest::Home);
                }
                let exe = std::env::current_exe().unwrap_or_default();
                cf_platform::startup::refresh(&exe);
                crate::settings::clean_up(&self.config, &self.paths);
                self.check_for_updates(false);
            }
            Err(err) => {
                self.startup_error = Some(err);
                event_loop.exit();
            }
        }
    }

    /// Checks for an update in the background: at start-up when `updates.check` is on, or
    /// on demand (`manual`), which reports what it found.
    fn check_for_updates(&self, manual: bool) {
        let updates = &self.config.updates;
        if let (true, Some(ready)) = (manual, &self.update) {
            let title = format!("ClipFrame {} is ready", ready.version);
            return notify(title, RESTART_HINT.into());
        }
        let Some(repo) = updates.repo.as_deref().filter(|_| updates.check || manual) else {
            if manual {
                notify(
                    "Updates are off".into(),
                    "Set updates.repo in the settings file.".into(),
                );
            }
            return;
        };
        let proxy = self.proxy.clone();
        updates::check(repo, move |outcome| {
            let _ = proxy.send_event(AppEvent::UpdateChecked { outcome, manual });
        });
    }

    fn on_update_checked(&mut self, outcome: updates::Outcome, manual: bool) {
        let (title, body) = match outcome {
            updates::Outcome::Ready(ready) => {
                if let Some(services) = self.services.as_mut() {
                    services.tray.offer_update(&ready.version);
                }
                let title = format!("ClipFrame {} is ready", ready.version);
                self.update = Some(ready);
                (title, RESTART_HINT.to_owned())
            }
            updates::Outcome::UpToDate => (
                "ClipFrame is up to date".to_owned(),
                format!("Version {}", env!("CARGO_PKG_VERSION")),
            ),
            updates::Outcome::NotInstalled => (
                "Can't update this copy".to_owned(),
                "Only copies installed with the ClipFrame installer update themselves.".to_owned(),
            ),
            updates::Outcome::Failed(err) => ("Could not check for updates".to_owned(), err),
        };
        if manual {
            notify(title, body);
        }
    }

    fn install_update(&self, event_loop: &ActiveEventLoop) {
        let (Some(ready), Some(repo)) = (&self.update, self.config.updates.repo.as_deref()) else {
            return;
        };
        match updates::install_and_restart(repo, ready) {
            Ok(()) => event_loop.exit(),
            Err(err) => tracing::error!("could not install the update: {err:#}"),
        }
    }

    /// A code posted by the capture helper or the installer.
    fn on_remote(&mut self, code: u32, event_loop: &ActiveEventLoop) {
        match code {
            updates::QUIT_CODE => {
                tracing::info!("asked to quit");
                event_loop.exit();
            }
            prefs::PAUSE_HOTKEYS_CODE => {
                if let Some(services) = self.services.as_mut() {
                    services.hotkeys.suspend();
                }
                self.sync_snipping(false);
            }
            prefs::RESUME_HOTKEYS_CODE => self.apply_hotkeys(),
            SHOW_HOME_CODE => helper_client::request(CaptureRequest::Home),
            updates::CHECK_CODE => self.check_for_updates(true),
            prefs::RELOAD_CODE => {
                tracing::debug!("settings changed in the settings window");
                self.perform(Action::ReloadConfig, event_loop);
            }
            _ => {}
        }
    }

    fn listen_to_helper(&mut self) {
        let proxy = self.proxy.clone();
        let window = RequestWindow::create(ipc::Channel::Tray, move |code| {
            let _ = proxy.send_event(AppEvent::Remote(code));
        });
        match window {
            Ok(window) => self.remote = Some(window),
            Err(err) => tracing::warn!("settings changes won't apply until restart: {err}"),
        }
    }

    fn apply_hotkeys(&mut self) {
        let Some(services) = self.services.as_mut() else {
            return;
        };
        let failures = services.hotkeys.apply(&self.config.hotkeys);
        for failure in &failures {
            tracing::warn!(
                hotkey = %failure.hotkey, action = ?failure.action,
                "could not register hotkey: {}", failure.reason
            );
        }
        let capture_ok = !failures.iter().any(|f| f.action == Action::Capture);
        services
            .tray
            .show_capture_hotkey(self.config.hotkeys.capture, capture_ok);
        self.sync_snipping(self.config.hotkeys.replace_snipping_tool);
    }

    /// Starts or stops catching Win+Shift+S.
    fn sync_snipping(&mut self, on: bool) {
        let Some(services) = self.services.as_mut() else {
            return;
        };
        if !on {
            services.snipping = None;
            return;
        }
        if services.snipping.is_some() {
            return;
        }
        let proxy = std::sync::Mutex::new(self.proxy.clone());
        let started = cf_platform::snipping::Takeover::start(move || {
            if let Ok(proxy) = proxy.lock() {
                let _ = proxy.send_event(AppEvent::Snip);
            }
        });
        match started {
            Ok(takeover) => services.snipping = Some(takeover),
            Err(err) => tracing::warn!("could not take over Win+Shift+S: {err}"),
        }
    }

    fn resolve_action(&self, event: &AppEvent) -> Option<Action> {
        let services = self.services.as_ref()?;
        match event {
            AppEvent::Hotkey(e) if e.state == HotKeyState::Pressed => {
                services.hotkeys.action_for(e.id)
            }
            AppEvent::Menu(e) => services.tray.action_for(&e.id),
            AppEvent::Snip
            | AppEvent::TrayClick(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }) => Some(Action::Capture),
            _ => None,
        }
    }

    fn perform(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        tracing::debug!(?action, "performing action");
        match action {
            Action::Capture => helper_client::request(CaptureRequest::Interactive),
            Action::RepeatLast => helper_client::request(CaptureRequest::RepeatLast),
            Action::OpenRecent(index) => {
                helper_client::request(CaptureRequest::OpenRecent(index));
            }
            Action::OpenCapturesFolder => {
                let dir = self
                    .config
                    .output
                    .captures_dir(&self.paths.default_captures_dir);
                report(action, shell::open_folder(&dir));
            }
            Action::OpenHome => helper_client::request(CaptureRequest::Home),
            Action::OpenSettings => helper_client::request(CaptureRequest::Settings),
            Action::CheckForUpdates => self.check_for_updates(true),
            Action::InstallUpdate => self.install_update(event_loop),
            Action::OpenConfig => {
                report(action, shell::open_in_text_editor(&self.paths.config_file));
            }
            Action::OpenLogs => report(action, shell::open_folder(&self.paths.log_dir)),
            Action::ReloadConfig => {
                self.config = load_config(&self.paths.config_file);
                self.apply_hotkeys();
            }
            Action::Quit => event_loop.exit(),
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::Init) {
            self.start(event_loop);
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Remote(code) => return self.on_remote(code, event_loop),
            AppEvent::UpdateChecked { outcome, manual } => {
                return self.on_update_checked(outcome, manual);
            }
            _ => {}
        }
        if let Some(action) = self.resolve_action(&event) {
            self.perform(action, event_loop);
        }
    }
}

fn create_services(paths: &AppPaths) -> anyhow::Result<Services> {
    Ok(Services {
        tray: Tray::new(paths.history_dir.clone()).context("creating the tray icon")?,
        hotkeys: HotkeyService::new().context("creating the hotkey manager")?,
        snipping: None,
    })
}

fn report(action: Action, result: std::io::Result<()>) {
    if let Err(err) = result {
        tracing::warn!(?action, "action failed: {err}");
    }
}

/// How to install a downloaded update.
const RESTART_HINT: &str = "Choose “Restart to install” in the tray menu.";

/// Shows a notification without blocking the tray (WinRT runs on a worker thread).
fn notify(title: String, body: String) {
    let shown = std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            let toast = cf_platform::toast::Toast {
                title: &title,
                body: &body,
                ..Default::default()
            };
            if let Err(err) = cf_platform::toast::show(&toast) {
                tracing::warn!("could not show the notification: {err}");
            }
        });
    if let Err(err) = shown {
        tracing::warn!("could not show the notification: {err}");
    }
}
