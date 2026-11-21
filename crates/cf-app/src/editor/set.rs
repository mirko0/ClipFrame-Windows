//! All open editor windows.

use std::thread::JoinHandle;
use std::time::Instant;

use cf_render::Gpu;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use super::{Captured, Editor, EditorOptions, EditorOutcome};
use crate::capture::Waker;

pub struct Editors {
    open: Vec<Editor>,
    /// Wakes the event loop when an editor's background work finishes.
    waker: Waker,
    /// Exports of editors that have closed, not yet collected.
    leftover: Vec<JoinHandle<()>>,
}

impl Editors {
    pub fn new(waker: Waker) -> Self {
        Self {
            open: Vec::new(),
            waker,
            leftover: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    pub fn open(
        &mut self,
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        capture: &Captured,
        options: EditorOptions,
    ) {
        match Editor::open(event_loop, gpu, capture, options, self.waker.clone()) {
            Ok(editor) => self.open.push(editor),
            Err(err) => tracing::error!("could not open the editor: {err:#}"),
        }
    }

    /// Routes a window event; returns `false` if the window isn't an editor.
    pub fn handle(&mut self, gpu: &Gpu, id: WindowId, event: WindowEvent) -> bool {
        let Some(index) = self.open.iter().position(|e| e.id() == id) else {
            return false;
        };
        if self.open[index].handle_event(gpu, event) == EditorOutcome::Close {
            let mut editor = self.open.remove(index);
            editor.close(gpu);
            self.leftover.extend(editor.take_jobs());
            tracing::info!("editor closed");
        }
        true
    }

    /// The earliest timer any editor needs.
    pub fn deadline(&self) -> Option<Instant> {
        self.open.iter().filter_map(Editor::deadline).min()
    }

    /// Some editor's background work finished.
    pub fn on_wake(&mut self) {
        for editor in &mut self.open {
            editor.on_wake();
        }
    }

    pub fn on_timer(&mut self, gpu: &Gpu) {
        for editor in &mut self.open {
            editor.on_timer(gpu);
        }
    }

    /// Background exports started by any editor (including closed ones).
    pub fn take_jobs(&mut self) -> Vec<JoinHandle<()>> {
        let mut jobs = std::mem::take(&mut self.leftover);
        for editor in &mut self.open {
            jobs.extend(editor.take_jobs());
        }
        jobs
    }
}
