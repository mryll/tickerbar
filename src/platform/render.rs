use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

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

/// Signed change %, EXCEPT for a flat quote: a `+` on something that did not move reads as
/// a gain, so `Flat` renders unsigned with a leading space (printf's space flag) — same
/// visible width, so the column stays aligned. The decision is the model's `Direction`, not
/// the rounded number: a tiny-but-nonzero move that prints as `0.00` is `Up`/`Down` and
/// keeps its real sign.
fn fmt_change(c: Option<f64>, dir: Option<Direction>) -> String {
    match c {
        Some(v) if dir == Some(Direction::Flat) => format!(" {:.2}%", v.abs()),
        Some(v) => format!("{:+.2}%", v),
        None => String::new(),
    }
}

/// Equal-weight mean of `change_pct` over quotes whose change is `Some` and finite.
/// Returns `None` when no quote has a usable change (=> the summary segment is omitted) or if
/// the computed mean is non-finite (overflow guard) — preserves the never-crash invariant.
/// Shared with the structured JSON output (`platform::data`).
pub(crate) fn avg_change_pct(quotes: &[Quote]) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0u32;
    for q in quotes {
        if let Some(v) = q.change_pct {
            if v.is_finite() {
                sum += v;
                count += 1;
            }
        }
    }
    if count == 0 {
        return None;
    }
    let avg = sum / f64::from(count);
    avg.is_finite().then_some(avg)
}

/// Format an asset's value for display. Rates (yields) render as a percent ("4.53%");
/// every other class uses the currency-style price formatter. Used for BOTH the column-width
/// measurement and the actual render so yields stay column-aligned.
fn fmt_value(price: Option<f64>, group: TooltipGroup) -> String {
    match (group, price) {
        (TooltipGroup::Rates, Some(v)) => format!("{v:.2}%"),
        _ => fmt_price(price),
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

fn render_one(
    q: &Quote,
    group: TooltipGroup,
    fmt: &str,
    icons: &IconSet,
    colors: &ThemeColors,
) -> String {
    let price_plain = fmt_value(q.price, group);
    let change_plain = fmt_change(q.change_pct, q.direction);
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
        .replace("{glyph}", group.glyph_for(*icons))
        .trim()
        .to_string()
}

/// Render the bar-level summary segment from the equal-weighted average change %.
/// Mirrors `render_one`: only the {avg_change}/{avg_arrow} substitutions are colored
/// (green up / red down); a flat average is left unspanned. Literal text is untouched.
fn render_summary(avg: f64, fmt: &str, icons: &IconSet, colors: &ThemeColors) -> String {
    let dir = Direction::from_change(avg);
    let change_plain = fmt_change(Some(avg), Some(dir));
    let arrow = icons.arrow(Some(dir));
    let (arrow_s, change_s) = match dir {
        Direction::Up => (
            waybar::fg(&colors.green, arrow),
            waybar::fg(&colors.green, &change_plain),
        ),
        Direction::Down => (
            waybar::fg(&colors.red, arrow),
            waybar::fg(&colors.red, &change_plain),
        ),
        Direction::Flat => (arrow.to_string(), change_plain),
    };
    fmt.replace("{avg_change}", &change_s)
        .replace("{avg_arrow}", &arrow_s)
        .trim()
        .to_string()
}

/// Replace {bar} and {summary} in `layout` in a single left-to-right pass. Unknown {tokens}
/// are left verbatim; inserted content is never re-scanned (so a value containing a brace
/// token cannot be corrupted).
fn apply_layout(layout: &str, bar: &str, summary: &str) -> String {
    let mut out = String::with_capacity(layout.len() + bar.len() + summary.len());
    let mut rest = layout;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => {
                let token = &rest[open..=open + close];
                match token {
                    "{bar}" => out.push_str(bar),
                    "{summary}" => out.push_str(summary),
                    other => out.push_str(other),
                }
                rest = &rest[open + close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Candidate asset indices for the bar, in display order. `bar` empty => all assets in config
/// order; otherwise each label resolves to the FIRST matching asset index, in `bar` order,
/// skipping unknown labels, de-duplicated preserving first occurrence.
fn bar_candidates(assets: &[Asset], bar: &[String]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    if bar.is_empty() {
        out.extend(0..assets.len());
    } else {
        for name in bar {
            if let Some(i) = assets.iter().position(|a| &a.label == name) {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
    }
    out
}

/// Asset indices actually shown on the bar, per display mode — the single source of truth for
/// bar text, module direction class, the `closed` badge, and the structured output's `bar` list.
/// `epoch` = unix seconds (bucket).
pub(crate) fn bar_indices(assets: &[Asset], d: &Display, epoch: u64) -> Vec<usize> {
    let candidates = bar_candidates(assets, &d.bar);
    match d.mode {
        DisplayMode::Fixed => candidates.into_iter().take(d.max_on_bar).collect(),
        DisplayMode::Rotate => {
            if candidates.is_empty() {
                return Vec::new();
            }
            let interval = d.rotate_interval.max(1);
            vec![candidates[(epoch / interval) as usize % candidates.len()]]
        }
    }
}

pub fn bar_text(
    assets: &[Asset],
    quotes: &[Quote],
    d: &Display,
    epoch: u64,
    colors: &ThemeColors,
) -> String {
    let icons = IconSet::from_name(&d.icons);
    let bar = bar_indices(assets, d, epoch)
        .iter()
        .filter_map(|&i| {
            Some(render_one(
                quotes.get(i)?,
                group_of(&assets.get(i)?.source),
                &d.bar_format,
                &icons,
                colors,
            ))
        })
        .collect::<Vec<_>>()
        .join("   ");

    let summary = if d.summary_format.is_empty() {
        String::new()
    } else {
        match avg_change_pct(quotes) {
            Some(avg) => render_summary(avg, &d.summary_format, &icons, colors),
            None => String::new(),
        }
    };

    // Compose by which block is non-empty: literal separators in `bar_layout` only appear when
    // both blocks exist, so an empty block never leaves a dangling separator.
    match (bar.is_empty(), summary.is_empty()) {
        (true, true) => String::new(),
        (false, true) => bar,
        (true, false) => summary,
        (false, false) => apply_layout(&d.bar_layout, &bar, &summary),
    }
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

/// Which of the two Pango surfaces render in color. The structured JSON (`platform::data`)
/// carries no presentation and is unaffected by this; so is the waybar `class` list, which
/// is precisely what lets a monochrome user style the module from their own CSS.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorMode {
    bar: bool,
    tooltip: bool,
}

impl ColorMode {
    /// Colored bar and tooltip — the default.
    pub const FULL: Self = Self {
        bar: true,
        tooltip: true,
    };
    /// `--no-color` / `--no-color=all`.
    pub const NONE: Self = Self {
        bar: false,
        tooltip: false,
    };
    /// `--no-color=bar`: plain bar text, colored tooltip.
    pub const PLAIN_BAR: Self = Self {
        bar: false,
        tooltip: true,
    };
    /// `--no-color=tooltip`: colored bar text, plain tooltip.
    pub const PLAIN_TOOLTIP: Self = Self {
        bar: true,
        tooltip: false,
    };

    pub fn bar(self) -> bool {
        self.bar
    }

    pub fn tooltip(self) -> bool {
        self.tooltip
    }
}

impl Default for ColorMode {
    fn default() -> Self {
        Self::FULL
    }
}

pub fn build(
    cfg: &Config,
    quotes: &[Quote],
    now: DateTime<Utc>,
    colors: &ThemeColors,
    mode: ColorMode,
) -> WaybarOutput {
    // Monochrome is a palette, not a branch in the renderers: each surface is handed
    // either the theme or the empty palette, so plain and colored output travel the exact
    // same code path and the measured column widths cannot drift apart.
    let mono = ThemeColors::monochrome();
    let bar_colors = if mode.bar() { colors } else { &mono };
    let tooltip_colors = if mode.tooltip() { colors } else { &mono };

    let epoch = now.timestamp().max(0) as u64;
    let idx = bar_indices(&cfg.assets, &cfg.display, epoch);
    let vis: Vec<&Quote> = idx.iter().filter_map(|&i| quotes.get(i)).collect();
    let text = bar_text(&cfg.assets, quotes, &cfg.display, epoch, bar_colors);
    let mut class = module_class(quotes, &vis);
    // `closed` class when every asset currently on the bar is in a closed market.
    let all_closed = !idx.is_empty()
        && idx.iter().all(|&i| {
            cfg.assets
                .get(i)
                .map(|a| {
                    matches!(
                        market::gate(&a.source, now, &cfg.market_hours),
                        Gate::Closed { .. }
                    )
                })
                .unwrap_or(false)
        });
    if all_closed {
        class.push("closed".to_string());
    }
    let tooltip = build_tooltip(
        &cfg.assets,
        quotes,
        &cfg.display,
        &cfg.market_hours,
        tooltip_colors,
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
/// grouping is NOT stored in cached market data. Also the section vocabulary of the
/// structured JSON output (`platform::data`), keeping one source of truth for grouping.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum TooltipGroup {
    Crypto,
    FiatArs,
    AccionesAr,
    Bonos,
    Cedears,
    On,
    Stocks,
    Indices,
    Commodities,
    Rates,
    Forex,
}

pub(crate) const GROUP_ORDER: [TooltipGroup; 11] = [
    TooltipGroup::Crypto,
    TooltipGroup::FiatArs,
    TooltipGroup::AccionesAr,
    TooltipGroup::Bonos,
    TooltipGroup::Cedears,
    TooltipGroup::On,
    TooltipGroup::Stocks,
    TooltipGroup::Indices,
    TooltipGroup::Commodities,
    TooltipGroup::Rates,
    TooltipGroup::Forex,
];

impl TooltipGroup {
    pub(crate) fn label(self) -> &'static str {
        match self {
            TooltipGroup::Crypto => "Crypto",
            TooltipGroup::FiatArs => "Fiat · ARS",
            TooltipGroup::AccionesAr => "AR Stocks",
            TooltipGroup::Bonos => "AR Bonds",
            TooltipGroup::Cedears => "CEDEARs",
            TooltipGroup::On => "Corp Bonds",
            TooltipGroup::Stocks => "Stocks",
            TooltipGroup::Indices => "Indices",
            TooltipGroup::Commodities => "Commodities",
            TooltipGroup::Rates => "Rates",
            TooltipGroup::Forex => "Forex",
        }
    }
    /// Nerd-glyph for the class. Shared with the structured JSON output (`platform::data`).
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            TooltipGroup::Crypto => "\u{f15a}",      // bitcoin
            TooltipGroup::FiatArs => "\u{f155}",     // dollar
            TooltipGroup::AccionesAr => "\u{f201}",  // line chart
            TooltipGroup::Bonos => "\u{f0d6}",       // money
            TooltipGroup::Cedears => "\u{f0ac}",     // globe
            TooltipGroup::On => "\u{f1ad}",          // building
            TooltipGroup::Stocks => "\u{f201}",      // line chart
            TooltipGroup::Indices => "\u{f1fe}",     // area chart
            TooltipGroup::Commodities => "\u{f1b2}", // cube
            TooltipGroup::Rates => "\u{f295}",       // percent
            TooltipGroup::Forex => "\u{f0ec}",       // exchange
        }
    }

    /// Class glyph for the bar `{glyph}` placeholder, honoring the configured icon set so the
    /// bar matches the tooltip per asset class (commodities/indices/rates are no longer shown
    /// with the generic stocks glyph).
    fn glyph_for(self, set: IconSet) -> &'static str {
        match set {
            IconSet::Ascii => "",
            IconSet::Nerd => self.glyph(),
            IconSet::Emoji => match self {
                TooltipGroup::Crypto => "🪙",
                TooltipGroup::FiatArs => "💵",
                TooltipGroup::AccionesAr => "📈",
                TooltipGroup::Bonos => "📜",
                TooltipGroup::Cedears => "🌎",
                TooltipGroup::On => "🏢",
                TooltipGroup::Stocks => "📊",
                TooltipGroup::Indices => "📈",
                TooltipGroup::Commodities => "🛢",
                TooltipGroup::Rates => "🏦",
                TooltipGroup::Forex => "💱",
            },
        }
    }
}

pub(crate) fn group_of(src: &AssetSource) -> TooltipGroup {
    match src {
        AssetSource::Coingecko { .. } => TooltipGroup::Crypto,
        AssetSource::Dolarapi { .. } => TooltipGroup::FiatArs,
        AssetSource::Finnhub { .. } | AssetSource::Cnbc { .. } => TooltipGroup::Stocks,
        AssetSource::Commodity { .. } => TooltipGroup::Commodities,
        AssetSource::Index { .. } => TooltipGroup::Indices,
        AssetSource::Rate { .. } => TooltipGroup::Rates,
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
        text: String,
    },
}

/// Column-aligned, class-grouped, optionally multi-column tooltip.
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
    // Framed = box + Mono Nerd Font pin (columns aligned under any bar font). Plain
    // (default) = no border/pin, and chrome glyphs (group/closed/clock) are dropped
    // so nothing in measured/columnar content depends on Nerd glyph metrics.
    // Inner data-column widths are GLOBAL (computed across all quotes) so every data row is
    // uniform regardless of which tooltip-column it lands in.
    let label_w = quotes
        .iter()
        .map(|q| waybar::visible_len(&waybar::pango_escape(&q.label)))
        .max()
        .unwrap_or(0);
    let price_w = assets
        .iter()
        .zip(quotes)
        .map(|(a, q)| waybar::visible_len(&fmt_value(q.price, group_of(&a.source))))
        .max()
        .unwrap_or(0);
    let change_w = quotes
        .iter()
        .map(|q| waybar::visible_len(&fmt_change(q.change_pct, q.direction)))
        .max()
        .unwrap_or(0);
    let widths = ColWidths {
        label: label_w,
        price: price_w,
        change: change_w,
    };

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
        let closed_now = matches!(market::gate(&a.source, now, market), Gate::Closed { .. });
        group_closed
            .entry(g)
            .and_modify(|c| *c = *c && closed_now)
            .or_insert(closed_now);
    }

    // Grouped member quotes in the fixed section order (non-empty groups only) — the
    // same grouped shape the packing plan indexes into.
    let grouped: Vec<(TooltipGroup, Vec<&Quote>)> = GROUP_ORDER
        .into_iter()
        .filter_map(|group| {
            let members: Vec<&Quote> = assets
                .iter()
                .zip(quotes)
                .filter(|(a, _)| group_of(&a.source) == group)
                .map(|(_, q)| q)
                .collect();
            (!members.is_empty()).then_some((group, members))
        })
        .collect();

    // Column packing comes from the ONE shared plan (`pack_columns`) — the same
    // function the structured JSON exposes to other frontends.
    let columns: Vec<Vec<TooltipLine>> = if grouped.is_empty() {
        vec![vec![TooltipLine::Row {
            text: waybar::fg(&colors.dim, "    no assets configured"),
        }]]
    } else {
        let sizes: Vec<usize> = grouped.iter().map(|(_, m)| m.len()).collect();
        pack_columns(&sizes, display.tooltip_rows_per_column)
            .iter()
            .map(|col| {
                let mut out: Vec<TooltipLine> = Vec::new();
                for seg in col {
                    let (group, members) = &grouped[seg.group];
                    out.push(TooltipLine::Header {
                        group: *group,
                        continued: seg.continued,
                    });
                    for q in &members[seg.start..seg.start + seg.len] {
                        out.push(TooltipLine::Row {
                            text: render_row(
                                q,
                                *group,
                                display.tooltip_range,
                                &widths,
                                now,
                                colors,
                            ),
                        });
                    }
                }
                out
            })
            .collect()
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
    // The column divider is box drawing: safe now that the tooltip pins a
    // monospace font, and it is what makes the table read as a table.
    let sep = waybar::fg(&colors.dim, " │ ");
    // Cap columns per band; extra columns wrap into a new band stacked below (narrow/vertical
    // monitors). When not banding, keep per-column widths so the layout is unchanged.
    let band_size = band_size(col_strs.len(), display.tooltip_max_columns);
    let banding = band_size < col_strs.len();
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
    // House freshness footer: the clock glyph (nf-md-clock_outline) and
    // "Updated HH:MM", the same closing line every sibling widget uses in both
    // frontends. Plain mode keeps the glyph too — it has no right edge for a
    // mismeasured advance to push out of true.
    // Lifecycle suffix, matching the Omarchy panel's footer: the timestamp says
    // WHEN, and a non-fresh snapshot says WHY right after it. Without this the
    // Waybar tooltip claimed a plain "Updated HH:MM" for prices the fetch could
    // not refresh — the class field carried the fact, but nothing visible did.
    // The tooltip shows every asset, so the lifecycle here is the whole set's.
    let lifecycle = {
        let all_missing = quotes.iter().all(|q| q.price.is_none());
        let any_missing = quotes.iter().any(|q| q.price.is_none());
        let any_stale = quotes.iter().any(|q| q.state == QuoteState::Stale);
        if all_missing && !quotes.is_empty() {
            Some("error")
        } else if any_missing {
            Some("partial")
        } else if any_stale {
            Some("stale")
        } else {
            None
        }
    };
    let footer_suffix = match lifecycle {
        Some("partial") => " · partial data".to_string(),
        Some(other) => format!(" · {other}"),
        None => String::new(),
    };
    let footer = waybar::fg(
        &colors.dim,
        &format!(
            "  \u{f0150}  Updated {}{footer_suffix}",
            local.format("%H:%M")
        ),
    );
    let mut measurable: Vec<&str> = bands.iter().flatten().map(|s| s.as_str()).collect();
    measurable.push(footer.as_str());
    let width = waybar::content_width(&measurable).max(waybar::visible_len(&title));

    // One tooltip shape, pinned to a monospace font. The pin is not decoration:
    // this tooltip is a TABLE, and its rules are box-drawing characters. In a
    // proportional font the columns stop lining up and the rules render far
    // wider than the text they underline, so the tooltip sizes itself to the
    // rules and grows a dead margin on its right. Waybar draws the tooltip in a
    // GTK window that IGNORES font-family from CSS, so the markup is the only
    // place this can be said.
    let rule = || waybar::fg(&colors.dim, &"─".repeat(width));

    let mut out: Vec<String> = vec![title.clone(), rule()];
    for (i, band) in bands.iter().enumerate() {
        if i > 0 {
            out.push(rule());
        }
        for line in band {
            out.push(line.clone());
        }
    }
    out.push(rule());
    out.push(footer.clone());

    let body = out.join("\n");
    format!(
        "<span font_family='{}'>{body}</span>",
        waybar::pango_escape(&display.tooltip_font).replace('\'', "&apos;")
    )
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
    // The Nerd glyphs are safe on every path now: the tooltip pins a monospace
    // font, so their PUA advance is one cell and the columns below still align.
    let glyph_part = format!("{} ", waybar::fg(&colors.accent, group.glyph()));
    let closed_part = if closed {
        waybar::fg(&colors.dim, "  \u{f04c} closed")
    } else {
        String::new()
    };
    format!(
        "  {}{}{}{}{}",
        glyph_part,
        waybar::bold_fg(&colors.text, group.label()),
        src_part,
        cont_part,
        closed_part
    )
}

/// Shared inner-column widths, computed once across all quotes so every row is uniform.
struct ColWidths {
    label: usize,
    price: usize,
    change: usize,
}

/// The colour a quote's `direction` paints in. THE single source of truth: the Waybar
/// renderer paints with it and `platform::data` publishes it in the structured document's
/// `palette`, so no frontend has to re-derive direction colours from an accent and the two
/// frontends cannot disagree about the same number.
pub fn direction_color(d: Option<Direction>, colors: &ThemeColors) -> &str {
    match d {
        Some(Direction::Up) => &colors.green,
        Some(Direction::Down) => &colors.red,
        _ => &colors.text,
    }
}

fn render_row(
    q: &Quote,
    group: TooltipGroup,
    show_range: bool,
    w: &ColWidths,
    now: DateTime<Utc>,
    colors: &ThemeColors,
) -> String {
    let dir_color = direction_color(q.direction, colors);
    let label = pad_right(
        &waybar::bold_fg(&colors.text, &waybar::pango_escape(&q.label)),
        w.label,
    );
    let price = pad_left(&waybar::fg(dir_color, &fmt_value(q.price, group)), w.price);
    let change_plain = fmt_change(q.change_pct, q.direction);
    let change = if change_plain.is_empty() {
        pad_left("", w.change)
    } else {
        pad_left(&waybar::fg(dir_color, &change_plain), w.change)
    };
    // Dim suffix in the note area (not a measured column): optional intraday range, then state.
    let mut note = String::new();
    if show_range {
        if let (Some(lo), Some(hi)) = (q.day_low, q.day_high) {
            note.push_str(&waybar::fg(
                &colors.dim,
                &format!(
                    "  {}-{}",
                    fmt_value(Some(lo), group),
                    fmt_value(Some(hi), group)
                ),
            ));
        }
    }
    note.push_str(&match q.state {
        QuoteState::Stale => waybar::fg(
            &colors.dim,
            &format!(
                "  (stale {})",
                fmt_age(now.signed_duration_since(q.fetched_at))
            ),
        ),
        QuoteState::Missing | QuoteState::Error => waybar::fg(&colors.dim, "  (n/d)"),
        QuoteState::Fresh => String::new(),
    });
    format!("    {}  {}  {}{}", label, price, change, note)
}

/// One packed run of consecutive rows from a single (non-empty) display group inside one
/// wrapped column. `group` indexes the grouped list the plan was computed from, `start`/`len`
/// select the run of that group's rows, and `continued` marks a section that spilled over
/// from the previous column (rendered as a "(cont.)" header).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackedSegment {
    pub group: usize,
    pub start: usize,
    pub len: usize,
    pub continued: bool,
}

/// THE column-packing logic, shared by every frontend (the waybar tooltip renders it, the
/// structured JSON exposes it verbatim). Splits grouped rows into wrapped columns of
/// ~`rows_per_column` lines (1 header line per group + 1 line per row), with the rules:
/// a column never ends on a header (it moves to the next column's top; columns emptied by
/// that move are dropped), and a column starting mid-section opens a `continued` segment.
/// `rows_per_column == 0` (or fewer total lines than the budget) = single column.
pub(crate) fn pack_columns(
    group_sizes: &[usize],
    rows_per_column: usize,
) -> Vec<Vec<PackedSegment>> {
    #[derive(Clone, Copy)]
    enum Ln {
        Header(usize),
        Row(usize, usize),
    }
    let mut lines: Vec<Ln> = Vec::new();
    for (g, &size) in group_sizes.iter().enumerate() {
        if size == 0 {
            continue;
        }
        lines.push(Ln::Header(g));
        for r in 0..size {
            lines.push(Ln::Row(g, r));
        }
    }
    if lines.is_empty() {
        return Vec::new();
    }

    let n = rows_per_column;
    let mut cols: Vec<Vec<Ln>> = Vec::new();
    if n == 0 || lines.len() <= n {
        cols.push(lines);
    } else {
        let mut cur: Vec<Ln> = Vec::new();
        for l in lines {
            if cur.len() >= n {
                cols.push(std::mem::take(&mut cur));
            }
            cur.push(l);
        }
        if !cur.is_empty() {
            cols.push(cur);
        }
    }
    // A column must not end on a header — push a trailing header to the next column.
    let last = cols.len().saturating_sub(1);
    for i in 0..last {
        if matches!(cols[i].last(), Some(Ln::Header(_))) {
            let h = cols[i].pop().unwrap();
            cols[i + 1].insert(0, h);
        }
    }
    // Drop any column emptied by the orphan move (e.g. rows_per_column = 1).
    cols.retain(|c| !c.is_empty());

    // Compress each column's lines into segments. A row with no open segment for its
    // group means the section spilled from the previous column => continued header.
    cols.iter()
        .map(|col| {
            let mut segs: Vec<PackedSegment> = Vec::new();
            for l in col {
                match *l {
                    Ln::Header(g) => segs.push(PackedSegment {
                        group: g,
                        start: 0,
                        len: 0,
                        continued: false,
                    }),
                    Ln::Row(g, r) => match segs.last_mut() {
                        Some(s) if s.group == g && (s.len == 0 || s.start + s.len == r) => {
                            if s.len == 0 {
                                s.start = r;
                            }
                            s.len += 1;
                        }
                        _ => segs.push(PackedSegment {
                            group: g,
                            start: r,
                            len: 1,
                            continued: true,
                        }),
                    },
                }
            }
            segs
        })
        .collect()
}

/// Columns per side-by-side band: capped at `max_columns` when positive and exceeded
/// (extra columns wrap into bands stacked below); otherwise all columns share one band.
/// Shared by the tooltip renderer and the structured JSON.
pub(crate) fn band_size(n_cols: usize, max_columns: usize) -> usize {
    if max_columns > 0 && n_cols > max_columns {
        max_columns
    } else {
        n_cols.max(1)
    }
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
            day_high: None,
            day_low: None,
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
            summary_format: String::new(),
            bar: Vec::new(),
            bar_layout: "{summary}   {bar}".into(),
            tooltip_rows_per_column: 0,
            tooltip_max_columns: 0,
            tooltip_range: false,
            frame: false,
            tooltip_font: "JetBrainsMono Nerd Font Mono, JetBrainsMono Nerd Font, monospace".into(),
        }
    }

    // Quote with an explicit change_pct (the `quote()` helper above hardcodes 1.0).
    fn quote_chg(label: &str, change: Option<f64>) -> Quote {
        let mut q = quote(
            label,
            Some(1.0),
            change.map(Direction::from_change),
            QuoteState::Fresh,
        );
        q.change_pct = change;
        q
    }

    fn all_visible(qs: &[Quote]) -> Vec<&Quote> {
        qs.iter().collect()
    }

    #[test]
    fn avg_change_is_the_equal_weighted_mean_of_finite_changes() {
        let qs = vec![
            quote_chg("A", Some(2.0)),
            quote_chg("B", Some(-1.0)),
            quote_chg("C", Some(3.0)),
        ];
        assert_eq!(avg_change_pct(&qs), Some((2.0 - 1.0 + 3.0) / 3.0));
    }

    #[test]
    fn avg_change_excludes_quotes_without_a_usable_change() {
        // valid price but change_pct=None (price-only providers like dolarapi/frankfurter)
        let qs = vec![
            quote_chg("A", Some(4.0)),
            quote_chg("PRICEONLY", None),
            quote_chg("B", Some(2.0)),
        ];
        assert_eq!(avg_change_pct(&qs), Some(3.0));
    }

    #[test]
    fn avg_change_is_none_when_no_quote_has_a_change() {
        assert_eq!(
            avg_change_pct(&[quote_chg("A", None), quote_chg("B", None)]),
            None
        );
        assert_eq!(avg_change_pct(&[]), None);
    }

    #[test]
    fn avg_change_excludes_non_finite_values() {
        let qs = vec![quote_chg("A", Some(f64::NAN)), quote_chg("B", Some(2.0))];
        assert_eq!(avg_change_pct(&qs), Some(2.0));
    }

    #[test]
    fn the_summary_colors_a_positive_average_green_with_an_up_arrow() {
        let c = ThemeColors::default();
        let out = render_summary(0.5, "Σ {avg_arrow}{avg_change}", &IconSet::Ascii, &c);
        assert!(out.contains("+0.50%"));
        assert!(out.contains(&c.green));
        assert!(out.contains('^')); // ascii up arrow
        assert!(out.starts_with('Σ')); // literal text uncolored
    }

    #[test]
    fn the_summary_colors_a_negative_average_red() {
        let c = ThemeColors::default();
        let out = render_summary(-0.5, "{avg_arrow}{avg_change}", &IconSet::Ascii, &c);
        assert!(out.contains("-0.50%"));
        assert!(out.contains(&c.red));
        assert!(out.contains('v')); // ascii down arrow
    }

    #[test]
    fn a_flat_summary_is_left_unspanned_and_unsigned() {
        let c = ThemeColors::default();
        let out = render_summary(0.0, "{avg_change}", &IconSet::Ascii, &c);
        // No <span> wrapper, and no `+` claiming a gain. The bar summary is free-flowing
        // text (not a measured column), so `render_summary`'s trim drops the pad space.
        assert_eq!(out, "0.00%");
    }

    #[test]
    fn the_summary_prefix_appears_before_the_assets_when_configured() {
        let qs = vec![quote_chg("A", Some(2.0)), quote_chg("B", Some(4.0))];
        let mut d = disp(DisplayMode::Fixed, 3);
        d.summary_format = "AVG{avg_change}".into();
        let text = bar_text(&assets_for(&qs), &qs, &d, 0, &ThemeColors::default());
        assert!(text.starts_with("AVG"));
        assert!(text.contains("+3.00%"));
    }

    #[test]
    fn an_empty_summary_format_leaves_the_bar_unchanged() {
        // The body must be byte-identical with vs without the (empty) summary: render the
        // same input with a summary, and assert the no-summary output is its verbatim tail.
        let qs = vec![quote_chg("A", Some(2.0)), quote_chg("B", Some(4.0))];
        let assets = assets_for(&qs);
        let without = bar_text(
            &assets,
            &qs,
            &disp(DisplayMode::Fixed, 3),
            0,
            &ThemeColors::default(),
        );
        let mut d = disp(DisplayMode::Fixed, 3);
        d.summary_format = "AVG{avg_change}".into();
        let with = bar_text(&assets, &qs, &d, 0, &ThemeColors::default());
        assert!(!without.contains("AVG"));
        // The no-summary body must appear verbatim as the tail (unchanged by the feature),
        // and the summary segment + separator must be the only prefix added.
        assert!(with.starts_with("AVG"));
        assert!(with.ends_with(&format!("   {without}")));
    }

    #[test]
    fn a_whitespace_only_summary_format_does_not_prepend_a_dangling_separator() {
        let qs = vec![quote_chg("A", Some(2.0))];
        let assets = assets_for(&qs);
        let without = bar_text(
            &assets,
            &qs,
            &disp(DisplayMode::Fixed, 3),
            0,
            &ThemeColors::default(),
        );
        let mut d = disp(DisplayMode::Fixed, 3);
        d.summary_format = "   ".into(); // renders empty after trim => must fall back to body
        let with = bar_text(&assets, &qs, &d, 0, &ThemeColors::default());
        assert_eq!(with, without);
    }

    #[test]
    fn avg_change_excludes_infinities_and_is_none_when_only_infinite() {
        assert_eq!(avg_change_pct(&[quote_chg("A", Some(f64::INFINITY))]), None);
        assert_eq!(
            avg_change_pct(&[
                quote_chg("A", Some(f64::NEG_INFINITY)),
                quote_chg("B", Some(2.0))
            ]),
            Some(2.0)
        );
    }

    #[test]
    fn a_price_only_watchlist_shows_no_summary_segment() {
        let qs = vec![quote_chg("A", None)]; // valid price, no change_pct; module would be ok
        let mut d = disp(DisplayMode::Fixed, 3);
        d.summary_format = "AVG{avg_change}".into();
        let text = bar_text(&assets_for(&qs), &qs, &d, 0, &ThemeColors::default());
        assert!(!text.contains("AVG"));
    }

    #[test]
    fn an_empty_body_yields_the_summary_alone_without_a_trailing_separator() {
        // change 0.0 => flat => unspanned, so the assertion is about whitespace, not color.
        let qs = vec![quote_chg("A", Some(0.0))];
        let mut d = disp(DisplayMode::Fixed, 0); // max_on_bar=0 -> no visible assets
        d.summary_format = "AVG{avg_change}".into();
        let text = bar_text(&assets_for(&qs), &qs, &d, 0, &ThemeColors::default());
        assert_eq!(text, "AVG 0.00%"); // no leading/trailing spaces, no dangling separator
    }

    // ---- bar selection (`bar`) + layout (`bar_layout`) ----

    fn cfg_with(display: Display, qs: &[Quote]) -> Config {
        Config {
            display,
            market_hours: MarketHours::default(),
            assets: assets_for(qs),
        }
    }

    #[test]
    fn bar_candidates_empty_list_is_all_assets_in_config_order() {
        let qs = vec![quote_chg("A", Some(1.0)), quote_chg("B", Some(1.0))];
        assert_eq!(bar_candidates(&assets_for(&qs), &[]), vec![0, 1]);
    }

    #[test]
    fn bar_candidates_resolves_labels_in_list_order_skipping_unknown() {
        let qs = vec![
            quote_chg("A", Some(1.0)),
            quote_chg("B", Some(1.0)),
            quote_chg("C", Some(1.0)),
        ];
        let bar = vec!["C".to_string(), "NOPE".to_string(), "A".to_string()];
        assert_eq!(bar_candidates(&assets_for(&qs), &bar), vec![2, 0]);
    }

    #[test]
    fn bar_candidates_deduplicates_repeated_labels_keeping_first() {
        let qs = vec![quote_chg("A", Some(1.0)), quote_chg("B", Some(1.0))];
        let bar = vec!["B".to_string(), "B".to_string(), "A".to_string()];
        assert_eq!(bar_candidates(&assets_for(&qs), &bar), vec![1, 0]);
    }

    #[test]
    fn bar_candidates_all_unknown_is_empty() {
        let qs = vec![quote_chg("A", Some(1.0))];
        assert!(bar_candidates(&assets_for(&qs), &["X".to_string()]).is_empty());
    }

    #[test]
    fn bar_indices_fixed_takes_first_max_on_bar_candidates() {
        let qs = vec![
            quote_chg("A", Some(1.0)),
            quote_chg("B", Some(1.0)),
            quote_chg("C", Some(1.0)),
        ];
        let d = disp(DisplayMode::Fixed, 2);
        assert_eq!(bar_indices(&assets_for(&qs), &d, 0), vec![0, 1]);
    }

    #[test]
    fn bar_indices_fixed_honors_the_bar_subset_order() {
        let qs = vec![
            quote_chg("A", Some(1.0)),
            quote_chg("B", Some(1.0)),
            quote_chg("C", Some(1.0)),
        ];
        let mut d = disp(DisplayMode::Fixed, 3);
        d.bar = vec!["C".to_string(), "A".to_string()];
        assert_eq!(bar_indices(&assets_for(&qs), &d, 0), vec![2, 0]);
    }

    #[test]
    fn bar_indices_rotate_cycles_only_among_the_subset() {
        let qs = vec![
            quote_chg("A", Some(1.0)),
            quote_chg("B", Some(1.0)),
            quote_chg("C", Some(1.0)),
        ];
        let mut d = disp(DisplayMode::Rotate, 1);
        d.bar = vec!["A".to_string(), "C".to_string()]; // never B
        d.rotate_interval = 1;
        assert_eq!(bar_indices(&assets_for(&qs), &d, 0), vec![0]);
        assert_eq!(bar_indices(&assets_for(&qs), &d, 1), vec![2]);
        assert_eq!(bar_indices(&assets_for(&qs), &d, 2), vec![0]); // wraps within {A,C}
    }

    #[test]
    fn bar_indices_rotate_with_no_candidates_is_empty() {
        let qs = vec![quote_chg("A", Some(1.0))];
        let mut d = disp(DisplayMode::Rotate, 1);
        d.bar = vec!["NOPE".to_string()];
        assert!(bar_indices(&assets_for(&qs), &d, 0).is_empty());
    }

    #[test]
    fn apply_layout_substitutes_both_blocks() {
        assert_eq!(apply_layout("{summary}   {bar}", "BAR", "SUM"), "SUM   BAR");
        assert_eq!(apply_layout("{bar} | {summary}", "BAR", "SUM"), "BAR | SUM");
    }

    #[test]
    fn apply_layout_does_not_re_substitute_inserted_content() {
        // a bar value containing the literal {summary} must survive unchanged (single pass)
        assert_eq!(apply_layout("{bar}", "x{summary}y", "SUM"), "x{summary}y");
    }

    #[test]
    fn apply_layout_leaves_unknown_tokens_verbatim() {
        assert_eq!(apply_layout("{glyph} {bar}", "BAR", "SUM"), "{glyph} BAR");
    }

    #[test]
    fn bar_text_renders_the_subset_in_list_order_only() {
        let qs = vec![
            quote_chg("A", Some(1.0)),
            quote_chg("B", Some(1.0)),
            quote_chg("C", Some(1.0)),
        ];
        let mut d = disp(DisplayMode::Fixed, 3);
        d.bar = vec!["C".to_string(), "A".to_string()];
        let text = bar_text(&assets_for(&qs), &qs, &d, 0, &ThemeColors::default());
        let c = text.find('C').unwrap();
        let a = text.find('A').unwrap();
        assert!(c < a, "C must render before A");
        assert!(!text.contains('B'), "B is not in the bar subset");
    }

    #[test]
    fn bar_text_layout_places_summary_after_assets_when_configured() {
        // +2.00% is positive => wrapped in a color span, so "AVG" and "+2.00%" are NOT
        // contiguous; assert order + presence, not a contiguous substring.
        let qs = vec![quote_chg("A", Some(2.0))];
        let mut d = disp(DisplayMode::Fixed, 3);
        d.summary_format = "AVG{avg_change}".into();
        d.bar_layout = "{bar}   {summary}".into();
        let text = bar_text(&assets_for(&qs), &qs, &d, 0, &ThemeColors::default());
        assert!(text.contains("+2.00%"));
        assert!(
            text.find('A').unwrap() < text.find("AVG").unwrap(),
            "assets before summary"
        );
    }

    #[test]
    fn bar_text_layout_can_show_summary_only() {
        let qs = vec![quote_chg("A", Some(2.0))]; // render_one would print price "1.00"
        let mut d = disp(DisplayMode::Fixed, 3);
        d.summary_format = "AVG{avg_change}".into();
        d.bar_layout = "{summary}".into(); // {bar} omitted on purpose
        let text = bar_text(&assets_for(&qs), &qs, &d, 0, &ThemeColors::default());
        assert!(text.contains("AVG") && text.contains("+2.00%"));
        assert!(
            !text.contains("1.00"),
            "the asset block must not be rendered"
        );
    }

    #[test]
    fn module_direction_class_reflects_the_bar_subset_not_all_quotes() {
        let qs = vec![
            quote_chg("UP1", Some(2.0)),
            quote_chg("UP2", Some(3.0)),
            quote_chg("DOWN", Some(-5.0)),
        ];
        let mut d = disp(DisplayMode::Fixed, 3);
        d.bar = vec!["UP1".to_string(), "UP2".to_string()]; // excludes the Down asset
        let out = build(
            &cfg_with(d, &qs),
            &qs,
            Utc::now(),
            &ThemeColors::default(),
            ColorMode::FULL,
        );
        assert!(out.class.contains(&"up".to_string()));
        assert!(!out.class.contains(&"mixed".to_string()));
    }

    #[test]
    fn class_follows_selection_even_when_layout_hides_the_bar_text() {
        let qs = vec![quote_chg("UP1", Some(2.0)), quote_chg("DOWN", Some(-5.0))];
        let mut d = disp(DisplayMode::Fixed, 3);
        d.bar = vec!["UP1".to_string()];
        d.summary_format = "AVG{avg_change}".into();
        d.bar_layout = "{summary}".into(); // text shows only the summary
        let out = build(
            &cfg_with(d, &qs),
            &qs,
            Utc::now(),
            &ThemeColors::default(),
            ColorMode::FULL,
        );
        assert!(out.class.contains(&"up".to_string()));
    }

    #[test]
    fn closed_badge_reflects_the_bar_subset() {
        use chrono::TimeZone;
        // A CNBC stock (closeable) + a crypto (24/7). 2026-07-02 00:00 UTC = 20:00 EDT => US closed.
        let assets = vec![
            Asset {
                label: "AAPL".to_string(),
                source: AssetSource::Cnbc {
                    symbol: "AAPL".to_string(),
                },
            },
            Asset {
                label: "BTC".to_string(),
                source: AssetSource::Coingecko {
                    id: "bitcoin".to_string(),
                    quote: "usd".to_string(),
                },
            },
        ];
        let qs = vec![
            quote("AAPL", Some(1.0), Some(Direction::Up), QuoteState::Fresh),
            quote("BTC", Some(1.0), Some(Direction::Up), QuoteState::Fresh),
        ];
        let now = Utc.with_ymd_and_hms(2026, 7, 2, 0, 0, 0).unwrap();
        let mk = |bar: Vec<String>| {
            let mut d = disp(DisplayMode::Fixed, 3);
            d.bar = bar;
            Config {
                display: d,
                market_hours: MarketHours::default(),
                assets: assets.clone(),
            }
        };
        // Bar = only the (closed) stock => the whole bar is closed.
        let only_stock = build(
            &mk(vec!["AAPL".to_string()]),
            &qs,
            now,
            &ThemeColors::default(),
            ColorMode::FULL,
        );
        assert!(only_stock.class.contains(&"closed".to_string()));
        // No bar subset => BTC (always open) is also on the bar => not all closed.
        let all = build(
            &mk(vec![]),
            &qs,
            now,
            &ThemeColors::default(),
            ColorMode::FULL,
        );
        assert!(!all.class.contains(&"closed".to_string()));
    }

    #[test]
    fn apply_layout_handles_unclosed_and_stray_braces() {
        assert_eq!(apply_layout("{bar", "B", "S"), "{bar"); // unclosed token kept verbatim
        assert_eq!(apply_layout("foo } {bar}", "B", "S"), "foo } B"); // stray close brace
        assert_eq!(apply_layout("{{bar}}", "B", "S"), "{{bar}}"); // doubled braces => unknown token
        assert_eq!(apply_layout("{bar}{summary}", "B", "S"), "BS"); // adjacent tokens
    }

    #[test]
    fn the_new_cnbc_classes_map_to_their_tooltip_groups() {
        assert_eq!(
            group_of(&AssetSource::Commodity {
                symbol: "gold".into()
            }),
            TooltipGroup::Commodities
        );
        assert_eq!(
            group_of(&AssetSource::Index {
                symbol: "vix".into()
            }),
            TooltipGroup::Indices
        );
        assert_eq!(
            group_of(&AssetSource::Rate {
                symbol: "us10y".into()
            }),
            TooltipGroup::Rates
        );
    }

    #[test]
    fn a_rate_value_renders_as_a_percent_but_a_commodity_as_a_price() {
        assert_eq!(fmt_value(Some(4.532), TooltipGroup::Rates), "4.53%");
        assert_eq!(
            fmt_value(Some(4353.9), TooltipGroup::Commodities),
            "4,353.90"
        );
    }

    #[test]
    fn the_intraday_range_is_appended_only_when_enabled() {
        let mut q = quote(
            "GOLD",
            Some(4400.0),
            Some(Direction::Down),
            QuoteState::Fresh,
        );
        q.day_low = Some(4336.6);
        q.day_high = Some(4508.7);
        let now = Utc::now();
        let colors = ThemeColors::default();
        let w = ColWidths {
            label: 6,
            price: 10,
            change: 8,
        };
        let off = render_row(&q, TooltipGroup::Commodities, false, &w, now, &colors);
        let on = render_row(&q, TooltipGroup::Commodities, true, &w, now, &colors);
        assert!(!off.contains("4,336.60"));
        assert!(on.contains("4,336.60-4,508.70"));
    }

    #[test]
    fn a_rate_range_renders_as_percents() {
        let mut q = quote("US10Y", Some(4.53), Some(Direction::Up), QuoteState::Fresh);
        q.day_low = Some(4.457);
        q.day_high = Some(4.554);
        let w = ColWidths {
            label: 6,
            price: 8,
            change: 8,
        };
        let on = render_row(
            &q,
            TooltipGroup::Rates,
            true,
            &w,
            Utc::now(),
            &ThemeColors::default(),
        );
        assert!(on.contains("4.46%-4.55%"));
    }

    /// Generic (non-rate) assets aligned 1:1 with quotes, for bar-rendering tests.
    fn assets_for(qs: &[Quote]) -> Vec<Asset> {
        qs.iter()
            .map(|q| Asset {
                label: q.label.clone(),
                source: AssetSource::Coingecko {
                    id: "x".into(),
                    quote: "usd".into(),
                },
            })
            .collect()
    }

    #[test]
    fn fixed_mode_shows_at_most_max_on_bar_assets() {
        let qs = vec![
            quote("A", Some(1.0), Some(Direction::Up), QuoteState::Fresh),
            quote("B", Some(2.0), Some(Direction::Up), QuoteState::Fresh),
            quote("C", Some(3.0), Some(Direction::Up), QuoteState::Fresh),
        ];
        let text = bar_text(
            &assets_for(&qs),
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
        let a = assets_for(&qs);
        assert!(bar_text(&a, &qs, &d, 0, &ThemeColors::default()).contains('A'));
        assert!(bar_text(&a, &qs, &d, 5, &ThemeColors::default()).contains('B'));
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
            &assets_for(&qs),
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
                source: AssetSource::Cnbc {
                    symbol: "AAPL".into(),
                },
            },
        ];
        let qs = vec![
            quote("BTC", Some(68000.0), Some(Direction::Up), QuoteState::Fresh),
            Quote {
                source: ProviderKind::Cnbc,
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

    // ---- shared column-packing plan (pack_columns) ----

    fn seg(group: usize, start: usize, len: usize, continued: bool) -> PackedSegment {
        PackedSegment {
            group,
            start,
            len,
            continued,
        }
    }

    #[test]
    fn a_column_never_ends_on_a_header() {
        // Two 1-row groups with a 3-line budget: naive chunking would strand the second
        // group's header at the bottom of column 0; it must move to column 1's top.
        let cols = pack_columns(&[1, 1], 3);
        assert_eq!(
            cols,
            vec![vec![seg(0, 0, 1, false)], vec![seg(1, 0, 1, false)]]
        );
        // No zero-length (header-only) segment may survive anywhere.
        assert!(cols.iter().flatten().all(|s| s.len > 0));
    }

    #[test]
    fn a_section_split_across_columns_gets_a_continuation_segment() {
        // One 3-row group with a 2-line budget: [H, R0] | [R1, R2] -> the second column
        // starts mid-section and must open a continued segment covering rows 1..3.
        let cols = pack_columns(&[3], 2);
        assert_eq!(
            cols,
            vec![vec![seg(0, 0, 1, false)], vec![seg(0, 1, 2, true)]]
        );
    }

    #[test]
    fn a_single_line_per_column_does_not_produce_empty_columns() {
        // Budget 1: [H] | [R] -> the orphan move empties column 0, which must be dropped.
        let cols = pack_columns(&[1], 1);
        assert_eq!(cols, vec![vec![seg(0, 0, 1, false)]]);
    }

    #[test]
    fn zero_budget_or_small_content_packs_a_single_column() {
        // rows_per_column = 0 keeps the waybar single-column semantics.
        assert_eq!(
            pack_columns(&[2, 1], 0),
            vec![vec![seg(0, 0, 2, false), seg(1, 0, 1, false)]]
        );
        // Fewer total lines (5) than the budget (10) also stays single-column.
        assert_eq!(pack_columns(&[2, 1], 10).len(), 1);
        // No groups -> no columns.
        assert!(pack_columns(&[], 4).is_empty());
        // Empty groups are skipped without shifting the indices of later groups.
        assert_eq!(pack_columns(&[0, 2], 0), vec![vec![seg(1, 0, 2, false)]]);
    }

    #[test]
    fn band_size_caps_columns_only_when_exceeded() {
        assert_eq!(band_size(5, 3), 3); // banding kicks in
        assert_eq!(band_size(3, 3), 3); // exactly at the cap: one band
        assert_eq!(band_size(2, 0), 2); // unlimited
        assert_eq!(band_size(0, 0), 1); // degenerate floor
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
        d.frame = true; // equal-width is a framed-box property
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
        // Without a box the lines no longer share one width; what has to hold
        // is that the DATA rows still line up as columns.
        let row_widths: Vec<usize> = banded
            .lines()
            .filter(|l| l.contains("SYM"))
            .map(waybar::visible_len)
            .collect();
        assert!(
            row_widths.windows(2).all(|w| w[0] == w[1]),
            "data rows lost their column alignment: {row_widths:?}"
        );
    }

    #[test]
    fn tooltip_data_rows_have_equal_visible_width() {
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
        // The DATA rows are the ones that must line up; the title and the
        // footer are shorter on purpose.
        let row_widths: Vec<usize> = tip
            .lines()
            .filter(|l| l.contains("SYM"))
            .map(waybar::visible_len)
            .collect();
        assert!(
            row_widths.windows(2).all(|w| w[0] == w[1]),
            "data rows lost their column alignment: {row_widths:?}"
        );
    }

    #[test]
    fn the_tooltip_is_borderless_pinned_and_column_aligned() {
        let assets: Vec<Asset> = (0..3)
            .map(|i| Asset {
                label: format!("SYM{i}"),
                source: AssetSource::Cnbc {
                    symbol: format!("S{i}"),
                },
            })
            .collect();
        let qs: Vec<Quote> = (0..3)
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
        let d = disp(DisplayMode::Fixed, 3);
        let tip = build_tooltip(
            &assets,
            &qs,
            &d,
            &MarketHours::default(),
            &ThemeColors::default(),
            Utc::now(),
        );
        // No box, but the font IS pinned: it is what keeps the rules the same
        // width as the text and the columns lined up.
        assert!(!tip.contains('╭') && !tip.contains('╰'));
        assert!(tip.contains("font_family="));
        // The header keeps its label AND its Nerd group glyph: with the font
        // pinned, the glyph's advance is one cell and nothing below shifts.
        let g = group_of(&assets[0].source);
        assert!(tip.contains(g.label()));
        assert!(tip.contains(g.glyph()));
        // Data rows stay column-aligned even without a frame (uniform visible width).
        let row_widths: Vec<usize> = tip
            .lines()
            .filter(|l| l.contains("SYM"))
            .map(waybar::visible_len)
            .collect();
        assert_eq!(row_widths.len(), 3);
        assert!(
            row_widths.windows(2).all(|w| w[0] == w[1]),
            "plain data rows stay aligned"
        );
    }

    // ---- Flat change formatting --------------------------------------------------------

    #[test]
    fn a_flat_change_renders_unsigned_so_it_never_reads_as_a_gain() {
        assert_eq!(fmt_change(Some(0.0), Some(Direction::Flat)), " 0.00%");
        // Negative zero must not leak a minus sign either.
        assert_eq!(fmt_change(Some(-0.0), Some(Direction::Flat)), " 0.00%");
    }

    #[test]
    fn a_move_that_rounds_to_zero_keeps_its_real_sign() {
        // The direction is the model's, not the rounded text's: a tiny-but-nonzero move
        // still moved, so it keeps the sign that says which way.
        assert_eq!(fmt_change(Some(0.004), Some(Direction::Up)), "+0.00%");
        assert_eq!(fmt_change(Some(-0.004), Some(Direction::Down)), "-0.00%");
    }

    #[test]
    fn a_flat_change_measures_the_same_width_as_a_signed_one() {
        let flat = fmt_change(Some(0.0), Some(Direction::Flat));
        let up = fmt_change(Some(1.0), Some(Direction::Up));
        assert_eq!(waybar::visible_len(&flat), waybar::visible_len(&up));
    }

    #[test]
    fn a_flat_row_stays_column_aligned_with_a_signed_one() {
        let qs = vec![quote_chg("FLAT", Some(0.0)), quote_chg("RISE", Some(1.50))];
        let cfg = cfg_with(disp(DisplayMode::Fixed, 5), &qs);
        let out = build(
            &cfg,
            &qs,
            Utc::now(),
            &ThemeColors::default(),
            ColorMode::NONE,
        );

        let rows: Vec<&str> = out
            .tooltip
            .lines()
            .filter(|l| l.contains("FLAT") || l.contains("RISE"))
            .collect();
        assert_eq!(rows.len(), 2);
        assert!(out.tooltip.contains(" 0.00%"), "flat row is unsigned");
        assert!(out.tooltip.contains("+1.50%"), "the mover keeps its sign");
        assert_eq!(
            waybar::visible_len(rows[0]),
            waybar::visible_len(rows[1]),
            "the pad space keeps the change column aligned"
        );
    }

    // ---- Monochrome (`--no-color`) -----------------------------------------------------

    /// Everything the reader actually sees: the markup with all tags removed.
    fn strip_tags(s: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        for ch in s.chars() {
            match ch {
                '<' => in_tag = true,
                '>' if in_tag => in_tag = false,
                _ if !in_tag => out.push(ch),
                _ => {}
            }
        }
        out
    }

    /// Both directions plus the framed tooltip, so borders, header, rows and footer are
    /// all exercised.
    fn two_way_market() -> (Config, Vec<Quote>) {
        let qs = vec![quote_chg("UP", Some(1.50)), quote_chg("DOWN", Some(-2.25))];
        let mut d = disp(DisplayMode::Fixed, 5);
        d.bar_format = "{label} {price} {arrow}{change_pct}".into();
        d.frame = true;
        (cfg_with(d, &qs), qs)
    }

    fn built(mode: ColorMode) -> WaybarOutput {
        let (cfg, qs) = two_way_market();
        build(&cfg, &qs, Utc::now(), &ThemeColors::default(), mode)
    }

    /// Both surfaces from ONE market and ONE instant, so nothing but the palette differs.
    fn built_pair() -> (WaybarOutput, WaybarOutput) {
        let (cfg, qs) = two_way_market();
        let now = Utc::now();
        let theme = ThemeColors::default();
        (
            build(&cfg, &qs, now, &theme, ColorMode::FULL),
            build(&cfg, &qs, now, &theme, ColorMode::NONE),
        )
    }

    #[test]
    fn both_surfaces_carry_color_by_default() {
        assert_eq!(ColorMode::default(), ColorMode::FULL);
        let out = built(ColorMode::FULL);
        assert!(out.text.contains("foreground="));
        assert!(out.tooltip.contains("foreground="));
    }

    #[test]
    fn monochrome_leaves_no_color_markup_on_either_surface() {
        let out = built(ColorMode::NONE);

        assert!(!out.text.contains("foreground="), "bar: {}", out.text);
        assert!(!out.text.contains('#'), "no inline hex on the bar");
        assert!(!out.tooltip.contains("foreground="));
        assert!(!out.tooltip.contains('#'), "no inline hex in the tooltip");
    }

    #[test]
    fn a_monochrome_bar_keeps_the_tooltip_colored() {
        let out = built(ColorMode::PLAIN_BAR);

        assert!(!out.text.contains("foreground="));
        assert!(out.tooltip.contains("foreground="));
    }

    #[test]
    fn a_monochrome_tooltip_keeps_the_bar_colored() {
        let out = built(ColorMode::PLAIN_TOOLTIP);

        assert!(out.text.contains("foreground="));
        assert!(!out.tooltip.contains("foreground="));
    }

    #[test]
    fn monochrome_keeps_the_module_class_so_it_can_be_styled_from_css() {
        let (colored, plain) = built_pair();

        assert_eq!(colored.class, plain.class);
        assert_eq!(colored.alt, plain.alt);
        assert!(plain
            .class
            .iter()
            .any(|c| c == "mixed" || c == "up" || c == "down"));
    }

    #[test]
    fn monochrome_keeps_the_structure_glyphs_and_the_direction_sign() {
        let plain = built(ColorMode::NONE);

        // Box drawing (the rules — the ─ that separate the sections), the bold
        // weight and the ascii arrows all survive. The │ column divider only
        // appears once the watchlist needs more than one column.
        assert!(plain.tooltip.contains('─'));
        assert!(plain.tooltip.contains("font_weight='bold'"));
        assert!(plain.text.contains('^') && plain.text.contains('v'));
        // Direction stays readable in the tooltip through the signed change column,
        // which is the only direction carrier there (the column has no glyph by design).
        assert!(plain.tooltip.contains("+1.50%"));
        assert!(plain.tooltip.contains("-2.25%"));
    }

    #[test]
    fn column_widths_do_not_shift_when_the_spans_are_absent() {
        let (colored, plain) = built_pair();

        // Same visible text, character for character: padding was measured with
        // `visible_len`, which never counted the tags in the first place.
        assert_eq!(strip_tags(&colored.tooltip), strip_tags(&plain.tooltip));
        assert_eq!(strip_tags(&colored.text), strip_tags(&plain.text));

        let widths = |s: &str| -> Vec<usize> { s.lines().map(waybar::visible_len).collect() };
        assert_eq!(widths(&colored.tooltip), widths(&plain.tooltip));
    }
}
