//! The tray process: tray icon, global hotkeys and settings. It never opens windows or
//! touches the GPU — captures are handed to the helper process (see `helper`). Idle cost is
//! zero: `ControlFlow::Wait` sleeps until the OS delivers a hotkey or tray event.

use anyhow::Context as _;
use cf_core::capture_request::CaptureRequest;
use cf_core::config::Config;
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

/// Events forwarded into the winit loop from the hotkey and tray crates.
#[derive(Debug)]
pub enum AppEvent {
    Hotkey(GlobalHotKeyEvent),
    Menu(MenuEvent),
    TrayClick(TrayIconEvent),
}

pub fn run(paths: AppPaths) -> anyhow::Result<()> {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("creating the event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    forward_native_events(&event_loop.create_proxy());

    let mut app = App::new(paths);
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
}

impl App {
    fn new(paths: AppPaths) -> Self {
        let config = load_config(&paths.config_file);
        Self {
            paths,
            config,
            services: None,
            startup_error: None,
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) {
        match create_services() {
            Ok(services) => {
                self.services = Some(services);
                self.apply_hotkeys();
                tracing::info!("ready");
            }
            Err(err) => {
                self.startup_error = Some(err);
                event_loop.exit();
            }
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
            Action::OpenCapturesFolder => {
                let dir = self
                    .config
                    .output
                    .captures_dir(&self.paths.default_captures_dir);
                report(action, shell::open_folder(&dir));
            }
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
        if let Some(action) = self.resolve_action(&event) {
            self.perform(action, event_loop);
        }
    }
}

fn create_services() -> anyhow::Result<Services> {
    Ok(Services {
        tray: Tray::new().context("creating the tray icon")?,
        hotkeys: HotkeyService::new().context("creating the hotkey manager")?,
    })
}

fn report(action: Action, result: std::io::Result<()>) {
    if let Err(err) = result {
        tracing::warn!(?action, "action failed: {err}");
    }
}
