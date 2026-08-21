//! Structured JSON output (`--output json`): raw data for machine consumers, notably the
//! Omarchy shell (Quickshell) plugin in `omarchy/`. No Pango markup, no colors, no
//! pre-rendered strings — numbers stay numbers and the frontend decides presentation.
//!
//! Grouping, ordering, the curated bar subset, and the summary average all reuse the same
//! functions the waybar renderer uses (`render::group_of`, `render::GROUP_ORDER`,
//! `render::bar_indices`, `render::avg_change_pct`), so the two outputs can never disagree.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::platform::config::Config;
use crate::platform::market::{self, Gate};
use crate::platform::model::{Asset, AssetSource, Direction, Quote, QuoteState};
use crate::platform::render::{self, TooltipGroup, GROUP_ORDER};
use crate::platform::theme::ThemeColors;

pub const SCHEMA_VERSION: u32 = 1;

/// Uniform error shape: `null | {message, code?}` (shared across the sibling *bar schemas).
#[derive(Serialize, Clone)]
pub struct ErrorInfo {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ErrorInfo {
    fn new(message: &str) -> Self {
        ErrorInfo {
            message: message.to_string(),
            code: None,
        }
    }
}

/// The resolved colour palette, published so every frontend paints the same number the
/// same colour. `up`/`down`/`flat` ARE the direction semantics — the exact colours the
/// core's own Waybar renderer uses for a quote's `direction` (see
/// `render::direction_color`) — so a frontend never re-derives them from an accent and
/// cannot drift when the theme chain changes.
///
/// Never monochrome: `--no-color` is a choice about the Pango surfaces and must not alter
/// this document. A frontend that renders monochrome simply does not apply the palette.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct Palette {
    pub up: String,
    pub down: String,
    pub flat: String,
    pub text: String,
    pub dim: String,
    pub accent: String,
    pub error: String,
}

impl Palette {
    pub fn from_theme(c: &ThemeColors) -> Self {
        Self {
            up: render::direction_color(Some(Direction::Up), c).to_string(),
            down: render::direction_color(Some(Direction::Down), c).to_string(),
            flat: render::direction_color(Some(Direction::Flat), c).to_string(),
            text: c.text.clone(),
            dim: c.dim.clone(),
            accent: c.accent.clone(),
            error: c.error.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct DataOutput {
    pub schema_version: u32,
    /// Overall lifecycle: `ok` | `partial` (some quotes missing) | `stale` | `error`.
    /// `partial` wins over `stale` when both apply (missing data is the stronger signal).
    pub state: &'static str,
    /// Top-level failure (config error, internal error). Rows carry their own `error`.
    pub error: Option<ErrorInfo>,
    /// Run timestamp, ISO-8601.
    pub fetched_at: String,
    /// Curated bar subset: the SAME row objects as in `groups`, selected and ordered
    /// exactly like the waybar text (honors `display.bar`, `max_on_bar`, rotate mode).
    /// Full row objects — not labels — so duplicate labels stay unambiguous.
    pub bar: Vec<Row>,
    /// Equal-weighted mean daily change % across the whole watchlist (null if unusable).
    pub avg_change_pct: Option<f64>,
    /// Whether the TOML config has a non-empty `summary_format` (frontends may use it
    /// as the default for showing the summary).
    pub summary_configured: bool,
    /// Multi-column layout hints from the config, so frontends can wrap the watchlist
    /// exactly like the waybar tooltip does.
    pub layout: Layout,
    /// Rows grouped by asset class, in the tooltip's fixed section order.
    pub groups: Vec<Group>,
    /// Resolved colours + direction semantics. See `Palette`.
    pub palette: Palette,
}

/// The effective column-wrapping values (config merged with any `--rows-per-column` /
/// `--max-columns` CLI overrides) plus the packed plan itself. The packing — headers never
/// orphaned, `(cont.)` continuations, banding — is decided HERE by the same
/// `render::pack_columns` the waybar tooltip renders; frontends only draw it.
#[derive(Serialize)]
pub struct Layout {
    pub rows_per_column: usize,
    pub max_columns: usize,
    /// bands (stacked vertically) -> columns (side by side) -> segments. Each segment is
    /// `{group, start, len, continued}`: `group` indexes `groups`, `start`/`len` select
    /// the run of that group's `rows`, `continued` marks a section spilling over from
    /// the previous column.
    pub bands: Vec<Vec<Vec<render::PackedSegment>>>,
}

#[derive(Serialize)]
pub struct Group {
    /// Stable machine id, snake_case.
    pub id: &'static str,
    /// Human section title (same as the waybar tooltip header).
    pub label: &'static str,
    /// Nerd-font class glyph (same one the waybar tooltip header uses).
    pub glyph: &'static str,
    /// Distinct providers feeding this group, config order (tooltip's dim "(source)" suffix).
    pub sources: Vec<&'static str>,
    /// True when every market feeding this group is currently closed.
    pub closed: bool,
    pub rows: Vec<Row>,
}

#[derive(Serialize, Clone)]
pub struct Row {
    pub label: String,
    /// Provider-level identifier (coingecko id, dolarapi casa, CNBC/BYMA symbol, FX pair).
    pub symbol: String,
    /// Nerd-font class glyph, the same one the group header carries. Published per row so a
    /// frontend drawing a FLAT list (the compact bar strip, where rows are not nested under
    /// their group) can mark each asset with its class without re-deriving which group the
    /// asset belongs to — that classification stays here, in one place.
    pub glyph: &'static str,
    pub price: Option<f64>,
    /// Quote currency (e.g. "usd", "ars"); null when unknown (missing quote).
    pub quote: Option<String>,
    pub change_pct: Option<f64>,
    /// "up" | "down" | "flat"; null when the provider has no daily change.
    pub direction: Option<&'static str>,
    /// Intraday range — present only when the config enables `tooltip_range` AND the
    /// provider supplies it. Same unit as `price`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_low: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_high: Option<f64>,
    /// "percent" for rates/yields (render `4.53` as "4.53%"), "currency" otherwise.
    pub unit: &'static str,
    /// "open" | "closed" — market-hours gate for this asset right now.
    pub market: &'static str,
    /// Quote lifecycle: "fresh" | "stale" | "missing" | "error".
    pub state: &'static str,
    /// Row-level problem, when the quote is unusable: `null | {message, code?}`.
    pub error: Option<ErrorInfo>,
}

fn group_id(g: TooltipGroup) -> &'static str {
    match g {
        TooltipGroup::Crypto => "crypto",
        TooltipGroup::FiatArs => "fiat_ars",
        TooltipGroup::AccionesAr => "acciones_ar",
        TooltipGroup::Bonos => "bonos_ar",
        TooltipGroup::Cedears => "cedears",
        TooltipGroup::On => "corp_bonds",
        TooltipGroup::Stocks => "stocks",
        TooltipGroup::Indices => "indices",
        TooltipGroup::Commodities => "commodities",
        TooltipGroup::Rates => "rates",
        TooltipGroup::Forex => "forex",
    }
}

fn symbol_of(src: &AssetSource) -> String {
    match src {
        AssetSource::Coingecko { id, .. } => id.clone(),
        AssetSource::Dolarapi { casa, .. } => casa.clone(),
        AssetSource::Finnhub { symbol }
        | AssetSource::Cnbc { symbol }
        | AssetSource::Commodity { symbol }
        | AssetSource::Index { symbol }
        | AssetSource::Rate { symbol }
        | AssetSource::Data912 { symbol, .. } => symbol.clone(),
        AssetSource::Frankfurter { base, quote } => format!("{base}/{quote}"),
    }
}

fn direction_str(d: Direction) -> &'static str {
    match d {
        Direction::Up => "up",
        Direction::Down => "down",
        Direction::Flat => "flat",
    }
}

fn state_str(s: QuoteState) -> &'static str {
    match s {
        QuoteState::Fresh => "fresh",
        QuoteState::Stale => "stale",
        QuoteState::Missing => "missing",
        QuoteState::Error => "error",
    }
}

/// Overall lifecycle from all quotes — same predicate set as `render::module_class`, folded
/// into a single value (JSON consumers want one state, not a class list).
fn overall_state(quotes: &[Quote]) -> &'static str {
    if quotes.is_empty() || quotes.iter().all(|q| q.price.is_none()) {
        "error"
    } else if quotes.iter().any(|q| q.price.is_none()) {
        "partial"
    } else if quotes.iter().any(|q| q.state == QuoteState::Stale) {
        "stale"
    } else {
        "ok"
    }
}

fn row(
    asset: &Asset,
    q: &Quote,
    group: TooltipGroup,
    show_range: bool,
    market: &'static str,
) -> Row {
    let error = match q.state {
        QuoteState::Missing => Some(ErrorInfo::new("no data")),
        QuoteState::Error => Some(ErrorInfo::new("fetch failed")),
        _ => None,
    };
    Row {
        label: q.label.clone(),
        symbol: symbol_of(&asset.source),
        glyph: group.glyph(),
        price: q.price,
        quote: (!q.quote.is_empty()).then(|| q.quote.clone()),
        change_pct: q.change_pct.filter(|v| v.is_finite()),
        direction: q.direction.map(direction_str),
        day_low: show_range.then_some(q.day_low).flatten(),
        day_high: show_range.then_some(q.day_high).flatten(),
        unit: if group == TooltipGroup::Rates {
            "percent"
        } else {
            "currency"
        },
        market,
        state: state_str(q.state),
        error,
    }
}

pub fn build(
    cfg: &Config,
    quotes: &[Quote],
    now: DateTime<Utc>,
    colors: &ThemeColors,
) -> DataOutput {
    debug_assert_eq!(cfg.assets.len(), quotes.len());
    let epoch = now.timestamp().max(0) as u64;

    // One serialized row per asset (config order); both `groups` and `bar` draw from this,
    // so the bar subset carries the exact same row objects as the grouped table.
    let per_asset: Vec<(TooltipGroup, &'static str, Row)> = cfg
        .assets
        .iter()
        .zip(quotes)
        .map(|(a, q)| {
            let g = render::group_of(&a.source);
            let market = match market::gate(&a.source, now, &cfg.market_hours) {
                Gate::Open => "open",
                Gate::Closed { .. } => "closed",
            };
            let provider = a.source.kind().as_str();
            (g, provider, row(a, q, g, cfg.display.tooltip_range, market))
        })
        .collect();

    let bar: Vec<Row> = render::bar_indices(&cfg.assets, &cfg.display, epoch)
        .into_iter()
        .filter_map(|i| per_asset.get(i).map(|(_, _, r)| r.clone()))
        .collect();

    let mut groups: Vec<Group> = Vec::new();
    for g in GROUP_ORDER {
        let members: Vec<&(TooltipGroup, &'static str, Row)> =
            per_asset.iter().filter(|(gg, _, _)| *gg == g).collect();
        if members.is_empty() {
            continue;
        }
        let mut sources: Vec<&'static str> = Vec::new();
        for (_, provider, _) in &members {
            if !sources.contains(provider) {
                sources.push(provider);
            }
        }
        let rows: Vec<Row> = members.iter().map(|(_, _, r)| r.clone()).collect();
        let closed = rows.iter().all(|r| r.market == "closed");
        groups.push(Group {
            id: group_id(g),
            label: g.label(),
            glyph: g.glyph(),
            sources,
            closed,
            rows,
        });
    }

    // The packed layout, from the ONE shared packing logic (also renders the tooltip).
    let sizes: Vec<usize> = groups.iter().map(|g| g.rows.len()).collect();
    let columns = render::pack_columns(&sizes, cfg.display.tooltip_rows_per_column);
    let bands: Vec<Vec<Vec<render::PackedSegment>>> = if columns.is_empty() {
        Vec::new()
    } else {
        let size = render::band_size(columns.len(), cfg.display.tooltip_max_columns);
        columns.chunks(size).map(|band| band.to_vec()).collect()
    };

    DataOutput {
        schema_version: SCHEMA_VERSION,
        state: overall_state(quotes),
        error: None,
        fetched_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        bar,
        avg_change_pct: render::avg_change_pct(quotes),
        summary_configured: !cfg.display.summary_format.trim().is_empty(),
        layout: Layout {
            rows_per_column: cfg.display.tooltip_rows_per_column,
            max_columns: cfg.display.tooltip_max_columns,
            bands,
        },
        groups,
        palette: Palette::from_theme(colors),
    }
}

/// Structured-mode counterpart of `waybar::error_output`: still exit 0 with valid JSON.
pub fn error_output(msg: &str, now: DateTime<Utc>, colors: &ThemeColors) -> DataOutput {
    DataOutput {
        schema_version: SCHEMA_VERSION,
        state: "error",
        error: Some(ErrorInfo::new(msg)),
        fetched_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        bar: Vec::new(),
        avg_change_pct: None,
        summary_configured: false,
        layout: Layout {
            rows_per_column: 0,
            max_columns: 0,
            bands: Vec::new(),
        },
        groups: Vec::new(),
        palette: Palette::from_theme(colors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::config::{Display, MarketHours};
    use crate::platform::model::ProviderKind;
    use chrono::TimeZone;

    fn asset(label: &str, source: AssetSource) -> Asset {
        Asset {
            label: label.into(),
            source,
        }
    }

    fn quote(label: &str, price: Option<f64>, change: Option<f64>, state: QuoteState) -> Quote {
        Quote {
            label: label.into(),
            base: "b".into(),
            quote: "usd".into(),
            native_quote: "usd".into(),
            price,
            change_pct: change,
            change_abs: None,
            direction: change.map(Direction::from_change),
            day_high: None,
            day_low: None,
            source: ProviderKind::CoinGecko,
            as_of: None,
            fetched_at: Utc::now(),
            state,
        }
    }

    fn cfg(assets: Vec<Asset>) -> Config {
        Config {
            display: Display::default(),
            market_hours: MarketHours::default(),
            assets,
        }
    }

    fn btc_asset() -> Asset {
        asset(
            "BTC",
            AssetSource::Coingecko {
                id: "bitcoin".into(),
                quote: "usd".into(),
            },
        )
    }

    #[test]
    fn groups_follow_the_tooltip_order_with_crypto_before_stocks() {
        let c = cfg(vec![
            asset(
                "AAPL",
                AssetSource::Cnbc {
                    symbol: "AAPL".into(),
                },
            ),
            btc_asset(),
        ]);
        let qs = vec![
            quote("AAPL", Some(201.5), Some(-0.3), QuoteState::Fresh),
            quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh),
        ];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        let ids: Vec<&str> = out.groups.iter().map(|g| g.id).collect();
        assert_eq!(ids, vec!["crypto", "stocks"]);
        // Header chrome for frontends: the tooltip's class glyph + provider list.
        assert_eq!(out.groups[0].glyph, "\u{f15a}");
        assert_eq!(out.groups[0].sources, vec!["coingecko"]);
        assert_eq!(out.groups[1].sources, vec!["cnbc"]);
        assert_eq!(out.groups[0].rows[0].symbol, "bitcoin");
        assert_eq!(out.groups[0].rows[0].price, Some(68000.0));
        assert_eq!(out.groups[0].rows[0].direction, Some("up"));
        assert_eq!(out.state, "ok");
    }

    #[test]
    fn the_published_palette_is_exactly_what_the_waybar_renderer_paints_with() {
        // The whole point of publishing it: a frontend that applies `palette.up` lands on
        // the same colour `render_row` gives an up quote. If these ever diverge, the two
        // frontends disagree about the same number — which is the bug this fixes.
        let theme = ThemeColors::default();
        let p = Palette::from_theme(&theme);

        assert_eq!(p.up, render::direction_color(Some(Direction::Up), &theme));
        assert_eq!(
            p.down,
            render::direction_color(Some(Direction::Down), &theme)
        );
        assert_eq!(
            p.flat,
            render::direction_color(Some(Direction::Flat), &theme)
        );
        assert_eq!(p.up, theme.green);
        assert_eq!(p.down, theme.red);
        assert_eq!(p.flat, theme.text);
        assert_eq!(p.accent, theme.accent);
        assert_eq!(p.dim, theme.dim);
        assert_eq!(p.error, theme.error);
    }

    #[test]
    fn the_palette_follows_the_resolved_theme_not_the_builtin_defaults() {
        let theme = ThemeColors {
            green: "#9ece6a".into(),
            red: "#f7768e".into(),
            accent: "#7aa2f7".into(),
            ..Default::default()
        };

        let p = Palette::from_theme(&theme);

        assert_eq!(p.up, "#9ece6a");
        assert_eq!(p.down, "#f7768e");
        assert_eq!(p.accent, "#7aa2f7");
    }

    #[test]
    fn even_an_error_document_publishes_the_palette() {
        // The panel needs colours to paint the error card itself.
        let out = error_output("boom", Utc::now(), &ThemeColors::default());

        assert_eq!(out.state, "error");
        assert_eq!(out.palette, Palette::from_theme(&ThemeColors::default()));
    }

    #[test]
    fn a_zero_change_is_carried_as_flat_for_frontends_to_render_unsigned() {
        // The QML panel mirrors the CLI's unsigned flat formatting off THIS field, not off
        // the rounded number, so the contract is worth pinning down.
        let c = cfg(vec![btc_asset()]);
        let qs = vec![quote("BTC", Some(68000.0), Some(0.0), QuoteState::Fresh)];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());

        assert_eq!(out.groups[0].rows[0].direction, Some("flat"));
        assert_eq!(out.groups[0].rows[0].change_pct, Some(0.0));
    }

    #[test]
    fn the_bar_list_honors_the_curated_subset_and_order() {
        let mut c = cfg(vec![
            btc_asset(),
            asset(
                "Blue",
                AssetSource::Dolarapi {
                    casa: "blue".into(),
                    side: Default::default(),
                },
            ),
        ]);
        c.display.bar = vec!["Blue".into(), "BTC".into()];
        let qs = vec![
            quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh),
            quote("Blue", Some(1200.0), None, QuoteState::Fresh),
        ];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        let labels: Vec<&str> = out.bar.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["Blue", "BTC"]);
        // Full row objects, not just labels: the dolarapi row keeps its own symbol/price.
        assert_eq!(out.bar[0].symbol, "blue");
        assert_eq!(out.bar[0].price, Some(1200.0));
    }

    #[test]
    fn duplicate_labels_stay_unambiguous_in_the_bar_subset() {
        // Two assets sharing the label "BTC" (usd and ars quotes). A label list could not
        // distinguish them; raw row objects must carry each asset's own price/symbol.
        let c = cfg(vec![
            btc_asset(),
            asset(
                "BTC",
                AssetSource::Coingecko {
                    id: "bitcoin".into(),
                    quote: "ars".into(),
                },
            ),
        ]);
        let mut ars = quote("BTC", Some(90_000_000.0), Some(1.2), QuoteState::Fresh);
        ars.quote = "ars".into();
        let qs = vec![
            quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh),
            ars,
        ];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        assert_eq!(out.bar.len(), 2);
        assert_eq!(out.bar[0].price, Some(68000.0));
        assert_eq!(out.bar[1].price, Some(90_000_000.0));
        assert_eq!(out.bar[0].quote.as_deref(), Some("usd"));
        assert_eq!(out.bar[1].quote.as_deref(), Some("ars"));
    }

    #[test]
    fn a_rate_row_is_marked_percent_and_others_currency() {
        let c = cfg(vec![
            asset(
                "US 10Y",
                AssetSource::Rate {
                    symbol: "us10y".into(),
                },
            ),
            btc_asset(),
        ]);
        let qs = vec![
            quote("US 10Y", Some(4.53), Some(0.5), QuoteState::Fresh),
            quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh),
        ];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        let rates = out.groups.iter().find(|g| g.id == "rates").unwrap();
        let crypto = out.groups.iter().find(|g| g.id == "crypto").unwrap();
        assert_eq!(rates.rows[0].unit, "percent");
        assert_eq!(crypto.rows[0].unit, "currency");
    }

    #[test]
    fn the_day_range_appears_only_when_the_config_enables_it() {
        let mut c = cfg(vec![btc_asset()]);
        let mut q = quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh);
        q.day_low = Some(67000.0);
        q.day_high = Some(69000.0);
        let off = build(
            &c,
            std::slice::from_ref(&q),
            Utc::now(),
            &ThemeColors::default(),
        );
        assert_eq!(off.groups[0].rows[0].day_low, None);
        c.display.tooltip_range = true;
        let on = build(
            &c,
            std::slice::from_ref(&q),
            Utc::now(),
            &ThemeColors::default(),
        );
        assert_eq!(on.groups[0].rows[0].day_low, Some(67000.0));
        assert_eq!(on.groups[0].rows[0].day_high, Some(69000.0));
    }

    #[test]
    fn the_overall_state_is_partial_when_some_quotes_are_missing() {
        let c = cfg(vec![btc_asset(), btc_asset()]);
        let qs = vec![
            quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh),
            quote("BTC", None, None, QuoteState::Missing),
        ];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        assert_eq!(out.state, "partial");
        let row = &out.groups[0].rows[1];
        assert_eq!(row.state, "missing");
        assert_eq!(
            row.error.as_ref().map(|e| e.message.as_str()),
            Some("no data")
        );
        assert_eq!(row.price, None);
    }

    #[test]
    fn a_closed_market_placeholder_row_still_reports_its_currency() {
        use crate::platform::model::Panel;
        let byma = asset(
            "ALUA",
            AssetSource::Data912 {
                panel: Panel::Acciones,
                symbol: "ALUA".into(),
            },
        );
        let q = Quote::unavailable(&byma, QuoteState::Missing, Utc::now());
        let c = cfg(vec![byma]);
        let out = build(
            &c,
            std::slice::from_ref(&q),
            Utc::now(),
            &ThemeColors::default(),
        );
        let row = &out.groups[0].rows[0];
        assert_eq!(row.state, "missing");
        assert_eq!(row.price, None);
        assert_eq!(
            row.quote.as_deref(),
            Some("ars"),
            "priceless, not currencyless"
        );
    }

    #[test]
    fn a_closed_market_is_flagged_per_row_and_per_group() {
        // 2026-07-02 00:00 UTC = 20:00 EDT: US market closed; crypto is 24/7.
        let now = Utc.with_ymd_and_hms(2026, 7, 2, 0, 0, 0).unwrap();
        let c = cfg(vec![
            asset(
                "AAPL",
                AssetSource::Cnbc {
                    symbol: "AAPL".into(),
                },
            ),
            btc_asset(),
        ]);
        let qs = vec![
            quote("AAPL", Some(201.5), Some(0.4), QuoteState::Fresh),
            quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh),
        ];
        let out = build(&c, &qs, now, &ThemeColors::default());
        let stocks = out.groups.iter().find(|g| g.id == "stocks").unwrap();
        let crypto = out.groups.iter().find(|g| g.id == "crypto").unwrap();
        assert!(stocks.closed);
        assert_eq!(stocks.rows[0].market, "closed");
        assert!(!crypto.closed);
        assert_eq!(crypto.rows[0].market, "open");
    }

    #[test]
    fn the_error_output_is_valid_and_carries_a_structured_message() {
        let out = error_output(
            "config parse error: boom",
            Utc::now(),
            &ThemeColors::default(),
        );
        assert_eq!(out.state, "error");
        assert_eq!(out.schema_version, SCHEMA_VERSION);
        let s = serde_json::to_string(&out).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        // Uniform error shape: null | {message, code?}. No `code` here, so the key is absent.
        assert_eq!(v["error"]["message"], "config parse error: boom");
        assert!(v["error"].get("code").is_none());
        assert!(v["groups"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_layout_hints_mirror_the_tooltip_column_config() {
        let mut c = cfg(vec![btc_asset()]);
        c.display.tooltip_rows_per_column = 8;
        c.display.tooltip_max_columns = 2;
        let qs = vec![quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh)];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        assert_eq!(out.layout.rows_per_column, 8);
        assert_eq!(out.layout.max_columns, 2);
        // and the defaults (0 = unlimited) ride through untouched, including on errors.
        assert_eq!(
            error_output("x", Utc::now(), &ThemeColors::default())
                .layout
                .rows_per_column,
            0
        );
        assert!(error_output("x", Utc::now(), &ThemeColors::default())
            .layout
            .bands
            .is_empty());
    }

    #[test]
    fn the_packed_bands_come_from_the_shared_plan_with_continuations_and_banding() {
        // 5 BTC rows in one group, budget 3, max 1 column side by side:
        // lines [H,R0,R1,R2,R3,R4] -> columns [H,R0,R1] [R2,R3,R4] -> 2 bands of 1 column.
        let mut c = cfg(vec![
            btc_asset(),
            btc_asset(),
            btc_asset(),
            btc_asset(),
            btc_asset(),
        ]);
        c.display.tooltip_rows_per_column = 3;
        c.display.tooltip_max_columns = 1;
        let qs: Vec<Quote> = (0..5)
            .map(|i| {
                quote(
                    "BTC",
                    Some(1.0 + f64::from(i)),
                    Some(1.0),
                    QuoteState::Fresh,
                )
            })
            .collect();
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        let bands = &out.layout.bands;
        assert_eq!(bands.len(), 2, "extra columns wrap into stacked bands");
        assert_eq!(bands[0].len(), 1);
        let first = &bands[0][0][0];
        assert_eq!(
            (first.group, first.start, first.len, first.continued),
            (0, 0, 2, false)
        );
        let cont = &bands[1][0][0];
        assert_eq!(
            (cont.group, cont.start, cont.len, cont.continued),
            (0, 2, 3, true)
        );
        // Segments tile the group's rows exactly once, in order.
        let total: usize = bands.iter().flatten().flatten().map(|s| s.len).sum();
        assert_eq!(total, out.groups[0].rows.len());
    }

    #[test]
    fn a_single_column_plan_is_emitted_when_wrapping_is_off() {
        let c = cfg(vec![btc_asset()]); // rows_per_column = 0 (single column)
        let qs = vec![quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh)];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        assert_eq!(out.layout.bands.len(), 1);
        assert_eq!(out.layout.bands[0].len(), 1);
        let seg = &out.layout.bands[0][0][0];
        assert_eq!(
            (seg.group, seg.start, seg.len, seg.continued),
            (0, 0, 1, false)
        );
    }

    #[test]
    fn a_healthy_document_serializes_error_fields_as_null() {
        let c = cfg(vec![btc_asset()]);
        let qs = vec![quote("BTC", Some(68000.0), Some(1.2), QuoteState::Fresh)];
        let s =
            serde_json::to_string(&build(&c, &qs, Utc::now(), &ThemeColors::default())).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(v["error"].is_null());
        assert!(v["groups"][0]["rows"][0]["error"].is_null());
    }

    #[test]
    fn a_non_finite_change_is_emitted_as_null() {
        let c = cfg(vec![btc_asset()]);
        let qs = vec![quote("BTC", Some(1.0), Some(f64::NAN), QuoteState::Fresh)];
        let out = build(&c, &qs, Utc::now(), &ThemeColors::default());
        assert_eq!(out.groups[0].rows[0].change_pct, None);
        // and the whole document still serializes (NaN would poison serde_json).
        assert!(serde_json::to_string(&out).is_ok());
    }
}
