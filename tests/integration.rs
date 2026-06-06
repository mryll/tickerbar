// Real-API smoke tests. Run with: cargo test --features integration
// These hit live endpoints and may be flaky/offline — never part of default CI.
#![cfg(feature = "integration")]

use chrono::Utc;
use tickerbar::platform::http::Http;
use tickerbar::platform::model::*;
use tickerbar::providers::{self, coingecko, dolarapi, frankfurter};

#[test]
fn coingecko_returns_a_positive_btc_usd_price() {
    let http = Http::new(8);
    let a = Asset {
        label: "BTC".into(),
        source: AssetSource::Coingecko {
            id: "bitcoin".into(),
            quote: "usd".into(),
        },
    };
    let qs = coingecko::fetch(&[&a], &http, Utc::now()).expect("coingecko fetch");
    assert!(qs[0].price.unwrap_or(0.0) > 0.0);
}

#[test]
fn dolarapi_returns_a_blue_dollar_price() {
    let http = Http::new(8);
    let a = Asset {
        label: "Blue".into(),
        source: AssetSource::Dolarapi {
            casa: "blue".into(),
            side: Side::Sell,
        },
    };
    let qs = dolarapi::fetch(&[&a], &http, Utc::now()).expect("dolarapi fetch");
    assert!(qs[0].price.unwrap_or(0.0) > 0.0);
}

#[test]
fn frankfurter_returns_an_eur_usd_rate() {
    let http = Http::new(8);
    let a = Asset {
        label: "EUR/USD".into(),
        source: AssetSource::Frankfurter {
            base: "eur".into(),
            quote: "usd".into(),
        },
    };
    let qs = frankfurter::fetch(&[&a], &http, Utc::now()).expect("frankfurter fetch");
    assert!(qs[0].price.unwrap_or(0.0) > 0.0);
}

#[test]
fn stooq_degrades_gracefully_through_fetch_all() {
    // Stooq is frequently bot-walled; via fetch_all the widget must still return exactly one
    // quote for the asset (Fresh if reachable, otherwise Missing/Stale) and never error.
    let http = Http::new(8);
    let assets = vec![Asset {
        label: "AAPL".into(),
        source: AssetSource::Stooq {
            symbol: "aapl.us".into(),
        },
    }];
    let qs = providers::fetch_all(&assets, &http, Utc::now());
    assert_eq!(qs.len(), 1);
    assert_eq!(qs[0].label, "AAPL");
}

#[test]
fn finnhub_returns_a_quote_when_a_token_is_set() {
    // Skips unless FINNHUB_TOKEN is exported (free key).
    if std::env::var("FINNHUB_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .is_none()
    {
        eprintln!("skipping finnhub test: FINNHUB_TOKEN not set");
        return;
    }
    let http = Http::new(8);
    let a = Asset {
        label: "AAPL".into(),
        source: AssetSource::Finnhub {
            symbol: "AAPL".into(),
        },
    };
    let qs = providers::finnhub::fetch(&[&a], &http, Utc::now()).expect("finnhub fetch");
    assert!(qs[0].price.unwrap_or(0.0) > 0.0);
}
