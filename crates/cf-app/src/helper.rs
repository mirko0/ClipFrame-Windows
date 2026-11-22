//! The capture helper process (`clipframe.exe --capture-helper <request>`).
//!
//! Everything that touches the GPU runs here, never in the tray process: once a GPU driver is
//! loaded it keeps tens of MB in its process even after the device is released. The helper
//! serves requests while the user is capturing, stays for the "Keep GPU ready" period so the
//! next capture is fast, then exits — returning all of that memory to the system.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Context as _;
use cf_core::capture_request::CaptureRequest;
use cf_core::config::Config;
use cf_core::encode;
use cf_core::image::Image;
use cf_core::state::AppState;
use cf_platform::ipc::{self, RequestWindow};
use cf_platform::paths::AppPaths;
use cf_platform::single_instance::{InstanceError, SingleInstance};
use cf_render::KeepAlive;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::capture::{CaptureController, Delivery, Waker};
use crate::prefs::{self, Mode, PrefsOutcome, PrefsWindow};
use crate::{settings, tray_icon_image};

/// Command-line flag that starts the helper instead of the tray.
pub const FLAG: &str = "--capture-helper";
const INSTANCE_NAME: &str = r"Local\ClipFrame.CaptureHelper";

#[derive(Debug)]
enum HelperEvent {
    Request(CaptureRequest),
    /// A background lookup for the overlay finished.
    Wake,
}

pub fn run(paths: AppPaths, initial: CaptureRequest) -> anyhow::Result<()> {
    let instance = match SingleInstance::acquire(INSTANCE_NAME) {
        Ok(instance) => instance,
        Err(InstanceError::AlreadyRunning) => {
            // Lost a start-up race against another helper: hand it the request instead.
            if ipc::post_request(ipc::Channel::Helper, initial.code()) == ipc::Posted::NoHelper {
                tracing::warn!(?initial, "another helper is starting; request dropped");
            }
            return Ok(());
        }
        Err(err) => return Err(err).context("checking for a running capture helper"),
    };
    tracing::info!(?initial, "capture helper started");
    let event_loop = EventLoop::<HelperEvent>::with_user_event()
        .build()
        .context("creating the event loop")?;
    let mut helper = Helper::new(paths, initial, event_loop.create_proxy());
    event_loop
        .run_app(&mut helper)
        .context("running the event loop")?;

    // Let a new helper start while this one finishes writing files.
    drop(instance);
    helper.capture.wait_for_deliveries();
    tracing::info!("capture helper exiting");
    Ok(())
}

struct Helper {
    paths: AppPaths,
    config: Config,
    capture: CaptureController,
    keep_alive: KeepAlive,
    request_window: Option<RequestWindow>,
    proxy: EventLoopProxy<HelperEvent>,
    initial: Option<CaptureRequest>,
    /// The settings window or first-run setup, when open.
    prefs: Option<PrefsWindow>,
    /// Wakes the event loop from worker threads.
    waker: Waker,
}

impl Helper {
    fn new(paths: AppPaths, initial: CaptureRequest, proxy: EventLoopProxy<HelperEvent>) -> Self {
        let config = settings::load_config(&paths.config_file);
        let wake_proxy = proxy.clone();
        let waker: Waker = Arc::new(move || {
            let _ = wake_proxy.send_event(HelperEvent::Wake);
        });
        let capture = CaptureController::new(paths.state_file.clone(), waker.clone());
        let keep_alive = KeepAlive::new(config.performance.keep_gpu_ready.idle_timeout());
        Self {
            paths,
            config,
            capture,
            keep_alive,
            request_window: None,
            proxy,
            initial: Some(initial),
            prefs: None,
            waker,
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) {
        settings::clean_up(&self.config, &self.paths);
        if self.config.notifications.toast {
            register_notifications(&self.paths);
        }
        let proxy = self.proxy.clone();
        let window = RequestWindow::create(ipc::Channel::Helper, move |code| {
            if let Some(request) = CaptureRequest::from_code(code) {
                // Only fails once the loop has exited, when requests no longer matter.
                let _ = proxy.send_event(HelperEvent::Request(request));
            }
        });
        match window {
            Ok(window) => self.request_window = Some(window),
            Err(err) => tracing::warn!("cannot receive further requests: {err}"),
        }
        if let Some(request) = self.initial.take() {
            self.handle(event_loop, request);
        }
    }

    fn handle(&mut self, event_loop: &ActiveEventLoop, request: CaptureRequest) {
        // Re-read settings for every capture, so tray "Reload settings" isn't needed here.
        self.config = settings::load_config(&self.paths.config_file);
        self.keep_alive
            .set_timeout(self.config.performance.keep_gpu_ready.idle_timeout());
        match request {
            CaptureRequest::Quit => {
                tracing::info!("asked to quit");
                self.prefs = None;
                self.request_window = None;
                event_loop.exit();
                return;
            }
            CaptureRequest::Home => self.open_prefs(event_loop, Mode::Settings, prefs::HOME),
            CaptureRequest::Settings => {
                self.open_prefs(event_loop, Mode::Settings, prefs::FIRST_SETTINGS);
            }
            CaptureRequest::Onboarding => self.open_prefs(event_loop, Mode::Setup, 0),
            _ => {
                let delivery = self.delivery();
                self.capture
                    .request(event_loop, request, &self.config, &delivery);
            }
        }
        self.track_idle();
    }

    fn open_prefs(&mut self, event_loop: &ActiveEventLoop, mode: Mode, page: usize) {
        if let Some(prefs) = self.prefs.as_mut() {
            prefs.raise(page);
            let waker = self.waker.clone();
            prefs.load_thumbs(move || waker());
            return;
        }
        if mode == Mode::Setup {
            // Shown once, even when closed early: don't nag on every start.
            let state = AppState {
                onboarded: true,
                ..AppState::load(&self.paths.state_file)
            };
            if let Err(err) = state.save(&self.paths.state_file) {
                tracing::warn!("could not save the setup state: {err}");
            }
        }
        let opened = self.capture.gpu().and_then(|gpu| {
            PrefsWindow::open(
                event_loop,
                gpu,
                (mode, page),
                self.config.clone(),
                &self.paths,
            )
        });
        match opened {
            Ok(mut prefs) => {
                let waker = self.waker.clone();
                prefs.load_thumbs(move || waker());
                self.prefs = Some(prefs);
            }
            Err(err) => tracing::error!("could not open settings: {err:#}"),
        }
    }

    /// Routes an event to the settings window; `false` when it isn't for it.
    fn prefs_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: &WindowEvent,
    ) -> bool {
        if self.prefs.as_ref().is_none_or(|p| p.id() != id) {
            return false;
        }
        let Ok(gpu) = self.capture.gpu() else {
            return true;
        };
        let outcome = self
            .prefs
            .as_mut()
            .map_or(PrefsOutcome::Close, |p| p.handle_event(gpu, event));
        match outcome {
            PrefsOutcome::Continue => {}
            PrefsOutcome::OpenRecent(index) => {
                self.handle(event_loop, CaptureRequest::OpenRecent(index));
            }
            PrefsOutcome::Close | PrefsOutcome::CloseAndCapture => {
                self.prefs = None;
                tracing::info!("main window closed");
                if outcome == PrefsOutcome::CloseAndCapture {
                    self.handle(event_loop, CaptureRequest::Interactive);
                }
            }
        }
        true
    }

    fn is_busy(&self) -> bool {
        self.capture.is_busy() || self.prefs.is_some()
    }

    fn delivery(&self) -> Delivery {
        settings::delivery(&self.config, &self.paths)
    }

    /// Starts the exit countdown when the last capture finished, stops it while capturing.
    fn track_idle(&mut self) {
        if self.is_busy() {
            self.keep_alive.mark_busy();
        } else if !self.keep_alive.is_idle() {
            self.keep_alive.mark_idle(Instant::now());
        }
    }
}

/// Writes the app icon next to the state file and registers it for toast headers.
fn register_notifications(paths: &AppPaths) {
    let icon = paths.state_file.with_file_name("icon.png");
    if !icon.exists() {
        let size = u32::try_from(tray_icon_image::SIZE).unwrap_or(32);
        let mut bgra = tray_icon_image::rgba();
        bgra.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|px| px.swap(0, 2));
        let written = Image::from_bgra(size, size, bgra)
            .context("icon size")
            .and_then(|image| Ok(encode::png(&image)?))
            .and_then(|png| Ok(std::fs::write(&icon, png)?));
        if let Err(err) = written {
            tracing::debug!("could not write the notification icon: {err:#}");
        }
    }
    if let Err(err) = cf_platform::toast::register(icon.exists().then_some(icon.as_path())) {
        tracing::warn!("could not register for notifications: {err}");
    }
}

impl ApplicationHandler<HelperEvent> for Helper {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        match cause {
            StartCause::Init => self.start(event_loop),
            StartCause::ResumeTimeReached { .. } => {
                let delivery = self.delivery();
                self.capture.on_timer(event_loop, &self.config, &delivery);
                if let Some(prefs) = &mut self.prefs {
                    prefs.on_timer();
                }
            }
            _ => {}
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: HelperEvent) {
        match event {
            HelperEvent::Request(request) => self.handle(event_loop, request),
            HelperEvent::Wake => {
                self.capture.on_wake();
                if let (Some(prefs), Ok(gpu)) = (self.prefs.as_mut(), self.capture.gpu()) {
                    prefs.take_thumbs(gpu);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.prefs_event(event_loop, id, &event) {
            return;
        }
        let delivery = self.delivery();
        self.capture
            .on_window_event(event_loop, id, event, &delivery);
    }

    /// Exits once idle for the keep-alive period; otherwise sleeps until the next timer.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.track_idle();
        if !self.is_busy() && self.keep_alive.is_expired(Instant::now()) {
            self.request_window = None; // new requests now start a fresh helper
            event_loop.exit();
            return;
        }
        let prefs = self.prefs.as_ref().and_then(PrefsWindow::deadline);
        let next = [
            self.capture.next_deadline(),
            self.keep_alive.deadline(),
            prefs,
        ]
        .into_iter()
        .flatten()
        .min();
        event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}
