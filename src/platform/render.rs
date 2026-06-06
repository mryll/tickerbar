use chrono::{DateTime, Duration, Utc};

use crate::platform::config::{Config, Display, DisplayMode};
use crate::platform::icons::IconSet;
use crate::platform::model::{Direction, ProviderKind, Quote, QuoteState};
use crate::platform::theme::ThemeColors;
use crate::platform::waybar::{self, WaybarOutput};

/// Group the integer part with comma thousands separators: 68000.5 -> "68,000.50".
fn group_thousands(s: &str) -> String {
    let (int_part, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let neg = int_part.starts_with('-');
    let digits = int_part.trim_start_matches('-');
    let len = digits.len();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let sign = if neg { "-" } else { "" };
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

fn fmt_price(p: Option<f64>) -> String {
    match p {
        Some(v) if v.abs() >= 1000.0 => group_thousands(&format!("{:.2}", v)),
        Some(v) if v.abs() >= 1.0 => format!("{:.2}", v),
        Some(v) => format!("{:.4}", v),
        None => "—".to_string(),
    }
}

fn fmt_change(c: Option<f64>) -> String {
    match c {
        Some(v) => format!("{:+.2}%", v),
        None => String::new(),
    }
}

fn fmt_age(d: Duration) -> String {
    let secs = d.num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// Pad `s` on the left to `width` VISIBLE columns (Pango tags excluded). Right-aligns.
fn pad_left(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(waybar::visible_len(s));
    format!("{}{}", " ".repeat(pad), s)
}

/// Pad `s` on the right to `width` visible columns. Left-aligns.
fn pad_right(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(waybar::visible_len(s));
    format!("{}{}", s, " ".repeat(pad))
}

fn group_title(k: ProviderKind) -> &'static str {
    match k {
        ProviderKind::CoinGecko => "Crypto",
        ProviderKind::DolarApi => "Fiat · ARS",
        ProviderKind::Stooq => "Stocks",
        ProviderKind::Frankfurter => "Forex",
        ProviderKind::Finnhub => "Stocks",
    }
}

fn render_one(q: &Quote, fmt: &str, icons: &IconSet, colors: &ThemeColors) -> String {
    let price_plain = fmt_price(q.price);
    let change_plain = fmt_change(q.change_pct);
    let arrow = icons.arrow(q.direction);
    // Color the price + arrow + change% by direction (green up / red down), matching the
    // tooltip; label and glyph stay the theme foreground. Bar text is rendered with Pango.
    let (price_s, arrow_s, change_s) = match q.direction {
        Some(Direction::Up) => (
            waybar::fg(&colors.green, &price_plain),
            waybar::fg(&colors.green, arrow),
            waybar::fg(&colors.green, &change_plain),
        ),
        Some(Direction::Down) => (
            waybar::fg(&colors.red, &price_plain),
            waybar::fg(&colors.red, arrow),
            waybar::fg(&colors.red, &change_plain),
        ),
        _ => (price_plain, arrow.to_string(), change_plain),
    };
    fmt.replace("{label}", &waybar::pango_escape(&q.label))
        .replace("{price}", &price_s)
        .replace("{change_pct}", &change_s)
        .replace("{arrow}", &arrow_s)
        .replace("{glyph}", icons.kind_glyph(q.source))
        .trim()
        .to_string()
}

/// Assets shown on the bar, per display mode. `epoch` = unix seconds (rotation bucket).
fn visible<'a>(quotes: &'a [Quote], d: &Display, epoch: u64) -> Vec<&'a Quote> {
    match d.mode {
        DisplayMode::Fixed => quotes.iter().take(d.max_on_bar).collect(),
        DisplayMode::Rotate => {
            if quotes.is_empty() {
                return Vec::new();
            }
            let interval = d.rotate_interval.max(1);
            let idx = ((epoch / interval) as usize) % quotes.len();
            vec![&quotes[idx]]
        }
    }
}

pub fn bar_text(quotes: &[Quote], d: &Display, epoch: u64, colors: &ThemeColors) -> String {
    let icons = IconSet::from_name(&d.icons);
    visible(quotes, d, epoch)
        .iter()
        .map(|q| render_one(q, &d.bar_format, &icons, colors))
        .collect::<Vec<_>>()
        .join("   ")
}

/// Lifecycle classes from ALL quotes; direction class from the VISIBLE quotes only.
/// `partial` and `stale` can both apply (some missing AND some stale), so styling keeps
/// both signals instead of one masking the other.
pub fn module_class(all: &[Quote], visible: &[&Quote]) -> Vec<String> {
    let mut classes: Vec<String> = Vec::new();
    if all.iter().all(|q| q.price.is_none()) {
        classes.push("error".to_string());
    } else {
        if all.iter().any(|q| q.price.is_none()) {
            classes.push("partial".to_string());
        }
        if all.iter().any(|q| q.state == QuoteState::Stale) {
            classes.push("stale".to_string());
        }
        if classes.is_empty() {
            classes.push("ok".to_string());
        }
    }

    let dirs: Vec<Direction> = visible.iter().filter_map(|q| q.direction).collect();
    let direction = match dirs.first() {
        Some(first) if dirs.iter().all(|d| d == first) => match first {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Flat => "flat",
        },
        Some(_) => "mixed",
        None => "flat",
    };
    classes.push(direction.to_string());
    classes
}

pub fn build(
    cfg: &Config,
    quotes: &[Quote],
    now: DateTime<Utc>,
    colors: &ThemeColors,
) -> WaybarOutput {
    let epoch = now.timestamp().max(0) as u64;
    let vis = visible(quotes, &cfg.display, epoch);
    let text = bar_text(quotes, &cfg.display, epoch, colors);
    let class = module_class(quotes, &vis);
    // Tooltip ALWAYS uses the Nerd icon set for consistent monospace alignment, regardless
    // of the configured bar icon set (Pango renders emoji from a different font with
    // different metrics, breaking box/column alignment). Same rule meteobar uses.
    let tooltip = build_tooltip(quotes, colors, &IconSet::Nerd, now);
    WaybarOutput {
        text,
        tooltip,
        class,
        alt: "ticker".to_string(),
    }
}

/// Column-aligned, class-grouped, framed tooltip.
fn build_tooltip(
    quotes: &[Quote],
    colors: &ThemeColors,
    icons: &IconSet,
    now: DateTime<Utc>,
) -> String {
    // Column widths from PLAIN cell text (Pango tags don't occupy cells; pad_* measure
    // visible width, so plain widths align the colored cells).
    let label_w = quotes
        .iter()
        .map(|q| waybar::visible_len(&waybar::pango_escape(&q.label)))
        .max()
        .unwrap_or(0);
    let price_w = quotes
        .iter()
        .map(|q| waybar::visible_len(&fmt_price(q.price)))
        .max()
        .unwrap_or(0);
    let change_w = quotes
        .iter()
        .map(|q| waybar::visible_len(&fmt_change(q.change_pct)))
        .max()
        .unwrap_or(0);

    // Group by asset CLASS (so e.g. Stooq + Finnhub share one "Stocks" section), in a
    // fixed class order. Config order is preserved within each class.
    let classes = ["Crypto", "Fiat · ARS", "Stocks", "Forex"];
    let mut rows: Vec<String> = Vec::new();
    for class in classes {
        let group: Vec<&Quote> = quotes
            .iter()
            .filter(|q| group_title(q.source) == class)
            .collect();
        if group.is_empty() {
            continue;
        }
        // Show the per-row source tag only when a class mixes providers (e.g. stooq+finnhub).
        let multi = group
            .iter()
            .map(|q| q.source)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1;
        rows.push(format!(
            "  {} {}",
            waybar::fg(&colors.accent, icons.kind_glyph(group[0].source)),
            waybar::bold_fg(&colors.text, class)
        ));
        for q in &group {
            rows.push(render_row(
                q, label_w, price_w, change_w, now, colors, multi,
            ));
        }
    }

    let title = waybar::bold_fg(&colors.accent, "tickerbar");
    let footer_order = [
        ProviderKind::CoinGecko,
        ProviderKind::DolarApi,
        ProviderKind::Stooq,
        ProviderKind::Finnhub,
        ProviderKind::Frankfurter,
    ];
    let sources: Vec<&str> = footer_order
        .iter()
        .filter(|k| quotes.iter().any(|q| q.source == **k))
        .map(|k| k.as_str())
        .collect();
    let local = now.with_timezone(&chrono::Local);
    let footer = waybar::fg(
        &colors.dim,
        &format!(
            "  \u{f017}  {} · {}",
            local.format("%H:%M"),
            sources.join("·")
        ),
    );

    let mut measurable: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    measurable.push(footer.as_str());
    let width = waybar::content_width(&measurable).max(waybar::visible_len(&title));

    let mut lines = vec![waybar::top_border(width, &colors.border)];
    let left = width.saturating_sub(waybar::visible_len(&title)) / 2;
    lines.push(waybar::border_line(
        &format!("{}{}", " ".repeat(left), title),
        width,
        &colors.border,
    ));
    lines.push(waybar::separator(width, &colors.border, &colors.dim));
    for r in &rows {
        lines.push(waybar::border_line(r, width, &colors.border));
    }
    lines.push(waybar::separator(width, &colors.border, &colors.dim));
    lines.push(waybar::border_line(&footer, width, &colors.border));
    lines.push(waybar::bottom_border(width, &colors.border));
    lines.join("\n")
}

fn render_row(
    q: &Quote,
    label_w: usize,
    price_w: usize,
    change_w: usize,
    now: DateTime<Utc>,
    colors: &ThemeColors,
    show_source: bool,
) -> String {
    let dir_color = match q.direction {
        Some(Direction::Up) => &colors.green,
        Some(Direction::Down) => &colors.red,
        _ => &colors.text,
    };
    let label = pad_right(
        &waybar::bold_fg(&colors.text, &waybar::pango_escape(&q.label)),
        label_w,
    );
    let price = pad_left(&waybar::fg(dir_color, &fmt_price(q.price)), price_w);
    let change_plain = fmt_change(q.change_pct);
    let change = if change_plain.is_empty() {
        pad_left("", change_w)
    } else {
        pad_left(&waybar::fg(dir_color, &change_plain), change_w)
    };
    let mut tail = String::new();
    if show_source {
        tail.push_str(&format!("  · {}", q.source.as_str()));
    }
    match q.state {
        QuoteState::Stale => tail.push_str(&format!(
            "  (stale {})",
            fmt_age(now.signed_duration_since(q.fetched_at))
        )),
        QuoteState::Missing | QuoteState::Error => tail.push_str("  (n/d)"),
        QuoteState::Fresh => {}
    }
    let note = if tail.is_empty() {
        String::new()
    } else {
        waybar::fg(&colors.dim, &tail)
    };
    format!("    {}  {}  {}{}", label, price, change, note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::config::{Display, DisplayMode};
    use crate::platform::model::*;
    use chrono::Utc;

    fn quote(label: &str, price: Option<f64>, dir: Option<Direction>, state: QuoteState) -> Quote {
        Quote {
            label: label.into(),
            base: "b".into(),
            quote: "usd".into(),
            native_quote: "usd".into(),
            price,
            change_pct: dir.map(|_| 1.0),
            change_abs: None,
            direction: dir,
            source: ProviderKind::CoinGecko,
            as_of: None,
            fetched_at: Utc::now(),
            state,
        }
    }

    fn disp(mode: DisplayMode, max: usize) -> Display {
        Display {
            mode,
            rotate_interval: 5,
            max_on_bar: max,
            icons: "ascii".into(),
            bar_format: "{label} {price} {arrow}".into(),
        }
    }

    fn all_visible(qs: &[Quote]) -> Vec<&Quote> {
        qs.iter().collect()
    }

    #[test]
    fn fixed_mode_shows_at_most_max_on_bar_assets() {
        let qs = vec![
            quote("A", Some(1.0), Some(Direction::Up), QuoteState::Fresh),
            quote("B", Some(2.0), Some(Direction::Up), QuoteState::Fresh),
            quote("C", Some(3.0), Some(Direction::Up), QuoteState::Fresh),
        ];
        let text = bar_text(
            &qs,
            &disp(DisplayMode::Fixed, 2),
            0,
            &ThemeColors::default(),
        );
        assert!(text.contains('A') && text.contains('B'));
        assert!(!text.contains('C'));
    }

    #[test]
    fn rotation_selects_an_asset_by_time_bucket() {
        let qs = vec![
            quote("A", Some(1.0), None, QuoteState::Fresh),
            quote("B", Some(2.0), None, QuoteState::Fresh),
        ];
        let d = disp(DisplayMode::Rotate, 1);
        assert!(bar_text(&qs, &d, 0, &ThemeColors::default()).contains('A'));
        assert!(bar_text(&qs, &d, 5, &ThemeColors::default()).contains('B'));
    }

    #[test]
    fn the_module_class_is_mixed_when_visible_assets_disagree() {
        let qs = vec![
            quote("A", Some(1.0), Some(Direction::Up), QuoteState::Fresh),
            quote("B", Some(2.0), Some(Direction::Down), QuoteState::Fresh),
        ];
        let cls = module_class(&qs, &all_visible(&qs));
        assert!(cls.contains(&"mixed".to_string()));
        assert!(cls.contains(&"ok".to_string()));
    }

    #[test]
    fn a_single_visible_asset_is_never_mixed() {
        let qs = vec![
            quote("A", Some(1.0), Some(Direction::Up), QuoteState::Fresh),
            quote("B", Some(2.0), Some(Direction::Down), QuoteState::Fresh),
        ];
        let vis = vec![&qs[0]];
        let cls = module_class(&qs, &vis);
        assert!(cls.contains(&"up".to_string()));
        assert!(!cls.contains(&"mixed".to_string()));
    }

    #[test]
    fn the_lifecycle_class_is_partial_when_some_quotes_are_missing() {
        let qs = vec![
            quote("A", Some(1.0), Some(Direction::Up), QuoteState::Fresh),
            quote("B", None, None, QuoteState::Missing),
        ];
        assert!(module_class(&qs, &all_visible(&qs)).contains(&"partial".to_string()));
    }

    #[test]
    fn a_missing_price_renders_as_a_dash() {
        let qs = vec![quote("A", None, None, QuoteState::Missing)];
        let text = bar_text(
            &qs,
            &disp(DisplayMode::Fixed, 3),
            0,
            &ThemeColors::default(),
        );
        assert!(text.contains('—'));
    }

    #[test]
    fn prices_over_a_thousand_get_comma_separators() {
        assert_eq!(group_thousands("68000.50"), "68,000.50");
        assert_eq!(group_thousands("999"), "999");
        assert_eq!(group_thousands("-1234567"), "-1,234,567");
    }

    #[test]
    fn pad_left_aligns_to_visible_width_ignoring_pango_tags() {
        let cell = waybar::fg("#fff", "12");
        let padded = pad_left(&cell, 5);
        assert_eq!(waybar::visible_len(&padded), 5);
        assert!(padded.ends_with("12</span>"));
    }

    #[test]
    fn the_tooltip_groups_assets_by_class_with_crypto_before_stocks() {
        let qs = vec![
            quote("BTC", Some(68000.0), Some(Direction::Up), QuoteState::Fresh),
            Quote {
                source: ProviderKind::Stooq,
                ..quote("AAPL", Some(201.5), None, QuoteState::Fresh)
            },
        ];
        let tip = build_tooltip(&qs, &ThemeColors::default(), &IconSet::Nerd, Utc::now());
        let crypto_at = tip.find("Crypto").expect("Crypto header");
        let stocks_at = tip.find("Stocks").expect("Stocks header");
        assert!(crypto_at < stocks_at);
        assert!(tip.contains("68,000.00"));
    }
}
