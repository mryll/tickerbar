//! Dev helper: print a tooltip's Pango markup for a mocked watchlist, to render README
//! screenshots with `pango-view --markup`. Not part of the shipped package.
//!
//! Usage: cargo run --example screenshot -- {simple|full}

use chrono::Utc;

use tickerbar::platform::config::{Config, Display, MarketHours};
use tickerbar::platform::model::{Asset, AssetSource, Direction, Panel, Quote, QuoteState};
use tickerbar::platform::render;
use tickerbar::platform::theme::ThemeColors;

fn pair(label: &str, source: AssetSource, price: f64, chg: Option<f64>) -> (Asset, Quote) {
    let kind = source.kind();
    let asset = Asset {
        label: label.to_string(),
        source,
    };
    let quote = Quote {
        label: label.to_string(),
        base: String::new(),
        quote: String::new(),
        native_quote: String::new(),
        price: Some(price),
        change_pct: chg,
        change_abs: None,
        direction: chg.map(Direction::from_change),
        day_high: None,
        day_low: None,
        source: kind,
        as_of: None,
        fetched_at: Utc::now(),
        state: QuoteState::Fresh,
    };
    (asset, quote)
}

fn cg(label: &str, id: &str, price: f64, chg: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Coingecko {
            id: id.into(),
            quote: "usd".into(),
        },
        price,
        Some(chg),
    )
}
fn us(label: &str, sym: &str, price: f64, chg: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Cnbc { symbol: sym.into() },
        price,
        Some(chg),
    )
}
fn fx(label: &str, base: &str, price: f64, chg: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Frankfurter {
            base: base.into(),
            quote: "usd".into(),
        },
        price,
        Some(chg),
    )
}
fn dolar(label: &str, casa: &str, price: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Dolarapi {
            casa: casa.into(),
            side: Default::default(),
        },
        price,
        None,
    )
}
fn ar(label: &str, panel: Panel, price: f64, chg: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Data912 {
            panel,
            symbol: label.into(),
        },
        price,
        Some(chg),
    )
}

fn com(label: &str, sym: &str, price: f64, chg: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Commodity { symbol: sym.into() },
        price,
        Some(chg),
    )
}
fn idx(label: &str, sym: &str, price: f64, chg: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Index { symbol: sym.into() },
        price,
        Some(chg),
    )
}
fn rate(label: &str, sym: &str, price: f64, chg: f64) -> (Asset, Quote) {
    pair(
        label,
        AssetSource::Rate { symbol: sym.into() },
        price,
        Some(chg),
    )
}

fn build_output(
    pairs: Vec<(Asset, Quote)>,
    display: Display,
) -> tickerbar::platform::waybar::WaybarOutput {
    let (assets, quotes): (Vec<Asset>, Vec<Quote>) = pairs.into_iter().unzip();
    let cfg = Config {
        display,
        // No market-hours gating in the demo so screenshots never show "closed".
        market_hours: MarketHours {
            enabled: false,
            providers: Default::default(),
        },
        assets,
    };
    let colors = ThemeColors::load();
    render::build(&cfg, &quotes, Utc::now(), &colors)
}

fn render_tooltip(pairs: Vec<(Asset, Quote)>, display: Display) -> String {
    build_output(pairs, display).tooltip
}

fn simple() -> String {
    // One of each asset class — the breadth in a single column.
    let pairs = vec![
        cg("BTC", "bitcoin", 68_000.50, 1.23),
        cg("ETH", "ethereum", 3_205.80, -0.45),
        us("AAPL", "AAPL", 232.10, 0.88),
        idx("S&P 500", "sp500", 5_588.20, 0.32),
        com("Gold", "gold", 2_412.30, 0.74),
        rate("US 10Y", "us10y", 4.53, 1.23),
        fx("EUR/USD", "eur", 1.0852, -0.12),
    ];
    let display = Display {
        tooltip_rows_per_column: 0,
        ..Display::default()
    };
    render_tooltip(pairs, display)
}

fn full() -> String {
    let pairs = vec![
        cg("BTC", "bitcoin", 68_000.50, 1.23),
        cg("ETH", "ethereum", 3_205.80, -0.45),
        cg("SOL", "solana", 168.22, 4.10),
        cg("XRP", "ripple", 0.5821, 2.31),
        cg("ADA", "cardano", 0.4410, -0.88),
        cg("DOGE", "dogecoin", 0.1623, 5.66),
        us("AAPL", "AAPL", 232.10, 0.88),
        us("MSFT", "MSFT", 428.90, 0.41),
        us("NVDA", "NVDA", 121.40, 3.92),
        us("TSLA", "TSLA", 261.40, -2.11),
        us("AMZN", "AMZN", 186.55, 1.04),
        idx("S&P 500", "sp500", 5_588.20, 0.32),
        idx("Nasdaq", "nasdaq", 18_240.10, 0.55),
        idx("Dow", "dow", 39_120.40, -0.18),
        idx("VIX", "vix", 13.20, -4.80),
        com("Gold", "gold", 2_412.30, 0.74),
        com("Silver", "silver", 31.55, 1.20),
        com("WTI Crude", "wti", 78.40, -1.36),
        com("Nat Gas", "natgas", 2.78, 2.04),
        com("Copper", "copper", 4.55, -0.42),
        rate("US 2Y", "us2y", 4.71, -0.30),
        rate("US 10Y", "us10y", 4.53, 1.23),
        rate("US 30Y", "us30y", 4.66, 0.88),
        fx("EUR/USD", "eur", 1.0852, -0.12),
        fx("USD/JPY", "usd", 157.30, 0.34),
        fx("GBP/USD", "gbp", 1.2740, 0.22),
        // A small taste of the Argentine-market support (full BYMA in the README).
        dolar("Blue", "blue", 1_030.0),
        ar("ALUA", Panel::Acciones, 973.0, -3.56),
    ];
    let display = Display {
        tooltip_rows_per_column: 13,
        tooltip_max_columns: 0,
        ..Display::default()
    };
    render_tooltip(pairs, display)
}

fn bar() -> String {
    // The compact in-bar line (the `text` field), a few assets shown inline.
    let pairs = vec![
        cg("BTC", "bitcoin", 68_000.50, 1.23),
        cg("ETH", "ethereum", 3_205.80, -0.45),
        us("AAPL", "AAPL", 232.10, 0.88),
        us("S&P 500", ".SPX", 5_588.20, 0.32),
        fx("EUR/USD", "eur", 1.0852, -0.12),
    ];
    let display = Display {
        max_on_bar: 5,
        ..Display::default()
    };
    build_output(pairs, display).text
}

fn ranged(pair: (Asset, Quote), lo: f64, hi: f64) -> (Asset, Quote) {
    let (a, mut q) = pair;
    q.day_low = Some(lo);
    q.day_high = Some(hi);
    (a, q)
}

/// Full WaybarOutput (bar text + tooltip) for a cross-class showcase with intraday ranges.
fn showcase() -> tickerbar::platform::waybar::WaybarOutput {
    let pairs = vec![
        cg("BTC", "bitcoin", 68_000.50, 1.23),
        cg("ETH", "ethereum", 3_205.80, -0.45),
        ranged(com("Gold", "gold", 2_412.30, 0.74), 2_398.00, 2_421.50),
        ranged(idx("S&P 500", "sp500", 5_588.20, 0.32), 5_560.10, 5_599.40),
        ranged(us("NVDA", "NVDA", 121.40, 3.92), 118.20, 123.05),
        ranged(idx("VIX", "vix", 13.20, -4.80), 12.80, 14.60),
        ranged(com("WTI", "wti", 78.40, -1.36), 77.10, 79.85),
        ranged(rate("US 10Y", "us10y", 4.53, 1.23), 4.49, 4.56),
        ranged(rate("US 2Y", "us2y", 4.71, -0.30), 4.68, 4.74),
        fx("EUR/USD", "eur", 1.0852, -0.12),
    ];
    let display = Display {
        max_on_bar: 4,
        tooltip_rows_per_column: 0,
        tooltip_max_columns: 0,
        tooltip_range: true,
        ..Display::default()
    };
    build_output(pairs, display)
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "simple".into());
    if which == "json" {
        let out = showcase();
        println!(
            "{}",
            serde_json::to_string(&out).expect("serialize WaybarOutput")
        );
        return;
    }
    let out = match which.as_str() {
        "full" => full(),
        "bar" => bar(),
        _ => simple(),
    };
    println!("{out}");
}
