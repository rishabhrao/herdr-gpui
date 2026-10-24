//! Checks for untrusted text shown in the app.

/// A control or direction-override character, which untrusted text loses.
pub fn unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{2028}' | '\u{2029}')
}

/// `text` cut to `limit` characters, without the characters `unsafe_char`
/// refuses.
pub fn safe_text(text: &str, limit: usize) -> String {
    // Bound scanning as well as output, even for a payload made entirely of controls.
    text.chars()
        .take(limit)
        .filter(|c| !unsafe_char(*c))
        .collect()
}
