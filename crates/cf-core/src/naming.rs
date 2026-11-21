//! File names from templates: `{date}`, `{time}`, `{app}`, `{title}`, `{n}` and friends.
//!
//! Every name is made safe for Windows (no reserved characters or device names, no trailing
//! dots or spaces, bounded length), whatever a window title contains.

use std::path::{Path, PathBuf};

use crate::output::{LocalDateTime, unique_path};

/// The default template: `ClipFrame 2026-11-19 at 23.41.05`.
pub const DEFAULT_TEMPLATE: &str = "ClipFrame {date} at {time}";

/// Longest file stem produced, in characters (keeps full paths well under `MAX_PATH`).
const MAX_STEM: usize = 120;

/// Where a capture came from, for `{app}` and `{title}`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    /// The foreground program's executable name without extension, e.g. `chrome`.
    pub app: String,
    /// The foreground window's title.
    pub title: String,
}

/// How saved files are named and filed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Naming {
    /// Template for the file stem; see the module docs for tokens.
    pub template: String,
    /// Put files into a `YYYY-MM` subfolder.
    pub date_folders: bool,
}

impl Default for Naming {
    fn default() -> Self {
        Self {
            template: DEFAULT_TEMPLATE.to_owned(),
            date_folders: false,
        }
    }
}

impl Naming {
    /// A free `.png` path in `dir` (or its month subfolder) for a capture taken at `time`.
    /// `{n}` counts up from 1 until the name is free; without it, ` (2)`, ` (3)`, … are
    /// appended when needed. `exists` is injected for testing.
    pub fn path(
        &self,
        dir: &Path,
        time: LocalDateTime,
        source: &Source,
        exists: impl Fn(&Path) -> bool,
    ) -> PathBuf {
        let dir = if self.date_folders {
            dir.join(format!("{:04}-{:02}", time.year, time.month))
        } else {
            dir.to_path_buf()
        };
        let template = if self.template.trim().is_empty() {
            DEFAULT_TEMPLATE
        } else {
            &self.template
        };
        let file = |n| format!("{}.png", render(template, time, source, n));
        if template.contains("{n}") {
            if let Some(free) = (1..10_000)
                .map(|n| dir.join(file(Some(n))))
                .find(|p| !exists(p))
            {
                return free;
            }
        }
        unique_path(&dir, &file(None), exists)
    }
}

/// The template with tokens replaced, made safe as a Windows file stem.
pub fn render(template: &str, t: LocalDateTime, source: &Source, n: Option<u32>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let token = &after[..close];
        match value(token, t, source, n) {
            Some(text) => out.push_str(&text),
            None => out.push_str(&rest[open..=open + close + 1]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    sanitize(&out)
}

fn value(token: &str, t: LocalDateTime, source: &Source, n: Option<u32>) -> Option<String> {
    Some(match token {
        "date" => format!("{:04}-{:02}-{:02}", t.year, t.month, t.day),
        "time" => format!("{:02}.{:02}.{:02}", t.hour, t.minute, t.second),
        "year" => format!("{:04}", t.year),
        "month" => format!("{:02}", t.month),
        "day" => format!("{:02}", t.day),
        "hour" => format!("{:02}", t.hour),
        "minute" => format!("{:02}", t.minute),
        "second" => format!("{:02}", t.second),
        "app" => source.app.clone(),
        "title" => source.title.clone(),
        "n" => n.unwrap_or(1).to_string(),
        _ => return None,
    })
}

/// Replaces characters Windows forbids, collapses whitespace, trims trailing dots and
/// spaces, bounds the length and avoids reserved device names.
pub fn sanitize(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let bounded: String = collapsed.chars().take(MAX_STEM).collect();
    let trimmed = bounded.trim_end_matches(['.', ' ']).trim_start();
    if trimmed.is_empty() {
        return "ClipFrame".to_owned();
    }
    if is_reserved(trimmed) {
        return format!("_{trimmed}");
    }
    trimmed.to_owned()
}

fn is_reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base[3..].chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: LocalDateTime = LocalDateTime {
        year: 2026,
        month: 3,
        day: 9,
        hour: 7,
        minute: 5,
        second: 3,
    };

    fn chrome() -> Source {
        Source {
            app: "chrome".into(),
            title: "Docs: \"Q1\" plan | Draft*".into(),
        }
    }

    #[test]
    fn tokens_are_replaced_and_unknown_ones_kept() {
        let name = render("{app} {date} {hour}{minute} {other} {", T, &chrome(), None);
        assert_eq!(name, "chrome 2026-03-09 0705 {other} {");
        assert_eq!(
            render(DEFAULT_TEMPLATE, T, &Source::default(), None),
            "ClipFrame 2026-03-09 at 07.05.03"
        );
    }

    #[test]
    fn titles_are_made_safe() {
        assert_eq!(
            render("{title}", T, &chrome(), None),
            "Docs_ _Q1_ plan _ Draft_"
        );
        assert_eq!(sanitize("  name. . "), "name");
        assert_eq!(sanitize("a\tb\nc"), "a b c");
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("com1.txt"), "_com1.txt");
        assert_eq!(sanitize("COMPANY"), "COMPANY");
        assert_eq!(sanitize("..."), "ClipFrame");
        assert_eq!(sanitize(&"x".repeat(300)).chars().count(), MAX_STEM);
    }

    #[test]
    fn counter_finds_the_first_free_number() {
        let naming = Naming {
            template: "shot {n}".into(),
            date_folders: false,
        };
        let taken = [
            PathBuf::from("C:/x/shot 1.png"),
            PathBuf::from("C:/x/shot 2.png"),
        ];
        let path = naming.path(Path::new("C:/x"), T, &Source::default(), |p| {
            taken.iter().any(|t| t == p)
        });
        assert_eq!(path, Path::new("C:/x/shot 3.png"));
    }

    #[test]
    fn date_folders_and_fallback_template() {
        let naming = Naming {
            template: "  ".into(),
            date_folders: true,
        };
        let path = naming.path(Path::new("C:/x"), T, &Source::default(), |_| false);
        assert_eq!(
            path,
            Path::new("C:/x/2026-03/ClipFrame 2026-03-09 at 07.05.03.png")
        );
    }
}
