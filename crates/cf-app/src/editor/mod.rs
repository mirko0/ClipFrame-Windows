//! The annotation editor: one window per edited capture, in the helper process (it draws
//! with the helper's GPU device).

mod command;
mod icons;
mod render;
mod set;
mod toolbar;
mod window;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cf_core::annotate::{Area, EditorState, Tool, Vec2};
use cf_core::geometry::Rect;
use cf_core::image::Image;
use cf_core::output;
use cf_render::{Canvas, Color, Gpu, GpuImage, RectF, WindowSurface};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Window, WindowId};

use self::command::Command;
pub use self::set::Editors;
use self::toolbar::Button;
use crate::capture::pipeline;

/// Wait after the last edit before re-copying to the clipboard.
const CLIPBOARD_DEBOUNCE: Duration = Duration::from_millis(300);
/// How long "Copied" / "Saved" stays in the toolbar.
const STATUS_TIME: Duration = Duration::from_millis(2500);
const UI_TEXT_PX: f32 = 13.0;

/// A capture to edit.
#[derive(Debug)]
pub struct Captured {
    pub image: Image,
    /// Where it was on the desktop (the editor opens on that monitor).
    pub region: Rect,
}

/// Where edited images go.
#[derive(Debug, Clone)]
pub struct EditorOptions {
    /// Re-copy to the clipboard after each edit.
    pub update_clipboard: bool,
    /// Folder for "Save".
    pub save_dir: PathBuf,
    /// Used when saving into `save_dir` fails.
    pub fallback_dir: PathBuf,
    /// Opacity of the spotlight dim layer.
    pub spotlight_dim: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorOutcome {
    Continue,
    Close,
}

pub struct Editor {
    // Field order matters: the surface must be released before its window.
    surface: WindowSurface,
    base: GpuImage,
    window: Window,
    state: EditorState,
    options: EditorOptions,
    input: Input,
    copy_due: Option<Instant>,
    exported_revision: u64,
    saved_path: Option<PathBuf>,
    status: Option<(&'static str, Instant)>,
    jobs: Vec<JoinHandle<()>>,
}

#[derive(Default)]
struct Input {
    pointer: (f32, f32),
    hover: Option<usize>,
    /// The primary button went down on the image (not the toolbar) and is still held.
    drawing: bool,
    modifiers: ModifiersState,
}

/// Image placement in the window: `window = image * zoom + offset`.
#[derive(Debug, Clone, Copy)]
struct View {
    zoom: f32,
    offset: (f32, f32),
}

impl Editor {
    /// Opens an editor for `image`, centred on the monitor around `near`.
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        capture: &Captured,
        options: EditorOptions,
    ) -> anyhow::Result<Self> {
        let image = &capture.image;
        let window = window::create(event_loop, (image.width(), image.height()), capture.region)?;
        let size = window.inner_size();
        let scale = window::scale_of(&window);
        let surface =
            WindowSurface::new(gpu, &window, (size.width, size.height), UI_TEXT_PX * scale)?;
        let base = GpuImage::upload(gpu, image)?;
        #[allow(clippy::cast_precision_loss)] // image sizes
        let state = EditorState::with_canvas(image.width() as f32, image.height() as f32);
        let editor = Self {
            surface,
            base,
            window,
            state,
            options,
            input: Input::default(),
            copy_due: None,
            exported_revision: 0,
            saved_path: None,
            status: None,
            jobs: Vec::new(),
        };
        editor.window.set_visible(true);
        editor.window.focus_window();
        Ok(editor)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// The next moment [`Self::on_timer`] has work.
    pub fn deadline(&self) -> Option<Instant> {
        let status_end = self.status.map(|(_, at)| at + STATUS_TIME);
        [self.copy_due, status_end].into_iter().flatten().min()
    }

    /// Runs the clipboard update and clears the status message when due.
    pub fn on_timer(&mut self, gpu: &Gpu) {
        let now = Instant::now();
        if self.copy_due.is_some_and(|due| now >= due) {
            self.copy_due = None;
            self.export(gpu, true, None);
        }
        if self.status.is_some_and(|(_, at)| now >= at + STATUS_TIME) {
            self.status = None;
            self.window.request_redraw();
        }
    }

    /// Finishes before closing: commits typed text and flushes a pending clipboard update.
    pub fn close(&mut self, gpu: &Gpu) {
        self.state.commit_text();
        let changed = self.state.document().revision() != self.exported_revision;
        if self.options.update_clipboard && changed {
            self.export(gpu, true, None);
        }
    }

    /// Background exports started since the last call (the helper waits for them on exit).
    pub fn take_jobs(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.jobs)
    }

    pub fn handle_event(&mut self, gpu: &Gpu, event: WindowEvent) -> EditorOutcome {
        let revision = self.state.document().revision();
        let outcome = match event {
            WindowEvent::RedrawRequested => {
                self.render(gpu);
                return EditorOutcome::Continue;
            }
            WindowEvent::CloseRequested => EditorOutcome::Close,
            WindowEvent::Resized(size) => {
                if let Err(err) = self.surface.resize((size.width, size.height)) {
                    tracing::warn!("could not resize the editor surface: {err:#}");
                }
                EditorOutcome::Continue
            }
            WindowEvent::ModifiersChanged(m) => {
                self.input.modifiers = m.state();
                return EditorOutcome::Continue;
            }
            WindowEvent::CursorMoved { position, .. } => {
                #[allow(clippy::cast_possible_truncation)] // window pixels
                self.on_pointer_move((position.x as f32, position.y as f32));
                EditorOutcome::Continue
            }
            WindowEvent::CursorLeft { .. } => {
                self.input.hover = None;
                EditorOutcome::Continue
            }
            WindowEvent::MouseInput { state, button, .. } => self.on_button(gpu, state, button),
            WindowEvent::KeyboardInput { event, .. } => self.on_key(gpu, &event),
            _ => return EditorOutcome::Continue,
        };
        self.after_input(revision);
        outcome
    }

    fn after_input(&mut self, revision: u64) {
        let now = self.state.document().revision();
        if now != revision && self.options.update_clipboard {
            self.copy_due = Some(Instant::now() + CLIPBOARD_DEBOUNCE);
        }
        self.window.request_redraw();
    }

    fn on_pointer_move(&mut self, p: (f32, f32)) {
        self.input.pointer = p;
        let in_toolbar = p.1 < toolbar::HEIGHT * self.scale();
        self.input.hover = if in_toolbar && !self.input.drawing {
            toolbar::hit(&self.buttons(), p)
        } else {
            None
        };
        if self.input.drawing {
            self.state.pointer_move(self.to_image(p));
        }
        self.window.set_cursor(self.cursor_icon(in_toolbar));
    }

    fn on_button(&mut self, gpu: &Gpu, state: ElementState, button: MouseButton) -> EditorOutcome {
        if button != MouseButton::Left {
            return EditorOutcome::Continue;
        }
        let p = self.input.pointer;
        match state {
            ElementState::Pressed => {
                if let Some(index) = toolbar::hit(&self.buttons(), p) {
                    let command = self.buttons()[index].command;
                    return self.run(gpu, command);
                }
                if p.1 >= toolbar::HEIGHT * self.scale() {
                    self.input.drawing = true;
                    let zoom = self.view().zoom;
                    self.state.pointer_down(self.to_image(p), 1.0 / zoom);
                }
            }
            ElementState::Released if self.input.drawing => {
                self.input.drawing = false;
                self.state.pointer_up(self.to_image(p));
            }
            ElementState::Released => {}
        }
        EditorOutcome::Continue
    }

    fn on_key(&mut self, gpu: &Gpu, event: &winit::event::KeyEvent) -> EditorOutcome {
        let typing = self.state.is_typing();
        if let Some(command) = command::for_key(event, self.input.modifiers, typing) {
            return self.run(gpu, command);
        }
        let plain = !self.input.modifiers.control_key() && !self.input.modifiers.alt_key();
        if typing && plain && event.state == ElementState::Pressed {
            if let Some(text) = &event.text {
                self.state.type_text(text);
            }
        }
        EditorOutcome::Continue
    }

    fn run(&mut self, gpu: &Gpu, command: Command) -> EditorOutcome {
        if let Some(style) = command.restyle(self.state.style()) {
            self.state.set_style(style);
            return EditorOutcome::Continue;
        }
        match command {
            Command::Tool(tool) => self.state.set_tool(tool),
            Command::Undo => _ = self.state.undo(),
            Command::Redo => _ = self.state.redo(),
            Command::Delete => self.state.delete_selected(),
            Command::Commit => self.state.commit_text(),
            Command::NewLine => self.state.new_line(),
            Command::Backspace => self.state.backspace(),
            Command::Copy => {
                self.export(gpu, true, None);
                self.status = Some(("Copied to the clipboard", Instant::now()));
            }
            Command::Save => self.save(gpu),
            Command::Escape if !self.state.escape() => return EditorOutcome::Close,
            _ => {}
        }
        EditorOutcome::Continue
    }

    fn save(&mut self, gpu: &Gpu) {
        let path = self.saved_path.clone().unwrap_or_else(|| {
            let name = output::capture_file_name(cf_platform::time::local_now(), "png");
            output::unique_path(&self.options.save_dir, &name, Path::exists)
        });
        self.export(gpu, false, Some(path.clone()));
        self.saved_path = Some(path);
        self.status = Some(("Saved", Instant::now()));
    }

    /// Renders the edited image and hands it to a background thread.
    fn export(&mut self, gpu: &Gpu, copy: bool, save_to: Option<PathBuf>) {
        self.state.commit_text();
        let doc = self.state.document();
        let area = doc.crop().unwrap_or_else(|| self.whole());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole pixels
        let size = (area.width() as u32, area.height() as u32);
        let backdrop = self.backdrop();
        let flattened = cf_render::render_to_image(gpu, size, |canvas| {
            canvas.set_transform(1.0, (-area.min.x, -area.min.y));
            canvas.draw_image(&self.base, rect_of(self.whole()), None, false);
            render::document(canvas, &backdrop, doc, None);
        });
        match flattened {
            Ok(image) => {
                if copy {
                    self.exported_revision = doc.revision();
                }
                let fallback = self.options.fallback_dir.clone();
                self.jobs
                    .extend(pipeline::export(image, copy, save_to, fallback));
            }
            Err(err) => tracing::error!("could not render the edited image: {err:#}"),
        }
    }

    fn render(&self, gpu: &Gpu) {
        let result = self.surface.draw(gpu, |canvas| self.paint(canvas));
        if let Err(err) = result {
            tracing::warn!("editor frame failed: {err:#}");
        }
    }

    fn paint(&self, canvas: &Canvas<'_>) {
        canvas.clear(Color::rgba8(0x1B, 0x1B, 0x20, 0xFF));
        let view = self.view();
        let image = rect_of(self.whole());
        canvas.set_transform(view.zoom, view.offset);
        canvas.push_clip(rect_of(self.shown()));
        canvas.draw_image(&self.base, image, None, view.zoom < 1.0);
        let preview = self.state.preview();
        render::document(
            canvas,
            &self.backdrop(),
            self.state.document(),
            preview.as_ref(),
        );
        if let Some(crop) = self.state.crop_preview().filter(|_| self.cropping()) {
            render::crop_overlay(canvas, image, crop, view.zoom);
        }
        if let Some(item) = self
            .state
            .selected()
            .and_then(|i| self.state.document().items().get(i))
        {
            render::selection_frame(canvas, item, view.zoom);
        }
        canvas.pop_clip();
        canvas.reset_transform();
        let look = toolbar::Look {
            state: &self.state,
            hover: self.input.hover,
            scale: self.scale(),
            status: self.status.map(|(text, _)| text),
        };
        toolbar::paint(canvas, &self.buttons(), &look);
    }

    /// Fits the shown part of the image (the crop, or all of it while cropping) into the
    /// window below the toolbar.
    fn view(&self) -> View {
        let s = self.scale();
        let (win_w, win_h) = self.surface.size();
        let margin = window::MARGIN * s;
        let top = toolbar::HEIGHT * s + margin;
        let shown = self.shown();
        #[allow(clippy::cast_precision_loss)] // window sizes
        let (area_w, area_h) = (win_w as f32 - 2.0 * margin, win_h as f32 - top - margin);
        let (img_w, img_h) = (shown.width().max(1.0), shown.height().max(1.0));
        let zoom = (area_w / img_w).min(area_h / img_h).clamp(0.05, 1.0);
        let offset = (
            margin + (area_w - img_w * zoom).max(0.0) / 2.0 - shown.min.x * zoom,
            top + (area_h - img_h * zoom).max(0.0) / 2.0 - shown.min.y * zoom,
        );
        View { zoom, offset }
    }

    /// The part of the image on screen.
    fn shown(&self) -> Area {
        if self.cropping() {
            return self.whole();
        }
        self.state.document().crop().unwrap_or_else(|| self.whole())
    }

    fn whole(&self) -> Area {
        let size = self.state.canvas();
        Area::from_corners(Vec2::default(), size)
    }

    fn cropping(&self) -> bool {
        self.state.tool() == Tool::Crop
    }

    fn backdrop(&self) -> render::Backdrop<'_> {
        render::Backdrop {
            base: &self.base,
            dim: self.options.spotlight_dim,
        }
    }

    fn to_image(&self, p: (f32, f32)) -> Vec2 {
        let view = self.view();
        Vec2::new(
            (p.0 - view.offset.0) / view.zoom,
            (p.1 - view.offset.1) / view.zoom,
        )
    }

    fn buttons(&self) -> Vec<Button> {
        toolbar::layout(self.scale())
    }

    fn scale(&self) -> f32 {
        window::scale_of(&self.window)
    }

    fn cursor_icon(&self, in_toolbar: bool) -> CursorIcon {
        match (in_toolbar, self.state.tool()) {
            (true, _) | (false, Tool::Select) => CursorIcon::Default,
            (false, Tool::Text) => CursorIcon::Text,
            _ => CursorIcon::Crosshair,
        }
    }
}

fn rect_of(area: Area) -> RectF {
    RectF::new(area.min.x, area.min.y, area.width(), area.height())
}
