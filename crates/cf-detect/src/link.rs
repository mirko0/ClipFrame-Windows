//! Deciding whether decoded text may be opened in the browser.

/// Longest link offered for opening.
const MAX_LEN: usize = 2048;

/// `text` (trimmed) when it is a plain `http`/`https` link: no other schemes (`file:`,
/// `javascript:`, custom protocol handlers…), no whitespace or control characters.
pub fn web_url(text: &str) -> Option<&str> {
    let url = text.trim();
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let scheme_ok = lower.starts_with("https://") || lower.starts_with("http://");
    let rest = url.split_once("://").map_or("", |(_, rest)| rest);
    let clean = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    (scheme_ok && clean && !rest.is_empty() && url.len() <= MAX_LEN).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_web_links_are_openable() {
        assert_eq!(
            web_url(" https://example.com/x "),
            Some("https://example.com/x")
        );
        assert_eq!(web_url("HTTP://Example.com"), Some("HTTP://Example.com"));
        assert_eq!(web_url("javascript:alert(1)"), None);
        assert_eq!(web_url("file:///C:/Windows"), None);
        assert_eq!(web_url("ms-settings:privacy"), None);
        assert_eq!(web_url("https://"), None);
        assert_eq!(web_url("https://a.com/x y"), None);
        assert_eq!(web_url("https://a.com/\u{7}"), None);
        assert_eq!(web_url("WIFI:S:home;T:WPA;P:secret;;"), None);
    }
}
