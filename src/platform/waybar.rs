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

pub fn fg(color: &str, text: &str) -> String {
    format!("<span foreground='{color}'>{text}</span>")
}

pub fn bold_fg(color: &str, text: &str) -> String {
    format!("<span font_weight='bold' foreground='{color}'>{text}</span>")
}

pub fn border_line(content: &str, width: usize, border_color: &str) -> String {
    let pad = width.saturating_sub(visible_len(content));
    let right_pad = " ".repeat(pad);
    format!(
        "{} {content}{right_pad} {}",
        fg(border_color, "│"),
        fg(border_color, "│")
    )
}

pub fn separator(width: usize, border_color: &str, dim_color: &str) -> String {
    border_line(&fg(dim_color, &"─".repeat(width)), width, border_color)
}

pub fn empty_line(width: usize, border_color: &str) -> String {
    border_line(&" ".repeat(width), width, border_color)
}

pub fn top_border(width: usize, border_color: &str) -> String {
    fg(border_color, &format!("╭{}╮", "─".repeat(width + 2)))
}

pub fn bottom_border(width: usize, border_color: &str) -> String {
    fg(border_color, &format!("╰{}╯", "─".repeat(width + 2)))
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

/// Generic bordered error box for the never-crash fallback paths.
pub fn error_output(
    title: &str,
    message: &str,
    colors: &crate::platform::theme::ThemeColors,
) -> WaybarOutput {
    let header = bold_fg(&colors.error, &format!("  {}", pango_escape(title)));
    let body = fg(&colors.dim, &format!("  {}", pango_escape(message)));

    let width = content_width(&[&header, &body]);

    let lines = [
        top_border(width, &colors.border),
        border_line(&header, width, &colors.border),
        separator(width, &colors.border, &colors.dim),
        border_line(&body, width, &colors.border),
        bottom_border(width, &colors.border),
    ];

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
}
