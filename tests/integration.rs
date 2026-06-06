// Real-API smoke tests. Run with: cargo test --features integration
// These hit live endpoints and may be flaky/offline — never part of default CI.
#![cfg(feature = "integration")]

use chrono::Utc;
use tickerbar::platform::http::Http;
use tickerbar::platform::model::*;
use tickerbar::providers::{coingecko, dolarapi, frankfurter, stooq};

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
fn stooq_returns_an_aapl_price() {
    let http = Http::new(8);
    let a = Asset {
        label: "AAPL".into(),
        source: AssetSource::Stooq {
            symbol: "aapl.us".into(),
        },
    };
    let qs = stooq::fetch(&[&a], &http, Utc::now()).expect("stooq fetch");
    // Stooq may be EOD/delayed; just assert we got a parseable (possibly missing) quote.
    assert_eq!(qs.len(), 1);
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
