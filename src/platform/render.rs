use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use crate::platform::config::{Config, Display, DisplayMode, MarketHours};
use crate::platform::icons::IconSet;
use crate::platform::market::{self, Gate};
use crate::platform::model::{Asset, AssetSource, Direction, Panel, Quote, QuoteState};
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
    let mut class = module_class(quotes, &vis);
    // `closed` class when every asset currently on the bar is in a closed market.
    let all_closed = !vis.is_empty()
        && vis.iter().all(|q| {
            matches!(
                market::gate(q.source, now, &cfg.market_hours),
                Gate::Closed { .. }
            )
        });
    if all_closed {
        class.push("closed".to_string());
    }
    let tooltip = build_tooltip(
        &cfg.assets,
        quotes,
        &cfg.display,
        &cfg.market_hours,
        colors,
        now,
    );
    WaybarOutput {
        text,
        tooltip,
        class,
        alt: "ticker".to_string(),
    }
}

// ---- Tooltip ---------------------------------------------------------------------------

/// Tooltip section. Derived from the asset's source (see `group_of`), so the same display
/// grouping is NOT stored in cached market data.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum TooltipGroup {
    Crypto,
    FiatArs,
    AccionesAr,
    Bonos,
    Cedears,
    On,
    Stocks,
    Forex,
}

const GROUP_ORDER: [TooltipGroup; 8] = [
    TooltipGroup::Crypto,
    TooltipGroup::FiatArs,
    TooltipGroup::AccionesAr,
    TooltipGroup::Bonos,
    TooltipGroup::Cedears,
    TooltipGroup::On,
    TooltipGroup::Stocks,
    TooltipGroup::Forex,
];

impl TooltipGroup {
    fn label(self) -> &'static str {
        match self {
            TooltipGroup::Crypto => "Crypto",
            TooltipGroup::FiatArs => "Fiat · ARS",
            TooltipGroup::AccionesAr => "AR Stocks",
            TooltipGroup::Bonos => "AR Bonds",
            TooltipGroup::Cedears => "CEDEARs",
            TooltipGroup::On => "Corp Bonds",
            TooltipGroup::Stocks => "Stocks",
            TooltipGroup::Forex => "Forex",
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            TooltipGroup::Crypto => "\u{f15a}",     // bitcoin
            TooltipGroup::FiatArs => "\u{f155}",    // dollar
            TooltipGroup::AccionesAr => "\u{f201}", // line chart
            TooltipGroup::Bonos => "\u{f0d6}",      // money
            TooltipGroup::Cedears => "\u{f0ac}",    // globe
            TooltipGroup::On => "\u{f1ad}",         // building
            TooltipGroup::Stocks => "\u{f201}",     // line chart
            TooltipGroup::Forex => "\u{f0ec}",      // exchange
        }
    }
}

fn group_of(src: &AssetSource) -> TooltipGroup {
    match src {
        AssetSource::Coingecko { .. } => TooltipGroup::Crypto,
        AssetSource::Dolarapi { .. } => TooltipGroup::FiatArs,
        AssetSource::Stooq { .. } | AssetSource::Finnhub { .. } | AssetSource::Cnbc { .. } => {
            TooltipGroup::Stocks
        }
        AssetSource::Frankfurter { .. } => TooltipGroup::Forex,
        AssetSource::Data912 { panel, .. } => match panel {
            Panel::Acciones => TooltipGroup::AccionesAr,
            Panel::Bonos => TooltipGroup::Bonos,
            Panel::Cedears => TooltipGroup::Cedears,
            Panel::Corp => TooltipGroup::On,
        },
    }
}

enum TooltipLine {
    Header {
        group: TooltipGroup,
        continued: bool,
    },
    Row {
        group: TooltipGroup,
        text: String,
    },
}

/// Column-aligned, class-grouped, optionally multi-column framed tooltip.
fn build_tooltip(
    assets: &[Asset],
    quotes: &[Quote],
    display: &Display,
    market: &MarketHours,
    colors: &ThemeColors,
    now: DateTime<Utc>,
) -> String {
    // fetch_all returns quotes in config order, so assets and quotes stay aligned 1:1.
    debug_assert_eq!(assets.len(), quotes.len());
    // Inner data-column widths are GLOBAL (computed across all quotes) so every data row is
    // uniform regardless of which tooltip-column it lands in.
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

    // Distinct sources per group (config order), shown dim next to the section title.
    let mut group_sources: HashMap<TooltipGroup, Vec<&'static str>> = HashMap::new();
    // A group is "closed" only when every source in it is currently closed.
    let mut group_closed: HashMap<TooltipGroup, bool> = HashMap::new();
    for (a, q) in assets.iter().zip(quotes) {
        let g = group_of(&a.source);
        let v = group_sources.entry(g).or_default();
        let s = q.source.as_str();
        if !v.contains(&s) {
            v.push(s);
        }
        let closed_now = matches!(market::gate(q.source, now, market), Gate::Closed { .. });
        group_closed
            .entry(g)
            .and_modify(|c| *c = *c && closed_now)
            .or_insert(closed_now);
    }

    // Flat list of structured lines, grouped by section in a fixed order.
    let mut lines: Vec<TooltipLine> = Vec::new();
    for group in GROUP_ORDER {
        let members: Vec<&Quote> = assets
            .iter()
            .zip(quotes)
            .filter(|(a, _)| group_of(&a.source) == group)
            .map(|(_, q)| q)
            .collect();
        if members.is_empty() {
            continue;
        }
        lines.push(TooltipLine::Header {
            group,
            continued: false,
        });
        for q in members {
            lines.push(TooltipLine::Row {
                group,
                text: render_row(q, label_w, price_w, change_w, now, colors),
            });
        }
    }
    if lines.is_empty() {
        lines.push(TooltipLine::Row {
            group: TooltipGroup::Crypto,
            text: waybar::fg(&colors.dim, "    no assets configured"),
        });
    }

    // Split into columns if configured.
    let n = display.tooltip_rows_per_column;
    let columns: Vec<Vec<TooltipLine>> = if n == 0 || lines.len() <= n {
        vec![lines]
    } else {
        chunk_columns(lines, n)
    };

    // Render each column to padded strings, then join side-by-side.
    let render_line = |l: &TooltipLine| match l {
        TooltipLine::Header { group, continued } => render_header(
            *group,
            *continued,
            group_closed.get(group).copied().unwrap_or(false),
            &group_sources,
            colors,
        ),
        TooltipLine::Row { text, .. } => text.clone(),
    };
    let col_strs: Vec<Vec<String>> = columns
        .iter()
        .map(|c| c.iter().map(&render_line).collect())
        .collect();
    let sep = waybar::fg(&colors.dim, " │ ");
    // Cap columns per band; extra columns wrap into a new band stacked below (narrow/vertical
    // monitors). When not banding, keep per-column widths so the layout is unchanged.
    let max_cols = display.tooltip_max_columns;
    let banding = max_cols > 0 && col_strs.len() > max_cols;
    let band_size = if banding {
        max_cols
    } else {
        col_strs.len().max(1)
    };
    let uniform_w = col_strs
        .iter()
        .flat_map(|c| c.iter())
        .map(|s| waybar::visible_len(s))
        .max()
        .unwrap_or(0);
    let per_col_w: Vec<usize> = col_strs
        .iter()
        .map(|c| c.iter().map(|s| waybar::visible_len(s)).max().unwrap_or(0))
        .collect();

    // Each band is a stacked grid of up to `band_size` columns, with its own height.
    let mut bands: Vec<Vec<String>> = Vec::new();
    for (bi, band) in col_strs.chunks(band_size).enumerate() {
        let band_height = band.iter().map(|c| c.len()).max().unwrap_or(0);
        let mut lines = Vec::with_capacity(band_height);
        for r in 0..band_height {
            let cells: Vec<String> = band
                .iter()
                .enumerate()
                .map(|(ci, col)| {
                    let w = if banding {
                        uniform_w
                    } else {
                        per_col_w[bi * band_size + ci]
                    };
                    pad_right(col.get(r).map(|s| s.as_str()).unwrap_or(""), w)
                })
                .collect();
            lines.push(cells.join(&sep));
        }
        bands.push(lines);
    }

    // Frame.
    let title = waybar::bold_fg(&colors.accent, "tickerbar");
    let local = now.with_timezone(&chrono::Local);
    let footer = waybar::fg(
        &colors.dim,
        &format!("  \u{f017}  Updated {}", local.format("%H:%M")),
    );
    let mut measurable: Vec<&str> = bands.iter().flatten().map(|s| s.as_str()).collect();
    measurable.push(footer.as_str());
    let width = waybar::content_width(&measurable).max(waybar::visible_len(&title));

    let mut out = vec![waybar::top_border(width, &colors.border)];
    let left = width.saturating_sub(waybar::visible_len(&title)) / 2;
    out.push(waybar::border_line(
        &format!("{}{}", " ".repeat(left), title),
        width,
        &colors.border,
    ));
    out.push(waybar::separator(width, &colors.border, &colors.dim));
    for (i, band) in bands.iter().enumerate() {
        if i > 0 {
            out.push(waybar::separator(width, &colors.border, &colors.dim));
        }
        for line in band {
            out.push(waybar::border_line(line, width, &colors.border));
        }
    }
    out.push(waybar::separator(width, &colors.border, &colors.dim));
    out.push(waybar::border_line(&footer, width, &colors.border));
    out.push(waybar::bottom_border(width, &colors.border));
    out.join("\n")
}

fn render_header(
    group: TooltipGroup,
    continued: bool,
    closed: bool,
    sources: &HashMap<TooltipGroup, Vec<&'static str>>,
    colors: &ThemeColors,
) -> String {
    let src = sources.get(&group).map(|v| v.join("·")).unwrap_or_default();
    let src_part = if src.is_empty() {
        String::new()
    } else {
        waybar::fg(&colors.dim, &format!(" ({src})"))
    };
    let cont_part = if continued {
        waybar::fg(&colors.dim, " (cont.)")
    } else {
        String::new()
    };
    let closed_part = if closed {
        waybar::fg(&colors.dim, "  \u{f04c} closed") // pause glyph
    } else {
        String::new()
    };
    format!(
        "  {} {}{}{}{}",
        waybar::fg(&colors.accent, group.glyph()),
        waybar::bold_fg(&colors.text, group.label()),
        src_part,
        cont_part,
        closed_part
    )
}

fn render_row(
    q: &Quote,
    label_w: usize,
    price_w: usize,
    change_w: usize,
    now: DateTime<Utc>,
    colors: &ThemeColors,
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
    let note = match q.state {
        QuoteState::Stale => waybar::fg(
            &colors.dim,
            &format!(
                "  (stale {})",
                fmt_age(now.signed_duration_since(q.fetched_at))
            ),
        ),
        QuoteState::Missing | QuoteState::Error => waybar::fg(&colors.dim, "  (n/d)"),
        QuoteState::Fresh => String::new(),
    };
    format!("    {}  {}  {}{}", label, price, change, note)
}

/// Split lines into columns of ~`n` lines, avoiding a column that ends on a section header
/// and inserting a `(cont.)` header when a section spills into the next column.
fn chunk_columns(lines: Vec<TooltipLine>, n: usize) -> Vec<Vec<TooltipLine>> {
    let mut cols: Vec<Vec<TooltipLine>> = Vec::new();
    let mut cur: Vec<TooltipLine> = Vec::new();
    for l in lines {
        if cur.len() >= n {
            cols.push(std::mem::take(&mut cur));
        }
        cur.push(l);
    }
    if !cur.is_empty() {
        cols.push(cur);
    }
    // A column must not end on a header — push a trailing header to the next column.
    let last = cols.len().saturating_sub(1);
    for i in 0..last {
        if matches!(cols[i].last(), Some(TooltipLine::Header { .. })) {
            let h = cols[i].pop().unwrap();
            cols[i + 1].insert(0, h);
        }
    }
    // Drop any column emptied by the orphan move (e.g. tooltip_rows_per_column = 1).
    cols.retain(|c| !c.is_empty());
    // A column that starts mid-section gets a continuation header.
    for col in cols.iter_mut().skip(1) {
        let cont_group = match col.first() {
            Some(TooltipLine::Row { group, .. }) => Some(*group),
            _ => None,
        };
        if let Some(group) = cont_group {
            col.insert(
                0,
                TooltipLine::Header {
                    group,
                    continued: true,
                },
            );
        }
    }
    cols
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::config::{Display, DisplayMode, MarketHours};
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
            tooltip_rows_per_column: 0,
            tooltip_max_columns: 0,
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
        let assets = vec![
            Asset {
                label: "BTC".into(),
                source: AssetSource::Coingecko {
                    id: "bitcoin".into(),
                    quote: "usd".into(),
                },
            },
            Asset {
                label: "AAPL".into(),
                source: AssetSource::Stooq {
                    symbol: "aapl.us".into(),
                },
            },
        ];
        let qs = vec![
            quote("BTC", Some(68000.0), Some(Direction::Up), QuoteState::Fresh),
            Quote {
                source: ProviderKind::Stooq,
                ..quote("AAPL", Some(201.5), None, QuoteState::Fresh)
            },
        ];
        let tip = build_tooltip(
            &assets,
            &qs,
            &disp(DisplayMode::Fixed, 3),
            &MarketHours::default(),
            &ThemeColors::default(),
            Utc::now(),
        );
        let crypto_at = tip.find("Crypto").expect("Crypto header");
        let stocks_at = tip.find("Stocks").expect("Stocks header");
        assert!(crypto_at < stocks_at);
        assert!(tip.contains("68,000.00"));
        // source shown next to the panel title (dim), not a footer source list
        assert!(tip.contains("coingecko"));
        assert!(tip.contains("Updated"));
    }

    // ---- multi-column helpers ----

    fn header(g: TooltipGroup) -> TooltipLine {
        TooltipLine::Header {
            group: g,
            continued: false,
        }
    }
    fn row(g: TooltipGroup) -> TooltipLine {
        TooltipLine::Row {
            group: g,
            text: "x".into(),
        }
    }
    fn ends_with_header(col: &[TooltipLine]) -> bool {
        matches!(col.last(), Some(TooltipLine::Header { .. }))
    }

    #[test]
    fn a_column_never_ends_on_a_header() {
        // naive chunk of [R,R,H,R] by 3 would put H last in column 0.
        let lines = vec![
            row(TooltipGroup::Crypto),
            row(TooltipGroup::Crypto),
            header(TooltipGroup::Stocks),
            row(TooltipGroup::Stocks),
        ];
        let cols = chunk_columns(lines, 3);
        assert!(cols.iter().all(|c| !ends_with_header(c)));
    }

    #[test]
    fn a_section_split_across_columns_gets_a_continuation_header() {
        // [H, R, R, R] by 2 -> column 1 starts mid-section -> gets a continued header.
        let lines = vec![
            header(TooltipGroup::On),
            row(TooltipGroup::On),
            row(TooltipGroup::On),
            row(TooltipGroup::On),
        ];
        let cols = chunk_columns(lines, 2);
        assert!(cols.len() >= 2);
        assert!(matches!(
            cols[1].first(),
            Some(TooltipLine::Header {
                continued: true,
                ..
            })
        ));
    }

    #[test]
    fn a_single_row_per_column_does_not_produce_empty_columns() {
        let lines = vec![header(TooltipGroup::Crypto), row(TooltipGroup::Crypto)];
        let cols = chunk_columns(lines, 1);
        assert!(cols.iter().all(|c| !c.is_empty()));
    }

    #[test]
    fn exceeding_max_columns_wraps_into_stacked_bands() {
        let assets: Vec<Asset> = (0..12)
            .map(|i| Asset {
                label: format!("S{i}"),
                source: AssetSource::Cnbc {
                    symbol: format!("S{i}"),
                },
            })
            .collect();
        let qs: Vec<Quote> = (0..12)
            .map(|i| Quote {
                source: ProviderKind::Cnbc,
                ..quote(
                    &format!("S{i}"),
                    Some(100.0 + i as f64),
                    Some(Direction::Up),
                    QuoteState::Fresh,
                )
            })
            .collect();
        let mut d = disp(DisplayMode::Fixed, 3);
        d.tooltip_rows_per_column = 4;
        let mut unlimited = d.clone();
        unlimited.tooltip_max_columns = 0;
        let wide = build_tooltip(
            &assets,
            &qs,
            &unlimited,
            &MarketHours::default(),
            &ThemeColors::default(),
            Utc::now(),
        );
        d.tooltip_max_columns = 2;
        let banded = build_tooltip(
            &assets,
            &qs,
            &d,
            &MarketHours::default(),
            &ThemeColors::default(),
            Utc::now(),
        );
        assert!(
            banded.lines().count() > wide.lines().count(),
            "banding should stack columns into more (shorter) rows"
        );
        let widths: Vec<usize> = banded.lines().map(waybar::visible_len).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "framed lines equal width"
        );
    }

    #[test]
    fn all_tooltip_lines_have_equal_visible_width() {
        let assets: Vec<Asset> = (0..10)
            .map(|i| Asset {
                label: format!("SYM{i}"),
                source: AssetSource::Cnbc {
                    symbol: format!("S{i}"),
                },
            })
            .collect();
        let qs: Vec<Quote> = (0..10)
            .map(|i| Quote {
                source: ProviderKind::Cnbc,
                ..quote(
                    &format!("SYM{i}"),
                    Some(100.0 + i as f64),
                    Some(Direction::Up),
                    QuoteState::Fresh,
                )
            })
            .collect();
        let mut d = disp(DisplayMode::Fixed, 3);
        d.tooltip_rows_per_column = 4; // force multiple columns
        let tip = build_tooltip(
            &assets,
            &qs,
            &d,
            &MarketHours::default(),
            &ThemeColors::default(),
            Utc::now(),
        );
        let widths: Vec<usize> = tip.lines().map(waybar::visible_len).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "all framed lines equal width"
        );
    }
}
