//! Recognition for one editor, off the UI thread: QR/barcodes are scanned as soon as the
//! editor opens; text is read (Windows OCR) the first time it is needed, then reused.
//! Workers report through a channel and wake the event loop with the capture `Waker`.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

use cf_core::annotate::{Annotation, Shape, Style};
use cf_core::image::Image;
use cf_core::ocr::Recognized;
use cf_detect::codes::{self, Code};
use cf_detect::{link, sensitive};

use super::Editor;
use super::chips::ChipAction;
use crate::capture::Waker;

/// Something waiting for the text to be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    CopyText,
    Redact,
}

enum Answer {
    Codes(Vec<Code>),
    Text(Result<Recognized, String>),
}

pub struct Smart {
    image: Arc<Image>,
    waker: Waker,
    sender: Sender<Answer>,
    answers: Receiver<Answer>,
    codes: Vec<Code>,
    text: Option<Recognized>,
    reading: bool,
    wants: Vec<Want>,
}

/// What changed since the last poll.
#[derive(Default)]
pub struct Update {
    pub changed: bool,
    pub ready: Vec<Want>,
    pub failed: Option<String>,
}

impl Smart {
    /// Starts scanning `image` for codes.
    pub fn start(image: Image, waker: Waker) -> Self {
        let (sender, answers) = mpsc::channel();
        let smart = Self {
            image: Arc::new(image),
            waker,
            sender,
            answers,
            codes: Vec::new(),
            text: None,
            reading: false,
            wants: Vec::new(),
        };
        smart.spawn("cf-scan", |image| Answer::Codes(codes::scan(image)));
        smart
    }

    pub fn codes(&self) -> &[Code] {
        &self.codes
    }

    pub fn text(&self) -> Option<&Recognized> {
        self.text.as_ref()
    }

    /// Asks for the text on behalf of `want`: `true` when it is ready now; otherwise it is
    /// being read and `want` is reported by [`Self::poll`] once it is.
    pub fn need_text(&mut self, want: Want) -> bool {
        if self.text.is_some() {
            return true;
        }
        if !self.wants.contains(&want) {
            self.wants.push(want);
        }
        if !self.reading {
            self.reading = true;
            self.spawn("cf-ocr", read_text);
        }
        false
    }

    /// Collects finished work.
    pub fn poll(&mut self) -> Update {
        let mut update = Update::default();
        while let Ok(answer) = self.answers.try_recv() {
            update.changed = true;
            match answer {
                Answer::Codes(codes) => {
                    tracing::debug!(found = codes.len(), "codes scanned");
                    self.codes = codes;
                }
                Answer::Text(result) => self.take_text(result, &mut update),
            }
        }
        update
    }

    fn take_text(&mut self, result: Result<Recognized, String>, update: &mut Update) {
        self.reading = false;
        let wants = std::mem::take(&mut self.wants);
        match result {
            Ok(text) => {
                self.text = Some(text);
                update.ready = wants;
            }
            Err(err) => update.failed = Some(err),
        }
    }

    fn spawn(&self, name: &str, work: impl FnOnce(&Image) -> Answer + Send + 'static) {
        let (image, sender, waker) = (
            Arc::clone(&self.image),
            self.sender.clone(),
            Arc::clone(&self.waker),
        );
        let spawned = thread::Builder::new().name(name.into()).spawn(move || {
            // The editor may have closed meanwhile; then nobody listens and that's fine.
            if sender.send(work(&image)).is_ok() {
                waker();
            }
        });
        if let Err(err) = spawned {
            tracing::error!("could not start {name}: {err}");
        }
    }
}

/// Runs Windows OCR on a worker thread.
fn read_text(image: &Image) -> Answer {
    let started = Instant::now();
    let text = cf_platform::ocr::recognize(image).map_err(|e| e.to_string());
    if let Ok(found) = &text {
        let ms = started.elapsed().as_millis();
        tracing::debug!(lines = found.lines.len(), ms, "text recognised");
    }
    Answer::Text(text)
}

impl Editor {
    /// Background work finished: show codes, finish what waited for the text.
    pub fn on_wake(&mut self) {
        let revision = self.state.document().revision();
        let update = self.smart.poll();
        for want in update.ready {
            self.fulfil(want);
        }
        if let Some(err) = update.failed {
            tracing::warn!("text recognition failed: {err}");
            self.say(format!("Could not read text: {err}"));
        }
        if update.changed {
            self.after_input(revision);
        }
    }

    /// Runs `want` now if the text is known, else once it has been read.
    pub(super) fn want_text(&mut self, want: Want) {
        if self.smart.need_text(want) {
            self.fulfil(want);
        } else {
            self.say("Reading text…");
        }
    }

    pub(super) fn on_chip(&mut self, action: ChipAction) {
        match action {
            ChipAction::Copy(index) => {
                let Some(code) = self.smart.codes().get(index) else {
                    return;
                };
                let copied = cf_platform::clipboard::set_text(&code.text);
                self.say(match copied {
                    Ok(()) => "Code content copied",
                    Err(_) => "The clipboard is busy",
                });
            }
            ChipAction::Open(index) => {
                let url = self
                    .smart
                    .codes()
                    .get(index)
                    .and_then(|c| link::web_url(&c.text));
                let Some(url) = url.map(str::to_owned) else {
                    return;
                };
                if let Err(err) = cf_platform::shell::open_url(&url) {
                    tracing::warn!("could not open the link: {err}");
                }
                self.say("Opened in the browser");
            }
        }
    }

    fn fulfil(&mut self, want: Want) {
        let area = self.state.document().crop().unwrap_or_else(|| self.whole());
        let Some(text) = self.smart.text().map(|t| t.within(area)) else {
            return;
        };
        match want {
            Want::CopyText if text.is_empty() => self.say("No text found"),
            Want::CopyText => {
                let message = match cf_platform::clipboard::set_text(&text.text()) {
                    Ok(()) => format!("Copied text ({} lines)", text.lines.len()),
                    Err(_) => "The clipboard is busy".to_owned(),
                };
                self.say(message);
            }
            Want::Redact => {
                let style = Style {
                    filled: true,
                    ..Style::default()
                };
                let items = sensitive::find(&text)
                    .into_iter()
                    .map(|found| Annotation {
                        shape: Shape::Redact {
                            a: found.area.min,
                            b: found.area.max,
                        },
                        style,
                    })
                    .collect();
                let count = self.state.add_items(items);
                self.say(match count {
                    0 => "Nothing sensitive found".to_owned(),
                    1 => "Hid 1 item".to_owned(),
                    n => format!("Hid {n} items"),
                });
            }
        }
    }
}
