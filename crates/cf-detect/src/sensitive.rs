//! Spotting sensitive text in OCR results, so the editor can offer to redact it.
//!
//! Detection is deliberately simple and local (no regex engine): it errs on the side of
//! suggesting too much, because every suggestion is an ordinary, undoable redaction.

use cf_core::annotate::{Area, Vec2};
use cf_core::ocr::{Recognized, Word};

/// Margin added around flagged words, in pixels.
const PAD: f32 = 2.0;

/// What kind of sensitive text was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An email address.
    Email,
    /// An IPv4 or IPv6 address.
    IpAddress,
    /// A long number: phone, card or account number (may span several words).
    Number,
    /// A key, token or password-like string.
    Token,
}

/// One piece of sensitive text and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// What it looks like.
    pub kind: Kind,
    /// Its bounds in image pixels, slightly padded.
    pub area: Area,
}

/// Everything in `text` that looks sensitive.
pub fn find(text: &Recognized) -> Vec<Finding> {
    let mut found = Vec::new();
    for line in &text.lines {
        let mut flagged = vec![false; line.words.len()];
        for (index, word) in line.words.iter().enumerate() {
            if let Some(kind) = word_kind(&word.text) {
                flagged[index] = true;
                found.push(finding(kind, std::slice::from_ref(word)));
            }
        }
        for run in number_runs(&line.words, &flagged) {
            found.push(finding(Kind::Number, &line.words[run]));
        }
    }
    found
}

/// What a single word is, if sensitive on its own.
pub fn word_kind(raw: &str) -> Option<Kind> {
    let word = trim_punctuation(raw);
    if is_email(word) {
        Some(Kind::Email)
    } else if is_ipv4(word) || is_ipv6(word) {
        Some(Kind::IpAddress)
    } else if is_token(word) {
        Some(Kind::Token)
    } else {
        None
    }
}

fn finding(kind: Kind, words: &[Word]) -> Finding {
    let mut area = words[0].area;
    for word in &words[1..] {
        area = Area {
            min: Vec2::new(
                area.min.x.min(word.area.min.x),
                area.min.y.min(word.area.min.y),
            ),
            max: Vec2::new(
                area.max.x.max(word.area.max.x),
                area.max.y.max(word.area.max.y),
            ),
        };
    }
    let area = Area {
        min: Vec2::new(area.min.x - PAD, area.min.y - PAD),
        max: Vec2::new(area.max.x + PAD, area.max.y + PAD),
    };
    Finding { kind, area }
}

/// Runs of adjacent number-like words (not already flagged) that together hold a phone,
/// card or account number: 10–19 digits, or 8+ when it starts with `+`.
fn number_runs(words: &[Word], flagged: &[bool]) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start = None;
    for index in 0..=words.len() {
        let numeric = index < words.len() && !flagged[index] && is_numberish(&words[index].text);
        match (numeric, start) {
            (true, None) => start = Some(index),
            (false, Some(first)) => {
                if is_long_number(&words[first..index]) {
                    runs.push(first..index);
                }
                start = None;
            }
            _ => {}
        }
    }
    runs
}

fn is_long_number(words: &[Word]) -> bool {
    let digits: usize = words
        .iter()
        .map(|w| w.text.chars().filter(char::is_ascii_digit).count())
        .sum();
    let international = words
        .first()
        .is_some_and(|w| trim_punctuation(&w.text).starts_with('+'));
    let min = if international { 8 } else { 10 };
    (min..=19).contains(&digits)
}

fn is_numberish(raw: &str) -> bool {
    let word = trim_punctuation(raw);
    !word.is_empty()
        && word.chars().any(|c| c.is_ascii_digit())
        && word
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '(' | ')' | '-' | '.' | '/'))
}

fn is_email(word: &str) -> bool {
    let Some((local, domain)) = word.split_once('@') else {
        return false;
    };
    let allowed = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let top_level = domain.rsplit('.').next().unwrap_or("");
    // OCR may glue junk to the front (a clipped first letter reads as `•nail@…`), so only
    // the characters right before `@` have to form a valid local part.
    let local_part = local.chars().rev().take_while(|&c| allowed(c)).count();
    local_part > 0
        && domain.contains('.')
        && top_level.chars().count() >= 2
        && domain.chars().all(allowed)
}

fn is_ipv4(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 3
                && p.chars().all(|c| c.is_ascii_digit())
                && p.parse::<u16>().is_ok_and(|n| n <= 255)
        })
}

fn is_ipv6(word: &str) -> bool {
    let colons = word.matches(':').count();
    colons >= 2
        && (word.contains("::") || colons >= 5)
        && word.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
        && word.chars().any(|c| c.is_ascii_hexdigit())
}

/// Long strings mixing letters and digits, like API keys, hashes and serials.
fn is_token(word: &str) -> bool {
    let allowed =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+' | '=');
    let letters = word.chars().filter(char::is_ascii_alphabetic).count();
    let digits = word.chars().filter(char::is_ascii_digit).count();
    word.chars().all(allowed) && letters + digits >= 12 && letters >= 3 && digits >= 3
}

fn trim_punctuation(word: &str) -> &str {
    word.trim_matches(|c: char| {
        matches!(
            c,
            ',' | ';' | ':' | '"' | '\'' | '<' | '>' | '[' | ']' | '{' | '}'
        )
    })
    .trim_end_matches(['.', '!', '?', ')'])
    .trim_start_matches('(')
}

#[cfg(test)]
mod tests {
    use cf_core::ocr::Line;

    use super::*;

    fn line(words: &[&str]) -> Recognized {
        let words = words
            .iter()
            .enumerate()
            .map(|(i, text)| {
                #[allow(clippy::cast_precision_loss)] // tiny test indices
                let x = i as f32 * 100.0;
                Word::new(*text, x, 10.0, 90.0, 20.0)
            })
            .collect();
        Recognized {
            lines: vec![Line { words }],
        }
    }

    fn kinds(words: &[&str]) -> Vec<Kind> {
        find(&line(words)).into_iter().map(|f| f.kind).collect()
    }

    #[test]
    fn single_word_kinds() {
        assert_eq!(word_kind("mail@example.com,"), Some(Kind::Email));
        assert_eq!(word_kind("•nail@example.com"), Some(Kind::Email));
        assert_eq!(word_kind("•@example.com"), None);
        assert_eq!(word_kind("(192.168.1.20)"), Some(Kind::IpAddress));
        assert_eq!(word_kind("fe80::1ff:fe23:4567:890a"), Some(Kind::IpAddress));
        assert_eq!(word_kind("4f9a-22c1-b7e0"), Some(Kind::Token));
        assert_eq!(word_kind("sk_live_51HxT9k2eZvKYlo2C"), Some(Kind::Token));
        for harmless in [
            "hello",
            "v0.1.0",
            "15:11:29",
            "2026-11-21T15:11:29.389813Z",
            "@user",
            "300.1.1.1",
        ] {
            assert_eq!(word_kind(harmless), None, "{harmless}");
        }
    }

    #[test]
    fn numbers_may_span_words() {
        assert_eq!(
            kinds(&["Call", "+1", "555", "123", "4567", "now"]),
            [Kind::Number]
        );
        assert_eq!(kinds(&["(555)", "123-4567", "x"]), [Kind::Number]);
        assert_eq!(kinds(&["4111", "1111", "1111", "1111"]), [Kind::Number]);
        assert!(kinds(&["on", "2026-11-21", "at", "15:30"]).is_empty());
        assert!(kinds(&["room", "12"]).is_empty());
    }

    #[test]
    fn a_finding_covers_all_its_words_with_padding() {
        let found = find(&line(&["tel", "+44", "20", "7946", "0958"]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].area.min, Vec2::new(98.0, 8.0));
        assert_eq!(found[0].area.max, Vec2::new(492.0, 32.0));
    }

    #[test]
    fn flagged_words_are_not_counted_again_as_numbers() {
        assert_eq!(
            kinds(&["10.0.0.1", "10.0.0.2"]),
            [Kind::IpAddress, Kind::IpAddress]
        );
    }
}
