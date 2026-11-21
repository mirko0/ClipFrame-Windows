//! The settings window and the first-run setup: one Direct2D window in the capture helper
//! (like the editor), drawn from [`model`] pages. Every change is saved immediately and the
//! tray is told to reload (it owns the global hotkeys).

mod model;
mod view;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cf_core::config::{self, Config};
use cf_core::hotkey::{Hotkey, Key, Modifiers};
use cf_core::naming::{self, Source};
use cf_platform::ipc;
use cf_platform::paths::AppPaths;
use cf_render::{Canvas, Gpu, WindowSurface};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key as LogicalKey, ModifiersState, NamedKey, PhysicalKey};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{CursorIcon, Window, WindowButtons, WindowId};

use self::model::{Context, Field, Kind, Page, SETTINGS, SETUP, Value};
pub use self::view::Mode;
use self::view::{RowLook, Target};

/// Message code telling the tray to reload settings.
pub const RELOAD_CODE: u32 = 1;
/// Message code: release the global shortcuts while one is being recorded.
pub const PAUSE_HOTKEYS_CODE: u32 = 3;
/// Message code: register the global shortcuts again.
pub const RESUME_HOTKEYS_CODE: u32 = 4;
const STATUS_TIME: Duration = Duration::from_millis(2500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsOutcome {
    Continue,
    Close,
    /// Close, then start a capture ("Take a capture now").
    CloseAndCapture,
}

enum Focus {
    Text { row: usize, text: String },
    Hotkey { row: usize },
}

pub struct PrefsWindow {
    // Field order matters: the surface must be released before its window.
    surface: WindowSurface,
    window: Window,
    mode: Mode,
    page: usize,
    config: Config,
    config_file: PathBuf,
    ctx: Context,
    hover: Option<Target>,
    focus: Option<Focus>,
    status: Option<(String, Instant)>,
    modifiers: ModifiersState,
    pointer: (f32, f32),
    /// Whether the tray's global shortcuts are paused for recording.
    hotkeys_paused: bool,
}

impl PrefsWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        mode: Mode,
        config: Config,
        paths: &AppPaths,
    ) -> anyhow::Result<Self> {
        let title = match mode {
            Mode::Settings => "ClipFrame settings",
            Mode::Setup => "Set up ClipFrame",
        };
        let window = event_loop.create_window(
            Window::default_attributes()
                .with_title(title)
                .with_resizable(false)
                .with_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE)
                .with_visible(false),
        )?;
        let scale = scale_of(&window);
        let size = PhysicalSize::new(to_px(view::WIDTH * scale), to_px(view::HEIGHT * scale));
        let _ = window.request_inner_size(size);
        center_on_pointer_monitor(&window, size);
        let actual = window.inner_size();
        let surface =
            WindowSurface::new(gpu, &window, (actual.width, actual.height), 13.0 * scale)?;
        let ctx = Context {
            startup: cf_platform::startup::is_enabled(),
            default_dir: paths.default_captures_dir.clone(),
        };
        let prefs = Self {
            surface,
            window,
            mode,
            page: 0,
            config,
            config_file: paths.config_file.clone(),
            ctx,
            hover: None,
            focus: None,
            status: None,
            modifiers: ModifiersState::empty(),
            pointer: (0.0, 0.0),
            hotkeys_paused: false,
        };
        prefs.window.set_visible(true);
        prefs.window.focus_window();
        Ok(prefs)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Shows the window again (when it is requested while already open).
    pub fn raise(&self) {
        self.window.set_minimized(false);
        self.window.focus_window();
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.status.as_ref().map(|(_, at)| *at + STATUS_TIME)
    }

    pub fn on_timer(&mut self) {
        if self
            .status
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= STATUS_TIME)
        {
            self.status = None;
            self.window.request_redraw();
        }
    }

    pub fn handle_event(&mut self, gpu: &Gpu, event: &WindowEvent) -> PrefsOutcome {
        let outcome = match event {
            WindowEvent::RedrawRequested => {
                self.render(gpu);
                return PrefsOutcome::Continue;
            }
            WindowEvent::CloseRequested => PrefsOutcome::Close,
            WindowEvent::Resized(size) => {
                if let Err(err) = self.surface.resize((size.width, size.height)) {
                    tracing::warn!("could not resize the settings window: {err:#}");
                }
                PrefsOutcome::Continue
            }
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
                return PrefsOutcome::Continue;
            }
            WindowEvent::CursorMoved { position, .. } => {
                #[allow(clippy::cast_possible_truncation)] // window pixels
                let p = (position.x as f32, position.y as f32);
                self.pointer = p;
                let hover = view::hit(&self.targets(), p);
                self.window.set_cursor(if hover.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                });
                if hover == self.hover {
                    return PrefsOutcome::Continue;
                }
                self.hover = hover;
                PrefsOutcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.on_click(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.on_key(event)
            }
            _ => return PrefsOutcome::Continue,
        };
        self.sync_hotkey_pause();
        self.window.request_redraw();
        outcome
    }

    /// Pauses the tray's global shortcuts while a shortcut is being recorded (otherwise
    /// pressing the current one would start a capture) and resumes them afterwards.
    fn sync_hotkey_pause(&mut self) {
        let recording = matches!(self.focus, Some(Focus::Hotkey { .. }));
        if recording != self.hotkeys_paused {
            self.hotkeys_paused = recording;
            let code = if recording {
                PAUSE_HOTKEYS_CODE
            } else {
                RESUME_HOTKEYS_CODE
            };
            let _ = ipc::post_request(ipc::Channel::Tray, code);
        }
    }

    fn pages(&self) -> &'static [Page] {
        match self.mode {
            Mode::Settings => &SETTINGS,
            Mode::Setup => &SETUP,
        }
    }

    fn targets(&self) -> Vec<(cf_render::RectF, Target)> {
        view::layout(self.mode, self.pages(), self.page, scale_of(&self.window))
    }

    fn on_click(&mut self) -> PrefsOutcome {
        let target = view::hit(&self.targets(), self.pointer);
        self.commit_text();
        if !matches!(target, Some(Target::Control(_))) {
            self.focus = None;
        }
        match target {
            Some(Target::Page(index)) => self.page = index,
            Some(Target::Control(row)) => self.activate(row),
            Some(Target::ChoicePrev(row)) => self.step_choice(row, false),
            Some(Target::ChoiceNext(row)) => self.step_choice(row, true),
            Some(Target::Back) => self.page = self.page.saturating_sub(1),
            Some(Target::Next) => return self.next_step(),
            Some(Target::CaptureNow) => return PrefsOutcome::CloseAndCapture,
            None => {}
        }
        PrefsOutcome::Continue
    }

    fn next_step(&mut self) -> PrefsOutcome {
        if self.page + 1 >= self.pages().len() {
            return PrefsOutcome::Close;
        }
        self.page += 1;
        PrefsOutcome::Continue
    }

    fn field(&self, row: usize) -> Option<Field> {
        self.pages().get(self.page)?.rows.get(row).map(|r| r.field)
    }

    fn activate(&mut self, row: usize) {
        let Some(field) = self.field(row) else { return };
        match (field.kind(), field.get(&self.config, &self.ctx)) {
            (Kind::Toggle, Value::Bool(on)) => self.apply(field, Value::Bool(!on)),
            (Kind::Hotkey { .. }, _) => self.focus = Some(Focus::Hotkey { row }),
            (Kind::Text, Value::Text(text)) => self.focus = Some(Focus::Text { row, text }),
            (Kind::Folder, Value::Text(current)) => self.pick_folder(field, &current),
            _ => {}
        }
    }

    /// Moves a choice one option back (`forward = false`) or on, wrapping around.
    fn step_choice(&mut self, row: usize, forward: bool) {
        let Some(field) = self.field(row) else { return };
        let (Kind::Choice(options), Value::Choice(index)) =
            (field.kind(), field.get(&self.config, &self.ctx))
        else {
            return;
        };
        let count = options.len().max(1);
        let next = if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        };
        self.apply(field, Value::Choice(next));
    }

    fn pick_folder(&mut self, field: Field, current: &str) {
        let start = model::folder_of(current, &self.ctx.default_dir);
        let owner = match self.window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
            _ => 0,
        };
        match cf_platform::folder_picker::pick_folder(
            owner,
            &start,
            "Choose where captures are saved",
        ) {
            Ok(Some(folder)) => self.apply(field, Value::Text(folder.display().to_string())),
            Ok(None) => {}
            Err(err) => tracing::warn!("folder picker failed: {err}"),
        }
    }

    fn on_key(&mut self, event: &KeyEvent) -> PrefsOutcome {
        self.modifiers = crate::modifiers::held();
        match self.focus.take() {
            Some(Focus::Hotkey { row }) => self.record_hotkey(row, event),
            Some(Focus::Text { row, text }) => self.edit_text(row, text, event),
            None => match &event.logical_key {
                LogicalKey::Named(NamedKey::Escape) if self.mode == Mode::Settings => {
                    return PrefsOutcome::Close;
                }
                LogicalKey::Named(NamedKey::Enter) if self.mode == Mode::Setup => {
                    return self.next_step();
                }
                _ => {}
            },
        }
        PrefsOutcome::Continue
    }

    fn record_hotkey(&mut self, row: usize, event: &KeyEvent) {
        let Some(field) = self.field(row) else { return };
        let optional = matches!(field.kind(), Kind::Hotkey { optional: true });
        match &event.logical_key {
            LogicalKey::Named(NamedKey::Escape) => return,
            LogicalKey::Named(NamedKey::Backspace | NamedKey::Delete) if optional => {
                return self.apply(field, Value::Hotkey(None));
            }
            _ => {}
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            self.focus = Some(Focus::Hotkey { row });
            return;
        };
        let Some(key) = Key::from_code_name(&format!("{code:?}")) else {
            // A modifier on its own (or an unusable key): keep listening.
            self.focus = Some(Focus::Hotkey { row });
            return;
        };
        let m = self.modifiers;
        let modifiers = Modifiers {
            ctrl: m.control_key(),
            alt: m.alt_key(),
            shift: m.shift_key(),
            win: m.super_key(),
        };
        match Hotkey::new(modifiers, key) {
            Ok(hotkey) => self.apply(field, Value::Hotkey(Some(hotkey))),
            Err(err) => {
                self.say(format!("{err}"));
                self.focus = Some(Focus::Hotkey { row });
            }
        }
    }

    fn edit_text(&mut self, row: usize, mut text: String, event: &KeyEvent) {
        match &event.logical_key {
            LogicalKey::Named(NamedKey::Escape) => return,
            LogicalKey::Named(NamedKey::Enter) => {
                if let Some(field) = self.field(row) {
                    self.apply(field, Value::Text(text));
                }
                return;
            }
            LogicalKey::Named(NamedKey::Backspace) => {
                text.pop();
            }
            _ if !self.modifiers.control_key() => {
                if let Some(typed) = &event.text {
                    text.extend(typed.chars().filter(|c| !c.is_control()));
                }
            }
            _ => {}
        }
        self.focus = Some(Focus::Text { row, text });
    }

    /// Finishes text being typed (clicking elsewhere keeps what was typed).
    fn commit_text(&mut self) {
        if let Some(Focus::Text { row, text }) = self.focus.take()
            && let Some(field) = self.field(row)
        {
            self.apply(field, Value::Text(text));
        }
    }

    /// Stores a change, saves the settings file and tells the tray to reload.
    fn apply(&mut self, field: Field, value: Value) {
        if field == Field::StartWithWindows {
            let Value::Bool(on) = value else { return };
            let exe = std::env::current_exe().unwrap_or_default();
            match cf_platform::startup::set_enabled(on, &exe) {
                Ok(()) => self.ctx.startup = on,
                Err(code) => self.say(format!(
                    "Could not change the startup setting (error {code})."
                )),
            }
            return;
        }
        if field.get(&self.config, &self.ctx) == value {
            return;
        }
        if let Err(message) = field.set(&mut self.config, value, &self.ctx) {
            self.say(message);
            return;
        }
        match config::save(&self.config_file, &self.config) {
            Ok(()) => {
                let _ = ipc::post_request(ipc::Channel::Tray, RELOAD_CODE);
                self.say("Saved");
            }
            Err(err) => self.say(format!("Could not save: {err}")),
        }
    }

    fn say(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), Instant::now()));
        self.window.request_redraw();
    }

    fn render(&self, gpu: &Gpu) {
        if let Err(err) = self.surface.draw(gpu, |canvas| self.paint(canvas)) {
            tracing::warn!("settings frame failed: {err:#}");
        }
    }

    fn paint(&self, canvas: &Canvas<'_>) {
        let s = scale_of(&self.window);
        canvas.clear(view::background());
        let targets = self.targets();
        let pages = self.pages();
        let Some(page) = pages.get(self.page) else {
            return;
        };
        if self.mode == Mode::Settings {
            view::sidebar(canvas, pages, &targets, (self.page, self.hover, s));
        }
        let intro = page
            .intro
            .replace("{hotkey}", &self.config.hotkeys.capture.to_string());
        view::heading(canvas, self.mode, page.title, &intro, s);
        for (index, row) in page.rows.iter().enumerate() {
            let Some(control) = view::control_rect(&targets, index) else {
                continue;
            };
            let value = row.field.get(&self.config, &self.ctx);
            let hint = self.hint(row.field, row.hint);
            let editing = match &self.focus {
                Some(Focus::Text { row: r, text }) if *r == index => Some(text.as_str()),
                Some(Focus::Hotkey { row: r }) if *r == index => Some(""),
                _ => None,
            };
            let hovered = matches!(self.hover, Some(Target::Control(i) | Target::ChoicePrev(i) | Target::ChoiceNext(i)) if i == index);
            let look = RowLook {
                label: row.label,
                hint: &hint,
                kind: row.field.kind(),
                value: &value,
                editing,
                hovered,
            };
            view::row(canvas, self.mode, control, &look, s);
        }
        if self.mode == Mode::Setup {
            view::setup_buttons(canvas, &targets, (self.page, pages.len()), self.hover, s);
        }
        if let Some((text, _)) = &self.status {
            view::status(canvas, self.mode, text, s);
        }
    }

    /// The file-name row shows an example of the current template.
    fn hint(&self, field: Field, hint: &'static str) -> String {
        if field != Field::FileName {
            return hint.to_owned();
        }
        let template = match &self.focus {
            Some(Focus::Text { text, .. }) => text.as_str(),
            _ => &self.config.output.file_name,
        };
        let example = Source {
            app: "chrome".into(),
            title: "Example page".into(),
        };
        let name = naming::render(template, cf_platform::time::local_now(), &example, Some(1));
        format!("e.g. {name}.png\n{hint}")
    }
}

impl Drop for PrefsWindow {
    /// Closing the window while recording must not leave the shortcuts switched off.
    fn drop(&mut self) {
        if self.hotkeys_paused {
            let _ = ipc::post_request(ipc::Channel::Tray, RESUME_HOTKEYS_CODE);
        }
    }
}

fn scale_of(window: &Window) -> f32 {
    #[allow(clippy::cast_possible_truncation)] // scale factors are small
    let scale = window.scale_factor() as f32;
    scale
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive, screen-sized
fn to_px(v: f32) -> u32 {
    v.round().max(1.0) as u32
}

fn center_on_pointer_monitor(window: &Window, size: PhysicalSize<u32>) {
    let pointer = cf_platform::cursor::position();
    let monitors = cf_platform::monitors::enumerate().unwrap_or_default();
    let area = monitors
        .iter()
        .find(|m| pointer.is_some_and(|p| m.rect.contains(p)))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .map(|m| m.work_area);
    if let Some(area) = area {
        let (w, h) = (
            i32::try_from(size.width).unwrap_or(0),
            i32::try_from(size.height).unwrap_or(0),
        );
        window.set_outer_position(PhysicalPosition::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
        ));
    }
}
