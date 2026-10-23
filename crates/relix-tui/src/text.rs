/*
 * @author t0bx
 * @since 23.10.2026
 */

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn width(text: &str) -> usize {
    text.width()
}

/// Truncates to at most `max` display columns, ending with `…` when cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if used + ch_width + 1 > max {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

/// Pads (or truncates) to exactly `width` display columns.
pub fn fit(text: &str, width: usize) -> String {
    let mut out = truncate(text, width);
    let current = out.width();
    out.extend(std::iter::repeat_n(' ', width.saturating_sub(current)));
    out
}

pub fn fit_right(text: &str, width: usize) -> String {
    let truncated = truncate(text, width);
    let padding = width.saturating_sub(truncated.width());
    format!("{}{truncated}", " ".repeat(padding))
}

/// Display width of the first `chars` characters of `text`.
pub fn width_of_prefix(text: &str, chars: usize) -> usize {
    text.chars()
        .take(chars)
        .map(|ch| ch.width().unwrap_or(0))
        .sum()
}

/// Characters of `text` starting at char index `skip` that fit into `max` columns.
pub fn visible_slice(text: &str, skip: usize, max: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars().skip(skip) {
        let ch = if ch == '\t' { ' ' } else { ch };
        let ch_width = ch.width().unwrap_or(0);
        if used + ch_width > max {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_respects_wide_characters() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(width(&fit("日本語テキスト", 6)), 6);
        assert_eq!(fit_right("7", 3), "  7");
        assert_eq!(visible_slice("a日b", 1, 2), "日");
        assert_eq!(width_of_prefix("日a", 1), 2);
    }
}
