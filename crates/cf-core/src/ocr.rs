//! Recognised text (from Windows OCR) in image pixel coordinates.

use crate::annotate::{Area, Vec2};

/// One recognised word and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    /// The word as read.
    pub text: String,
    /// Its bounding box in image pixels.
    pub area: Area,
}

impl Word {
    /// A word at `(x, y)` of `width × height` pixels.
    pub fn new(text: impl Into<String>, x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            text: text.into(),
            area: Area::from_corners(Vec2::new(x, y), Vec2::new(x + width, y + height)),
        }
    }
}

/// The words of one line, left to right.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    /// Words in reading order.
    pub words: Vec<Word>,
}

/// All text found in an image, top to bottom.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recognized {
    /// Lines in reading order.
    pub lines: Vec<Line>,
}

impl Recognized {
    /// Only the words whose centre lies inside `area` (e.g. the crop); empty lines dropped.
    #[must_use]
    pub fn within(&self, area: Area) -> Self {
        let lines = self
            .lines
            .iter()
            .map(|line| Line {
                words: line
                    .words
                    .iter()
                    .filter(|w| area.contains(centre(w.area)))
                    .cloned()
                    .collect(),
            })
            .filter(|line| !line.words.is_empty())
            .collect();
        Self { lines }
    }

    /// The text with words separated by spaces and lines by `\n`.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| {
                line.words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether no text was found.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

fn centre(area: Area) -> Vec2 {
    Vec2::new(
        f32::midpoint(area.min.x, area.max.x),
        f32::midpoint(area.min.y, area.max.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Recognized {
        Recognized {
            lines: vec![
                Line {
                    words: vec![
                        Word::new("Hello", 0.0, 0.0, 40.0, 10.0),
                        Word::new("world", 50.0, 0.0, 40.0, 10.0),
                    ],
                },
                Line {
                    words: vec![Word::new("below", 0.0, 20.0, 40.0, 10.0)],
                },
            ],
        }
    }

    #[test]
    fn text_joins_words_and_lines() {
        assert_eq!(sample().text(), "Hello world\nbelow");
        assert!(Recognized::default().is_empty());
    }

    #[test]
    fn within_keeps_words_centred_inside() {
        let crop = Area::from_corners(Vec2::new(45.0, 0.0), Vec2::new(100.0, 15.0));
        assert_eq!(sample().within(crop).text(), "world");
    }
}
