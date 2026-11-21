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
    /// An update was downloaded and can be installed.
    UpdateReady(updates::Ready),
}

pub fn run(paths: AppPaths) -> anyhow::Result<()> {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("creating the event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    forward_native_events(&event_loop.create_proxy());

    let mut app = App::new(paths, event_loop.create_proxy());
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
}

impl App {
    fn new(paths: AppPaths, proxy: EventLoopProxy<AppEvent>) -> Self {
        let config = load_config(&paths.config_file);
        Self {
            paths,
            config,
            services: None,
            startup_error: None,
            proxy,
            remote: None,
            update: None,
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
                }
                self.check_for_updates();
            }
            Err(err) => {
                self.startup_error = Some(err);
                event_loop.exit();
            }
        }
    }

    fn check_for_updates(&self) {
        let updates = &self.config.updates;
        let Some(repo) = updates.repo.as_deref().filter(|_| updates.check) else {
            return;
        };
        let proxy = self.proxy.clone();
        updates::check(repo, move |ready| {
            let _ = proxy.send_event(AppEvent::UpdateReady(ready));
        });
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
    }

    fn resolve_action(&self, event: &AppEvent) -> Option<Action> {
        let services = self.services.as_ref()?;
        match event {
            AppEvent::Hotkey(e) if e.state == HotKeyState::Pressed => {
                services.hotkeys.action_for(e.id)
            }
            AppEvent::Menu(e) => services.tray.action_for(&e.id),
            AppEvent::TrayClick(TrayIconEvent::Click {
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
            Action::OpenSettings => helper_client::request(CaptureRequest::Settings),
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
            AppEvent::Remote(code) => {
                if code == updates::QUIT_CODE {
                    tracing::info!("asked to quit");
                    event_loop.exit();
                } else if code == prefs::RELOAD_CODE {
                    tracing::debug!("settings changed in the settings window");
                    self.perform(Action::ReloadConfig, event_loop);
                }
                return;
            }
            AppEvent::UpdateReady(ready) => {
                if let Some(services) = self.services.as_mut() {
                    services.tray.offer_update(&ready.version);
                }
                self.update = Some(ready);
                return;
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
    })
}

fn report(action: Action, result: std::io::Result<()>) {
    if let Err(err) = result {
        tracing::warn!(?action, "action failed: {err}");
    }
}
