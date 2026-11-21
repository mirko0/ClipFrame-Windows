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

    /// Every word in reading order, with the index of its line.
    pub fn words(&self) -> impl Iterator<Item = (usize, &Word)> {
        self.lines
            .iter()
            .enumerate()
            .flat_map(|(line, l)| l.words.iter().map(move |w| (line, w)))
    }

    /// Index (in reading order) of the word under `p`, allowing `slop` pixels around it.
    pub fn word_at(&self, p: Vec2, slop: f32) -> Option<usize> {
        self.words()
            .position(|(_, w)| grow(w.area, slop).contains(p))
    }

    /// The word a text selection reaches when dragged to `p`: on the line nearest `p`
    /// vertically, the word nearest horizontally (so dragging past a line's end selects its
    /// last word).
    pub fn nearest_word(&self, p: Vec2) -> Option<usize> {
        let line = self
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| line_area(l).map(|a| (i, gap(p.y, a.min.y, a.max.y))))
            .min_by(|a, b| a.1.total_cmp(&b.1))?
            .0;
        let first = self.lines[..line]
            .iter()
            .map(|l| l.words.len())
            .sum::<usize>();
        let within = self.lines[line]
            .words
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let (da, db) = (
                    gap(p.x, a.area.min.x, a.area.max.x),
                    gap(p.x, b.area.min.x, b.area.max.x),
                );
                da.total_cmp(&db)
            })?
            .0;
        Some(first + within)
    }

    /// The text of words `a` to `b` (either order, inclusive): spaces between words of a
    /// line, line breaks between lines.
    pub fn text_between(&self, a: usize, b: usize) -> String {
        let (from, to) = (a.min(b), a.max(b));
        let mut out = String::new();
        let mut last_line = None;
        for (line, word) in self.words().skip(from).take(to - from + 1) {
            match last_line {
                Some(l) if l == line => out.push(' '),
                Some(_) => out.push('\n'),
                None => {}
            }
            out.push_str(&word.text);
            last_line = Some(line);
        }
        out
    }

    /// The areas of words `a` to `b` (either order, inclusive), for highlighting.
    pub fn areas_between(&self, a: usize, b: usize) -> Vec<Area> {
        let (from, to) = (a.min(b), a.max(b));
        self.words()
            .skip(from)
            .take(to - from + 1)
            .map(|(_, w)| w.area)
            .collect()
    }
}

fn line_area(line: &Line) -> Option<Area> {
    let first = line.words.first()?.area;
    Some(line.words.iter().fold(first, |acc, w| Area {
        min: Vec2::new(acc.min.x.min(w.area.min.x), acc.min.y.min(w.area.min.y)),
        max: Vec2::new(acc.max.x.max(w.area.max.x), acc.max.y.max(w.area.max.y)),
    }))
}

/// Distance from `v` to the span `min..=max` (0 inside).
fn gap(v: f32, min: f32, max: f32) -> f32 {
    if v < min {
        min - v
    } else if v > max {
        v - max
    } else {
        0.0
    }
}

fn grow(area: Area, by: f32) -> Area {
    Area {
        min: Vec2::new(area.min.x - by, area.min.y - by),
        max: Vec2::new(area.max.x + by, area.max.y + by),
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
    fn words_are_found_under_the_pointer() {
        let text = sample();
        assert_eq!(text.word_at(Vec2::new(60.0, 5.0), 0.0), Some(1));
        assert_eq!(
            text.word_at(Vec2::new(45.0, 5.0), 0.0),
            None,
            "between words"
        );
        assert_eq!(
            text.word_at(Vec2::new(45.0, 5.0), 6.0),
            Some(0),
            "with slop"
        );
        assert_eq!(Recognized::default().word_at(Vec2::default(), 5.0), None);
    }

    #[test]
    fn dragging_reaches_the_nearest_word_in_reading_order() {
        let text = sample();
        assert_eq!(
            text.nearest_word(Vec2::new(500.0, 3.0)),
            Some(1),
            "past the line end"
        );
        assert_eq!(
            text.nearest_word(Vec2::new(-20.0, 25.0)),
            Some(2),
            "left of line two"
        );
        assert_eq!(
            text.nearest_word(Vec2::new(10.0, 900.0)),
            Some(2),
            "far below"
        );
        assert_eq!(Recognized::default().nearest_word(Vec2::default()), None);
    }

    #[test]
    fn selected_text_keeps_spaces_and_line_breaks() {
        let text = sample();
        assert_eq!(text.text_between(0, 2), "Hello world\nbelow");
        assert_eq!(text.text_between(2, 1), "world\nbelow", "either order");
        assert_eq!(text.text_between(1, 1), "world");
        assert_eq!(text.areas_between(0, 1).len(), 2);
    }

    #[test]
    fn within_keeps_words_centred_inside() {
        let crop = Area::from_corners(Vec2::new(45.0, 0.0), Vec2::new(100.0, 15.0));
        assert_eq!(sample().within(crop).text(), "world");
    }
}
