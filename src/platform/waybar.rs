use serde::Serialize;
use unicode_width::UnicodeWidthStr;

#[derive(Serialize)]
pub struct WaybarOutput {
    pub text: String,
    pub tooltip: String,
    pub class: Vec<String>,
    pub alt: String,
}

const MIN_WIDTH: usize = 20;

pub fn pango_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Tint `text`. An EMPTY color emits no markup at all — that is the single mechanism
/// behind `--no-color`: monochrome surfaces are handed a `ThemeColors::monochrome()`
/// whose fields are all empty, so no `foreground=` reaches the output while structure,
/// glyphs and padding are untouched (`visible_len` ignores tags either way).
pub fn fg(color: &str, text: &str) -> String {
    if color.is_empty() {
        return text.to_string();
    }
    format!("<span foreground='{color}'>{text}</span>")
}

/// Weight is structure, not color: monochrome keeps the bold span and drops only the tint.
pub fn bold_fg(color: &str, text: &str) -> String {
    if color.is_empty() {
        return format!("<span font_weight='bold'>{text}</span>");
    }
    format!("<span font_weight='bold' foreground='{color}'>{text}</span>")
}

/// Visible (rendered) width of a string, ignoring Pango tags and counting entities as one.
pub fn visible_len(s: &str) -> usize {
    let mut plain = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut in_entity = false;

    for ch in s.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            continue;
        }
        if in_entity {
            if ch == ';' {
                in_entity = false;
                plain.push('x'); // entity counts as 1 visible cell
            }
            continue;
        }
        match ch {
            '<' => in_tag = true,
            '&' => in_entity = true,
            _ => plain.push(ch),
        }
    }

    plain.width()
}

pub fn content_width(items: &[&str]) -> usize {
    items
        .iter()
        .map(|c| visible_len(c))
        .max()
        .unwrap_or(MIN_WIDTH)
        .max(MIN_WIDTH)
}

/// Generic error tooltip for the never-crash fallback paths. Left unpinned on
/// purpose: it is one header plus one line, so there is no column to keep and
/// no rule long enough to overshoot — and the config, which is where the font
/// is named, may be exactly what failed to load here.
pub fn error_output(
    title: &str,
    message: &str,
    colors: &crate::platform::theme::ThemeColors,
) -> WaybarOutput {
    let header = bold_fg(&colors.error, &format!("  {}", pango_escape(title)));
    let body = fg(&colors.dim, &format!("  {}", pango_escape(message)));

    let width = content_width(&[&header, &body]);

    let lines = [header, fg(&colors.dim, &"─".repeat(width)), body];

    WaybarOutput {
        text: "?".to_string(),
        tooltip: lines.join("\n"),
        class: vec!["error".to_string()],
        alt: "error".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pango_escape_replaces_markup_metacharacters() {
        assert_eq!(pango_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn visible_len_ignores_pango_tags_and_counts_entities_as_one() {
        let s = fg("#fff", "AB&amp;C"); // visible: A B & C = 4
        assert_eq!(visible_len(&s), 4);
    }

    #[test]
    fn an_empty_color_emits_the_text_with_no_markup_at_all() {
        assert_eq!(fg("", "AB"), "AB");
    }

    #[test]
    fn an_empty_color_keeps_the_bold_weight_and_drops_only_the_tint() {
        let bold = bold_fg("", "AB");
        assert_eq!(bold, "<span font_weight='bold'>AB</span>");
        assert!(!bold.contains("foreground"));
    }

    #[test]
    fn a_tinted_and_an_untinted_cell_measure_the_same_width() {
        assert_eq!(visible_len(&fg("#fff", "12")), visible_len(&fg("", "12")));
        assert_eq!(
            visible_len(&bold_fg("#fff", "12")),
            visible_len(&bold_fg("", "12"))
        );
    }
}
