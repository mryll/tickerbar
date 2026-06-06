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
    pair(label, AssetSource::Cnbc { symbol: sym.into() }, price, Some(chg))
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

fn build_output(pairs: Vec<(Asset, Quote)>, display: Display) -> tickerbar::platform::waybar::WaybarOutput {
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
    let pairs = vec![
        cg("BTC", "bitcoin", 68_000.50, 1.23),
        cg("ETH", "ethereum", 3_205.80, -0.45),
        us("AAPL", "AAPL", 232.10, 0.88),
        us("TSLA", "TSLA", 261.40, -2.11),
        us("S&P 500", ".SPX", 5_588.20, 0.32),
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
        cg("BNB", "binancecoin", 602.10, -1.02),
        cg("XRP", "ripple", 0.5821, 2.31),
        cg("ADA", "cardano", 0.4410, -0.88),
        cg("DOGE", "dogecoin", 0.1623, 5.66),
        cg("DOT", "polkadot", 6.84, -2.40),
        us("AAPL", "AAPL", 232.10, 0.88),
        us("MSFT", "MSFT", 428.90, 0.41),
        us("NVDA", "NVDA", 121.40, 3.92),
        us("TSLA", "TSLA", 261.40, -2.11),
        us("AMZN", "AMZN", 186.55, 1.04),
        us("GOOGL", "GOOGL", 178.20, -0.63),
        us("META", "META", 503.70, 1.77),
        us("NFLX", "NFLX", 678.30, -1.20),
        us("S&P 500", ".SPX", 5_588.20, 0.32),
        us("Nasdaq", ".IXIC", 18_240.10, 0.55),
        us("Dow", ".DJI", 39_120.40, -0.18),
        cg("LTC", "litecoin", 84.20, 1.51),
        cg("LINK", "chainlink", 14.07, -1.88),
        fx("EUR/USD", "eur", 1.0852, -0.12),
        fx("GBP/USD", "gbp", 1.2740, 0.22),
        fx("USD/JPY", "usd", 157.30, 0.34),
        fx("AUD/USD", "aud", 0.6645, -0.41),
        // A small taste of the Argentine-market support (full BYMA in the README).
        dolar("Blue", "blue", 1_030.0),
        dolar("MEP", "bolsa", 1_061.4),
        ar("ALUA", Panel::Acciones, 973.0, -3.56),
    ];
    let display = Display {
        tooltip_rows_per_column: 12,
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

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "simple".into());
    let out = match which.as_str() {
        "full" => full(),
        "bar" => bar(),
        _ => simple(),
    };
    println!("{out}");
}
