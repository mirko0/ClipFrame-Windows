//! Background UI Automation thread for the element and browser-content modes.
//!
//! UI Automation calls cross into other processes and can take tens of milliseconds, so they
//! never run on the overlay's thread. Requests queue up here; the worker answers only the
//! newest element query (older ones are stale once the pointer has moved) and wakes the
//! event loop through `waker` when an answer is ready.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use cf_core::geometry::{Point, Rect};
use cf_core::targets::WindowInfo;
use cf_platform::ui_automation::UiAutomation;

/// Wakes the event loop from another thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Chromium builds its accessibility tree on the first query; ask once more after this.
const DOCUMENT_RETRY: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy)]
pub enum Query {
    Element { window: WindowInfo, point: Point },
    Document { window: WindowInfo },
}

#[derive(Debug, Clone)]
pub enum Answer {
    /// Element bounds under the queried point, innermost first, ending with the window.
    Element {
        window: WindowInfo,
        rects: Vec<Rect>,
    },
    Document {
        window: WindowInfo,
        rect: Option<Rect>,
    },
}

/// Handle to the worker; dropping it stops the worker after its current query.
#[derive(Debug)]
pub struct Picker {
    queries: Sender<Query>,
    answers: Receiver<Answer>,
}

impl Picker {
    pub fn spawn(waker: Waker) -> std::io::Result<Self> {
        let (queries, query_rx) = mpsc::channel();
        let (answer_tx, answers) = mpsc::channel();
        std::thread::Builder::new()
            .name("element-picker".into())
            .spawn(move || run(&query_rx, &answer_tx, &*waker))?;
        Ok(Self { queries, answers })
    }

    pub fn ask(&self, query: Query) {
        // Fails only if the worker died (UI Automation unavailable); hover then falls back
        // to whole windows.
        let _ = self.queries.send(query);
    }

    pub fn answers(&self) -> impl Iterator<Item = Answer> + '_ {
        self.answers.try_iter()
    }
}

fn run(queries: &Receiver<Query>, answers: &Sender<Answer>, wake: &(dyn Fn() + Send + Sync)) {
    let uia = match UiAutomation::new() {
        Ok(uia) => uia,
        Err(err) => {
            tracing::warn!("UI Automation unavailable; element modes pick windows: {err}");
            return;
        }
    };
    while let Ok(first) = queries.recv() {
        for query in coalesce(first, queries) {
            if answers.send(answer(&uia, query)).is_err() {
                return; // overlay closed
            }
            wake();
        }
    }
}

/// Everything queued right now, keeping only the newest element query.
fn coalesce(first: Query, queries: &Receiver<Query>) -> Vec<Query> {
    let mut pending = vec![first];
    pending.extend(queries.try_iter());
    let newest_element = pending
        .iter()
        .rposition(|q| matches!(q, Query::Element { .. }));
    pending
        .into_iter()
        .enumerate()
        .filter(|(i, q)| matches!(q, Query::Document { .. }) || Some(*i) == newest_element)
        .map(|(_, q)| q)
        .collect()
}

fn answer(uia: &UiAutomation, query: Query) -> Answer {
    match query {
        Query::Element { window, point } => {
            let mut rects = uia.elements_at(window.id, point).unwrap_or_else(|err| {
                tracing::debug!("element hit-test failed: {err}");
                Vec::new()
            });
            rects.push(window.rect);
            Answer::Element { window, rects }
        }
        Query::Document { window } => Answer::Document {
            window,
            rect: document(uia, window),
        },
    }
}

fn document(uia: &UiAutomation, window: WindowInfo) -> Option<Rect> {
    let find = || uia.document_rect(window.id).ok().flatten();
    find().or_else(|| {
        std::thread::sleep(DOCUMENT_RETRY);
        find()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> WindowInfo {
        WindowInfo {
            id: 1,
            rect: Rect::new(0, 0, 10, 10),
        }
    }

    fn element(x: i32) -> Query {
        Query::Element {
            window: window(),
            point: Point::new(x, 0),
        }
    }

    #[test]
    fn only_the_newest_element_query_survives_but_documents_all_do() {
        let (tx, rx) = mpsc::channel();
        for query in [Query::Document { window: window() }, element(2), element(3)] {
            tx.send(query).ok();
        }
        let kept = coalesce(element(1), &rx);
        assert_eq!(kept.len(), 2);
        assert!(matches!(kept[0], Query::Document { .. }));
        assert!(matches!(kept[1], Query::Element { point, .. } if point.x == 3));
    }
}
