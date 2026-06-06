use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::platform::http::Http;
use crate::platform::model::*;

// No-key stock/index quotes from CNBC's public quote endpoint. Batches many symbols in one
// request (pipe-delimited) and returns last price + percent change. Stocks use plain tickers
// (AAPL); indices use a leading dot (.SPX, .IXIC, .DJI). Unofficial endpoint — best-effort.
#[derive(Deserialize)]
struct Resp {
    #[serde(rename = "FormattedQuoteResult")]
    result: QuoteResult,
}
#[derive(Deserialize)]
struct QuoteResult {
    #[serde(rename = "FormattedQuote", default)]
    quotes: Vec<Row>,
}
#[derive(Deserialize)]
struct Row {
    symbol: String,
    last: Option<String>,
    change: Option<String>,
    change_pct: Option<String>,
    #[serde(rename = "currencyCode")]
    currency: Option<String>,
}

// Friendly aliases → CNBC symbols. Unknown symbols pass through unchanged, so any raw CNBC
// symbol (e.g. "@GC.1") works too. Verified live: gold/silver/wti/natgas/copper, vix/sp500/
// nasdaq/dow, us10y/us2y.
const COMMODITY_ALIASES: &[(&str, &str)] = &[
    ("gold", "@GC.1"),
    ("silver", "@SI.1"),
    ("wti", "@CL.1"),
    ("crude", "@CL.1"),
    ("brent", "@LCO.1"),
    ("natgas", "@NG.1"),
    ("copper", "@HG.1"),
    ("platinum", "@PL.1"),
    ("palladium", "@PA.1"),
];
const INDEX_ALIASES: &[(&str, &str)] = &[
    ("vix", ".VIX"),
    ("sp500", ".SPX"),
    ("nasdaq", ".IXIC"),
    ("dow", ".DJI"),
    ("dax", ".GDAXI"),
    ("ftse", ".FTSE"),
    ("nikkei", ".N225"),
    ("hangseng", ".HSI"),
];
const RATE_ALIASES: &[(&str, &str)] = &[
    ("us10y", "US10Y"),
    ("us2y", "US2Y"),
    ("us30y", "US30Y"),
    ("us5y", "US5Y"),
];

fn resolve(symbol: &str, table: &[(&str, &str)]) -> String {
    let key = symbol.to_lowercase();
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| symbol.to_string())
}

/// The CNBC request symbol for any CNBC-backed asset class, or None for other providers.
fn cnbc_symbol(src: &AssetSource) -> Option<String> {
    match src {
        AssetSource::Cnbc { symbol } => Some(symbol.clone()),
        AssetSource::Commodity { symbol } => Some(resolve(symbol, COMMODITY_ALIASES)),
        AssetSource::Index { symbol } => Some(resolve(symbol, INDEX_ALIASES)),
        AssetSource::Rate { symbol } => Some(resolve(symbol, RATE_ALIASES)),
        _ => None,
    }
}

pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let syms: Vec<String> = assets
        .iter()
        .filter_map(|a| cnbc_symbol(&a.source))
        .collect();
    let url = reqwest::Url::parse_with_params(
        "https://quote.cnbc.com/quote-html-webservice/restQuote/symbolType/symbol",
        &[
            ("symbols", syms.join("|").as_str()),
            ("requestMethod", "itv"),
            ("noform", "1"),
            ("output", "json"),
        ],
    )
    .map_err(|e| FetchError::Other(format!("url build: {e}")))?;
    let body = http.get(url.as_str())?;
    parse(&body, assets, now)
}

/// "7,383.74" / "$307.34" / "4.532%" -> 7383.74 / 307.34 / 4.532
/// (yields come as a percent string; the trailing `%` is stripped so they parse as a value).
fn parse_amount(s: &str) -> Option<f64> {
    s.trim()
        .trim_start_matches('$')
        .trim_end_matches('%')
        .replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|p| p.is_finite() && *p != 0.0)
}

/// "-1.25%" -> -1.25
fn parse_pct(s: &str) -> Option<f64> {
    s.trim()
        .trim_end_matches('%')
        .replace(',', "")
        .parse::<f64>()
        .ok()
}

fn parse(body: &str, assets: &[&Asset], now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let resp: Resp = serde_json::from_str(body)
        .map_err(|e| FetchError::Other(format!("cnbc: unexpected response: {e}")))?;
    let by_sym: HashMap<String, &Row> = resp
        .result
        .quotes
        .iter()
        .map(|r| (r.symbol.to_uppercase(), r))
        .collect();
    Ok(assets
        .iter()
        .map(|a| {
            let symbol = match cnbc_symbol(&a.source) {
                Some(s) => s,
                None => return Quote::unavailable(a, QuoteState::Error, now),
            };
            let row = by_sym.get(&symbol.to_uppercase());
            let price = row.and_then(|r| r.last.as_deref()).and_then(parse_amount);
            match price {
                Some(p) => {
                    let change_pct = row
                        .and_then(|r| r.change_pct.as_deref())
                        .and_then(parse_pct);
                    let change_abs = row.and_then(|r| r.change.as_deref()).and_then(parse_pct);
                    let quote = row
                        .and_then(|r| r.currency.as_deref())
                        .unwrap_or("usd")
                        .to_lowercase();
                    Quote {
                        label: a.label.clone(),
                        base: symbol.clone(),
                        quote: quote.clone(),
                        native_quote: quote,
                        price: Some(p),
                        change_pct,
                        change_abs,
                        direction: change_pct.map(Direction::from_change),
                        source: ProviderKind::Cnbc,
                        as_of: None,
                        fetched_at: now,
                        state: QuoteState::Fresh,
                    }
                }
                None => Quote::unavailable(a, QuoteState::Missing, now),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn asset(sym: &str) -> Asset {
        Asset {
            label: sym.into(),
            source: AssetSource::Cnbc { symbol: sym.into() },
        }
    }

    #[test]
    fn a_stock_row_maps_price_and_percent_change() {
        let body = include_str!("../../tests/fixtures/cnbc_ok.json");
        let assets = [asset("AAPL")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(307.34));
        assert_eq!(qs[0].change_pct, Some(-1.25));
        assert_eq!(qs[0].direction, Some(Direction::Down));
    }

    #[test]
    fn an_index_price_with_thousands_separators_is_parsed() {
        let body = include_str!("../../tests/fixtures/cnbc_ok.json");
        let assets = [asset(".SPX")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(7383.74));
    }

    #[test]
    fn an_unknown_symbol_is_missing() {
        let body = include_str!("../../tests/fixtures/cnbc_ok.json");
        let assets = [asset("NOPE")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].state, QuoteState::Missing);
    }

    #[test]
    fn a_whole_body_that_is_not_json_is_an_error() {
        let assets = [asset("AAPL")];
        let refs: Vec<&Asset> = assets.iter().collect();
        assert!(parse("<html>nope</html>", &refs, Utc::now()).is_err());
    }

    fn commodity(sym: &str) -> Asset {
        Asset {
            label: sym.into(),
            source: AssetSource::Commodity { symbol: sym.into() },
        }
    }
    fn index(sym: &str) -> Asset {
        Asset {
            label: sym.into(),
            source: AssetSource::Index { symbol: sym.into() },
        }
    }
    fn rate(sym: &str) -> Asset {
        Asset {
            label: sym.into(),
            source: AssetSource::Rate { symbol: sym.into() },
        }
    }

    #[test]
    fn friendly_aliases_resolve_to_their_cnbc_symbols() {
        assert_eq!(
            cnbc_symbol(&commodity("gold").source).as_deref(),
            Some("@GC.1")
        );
        assert_eq!(cnbc_symbol(&index("vix").source).as_deref(), Some(".VIX"));
        assert_eq!(cnbc_symbol(&rate("us10y").source).as_deref(), Some("US10Y"));
    }

    #[test]
    fn an_alias_is_case_insensitive() {
        assert_eq!(
            cnbc_symbol(&commodity("GOLD").source).as_deref(),
            Some("@GC.1")
        );
    }

    #[test]
    fn a_raw_cnbc_symbol_passes_through_unchanged() {
        assert_eq!(
            cnbc_symbol(&commodity("@GC.1").source).as_deref(),
            Some("@GC.1")
        );
        assert_eq!(cnbc_symbol(&index(".SPX").source).as_deref(), Some(".SPX"));
    }

    #[test]
    fn a_commodity_price_with_thousands_is_parsed_via_its_alias() {
        let body = include_str!("../../tests/fixtures/cnbc_classes.json");
        let assets = [commodity("gold")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(4353.90));
    }

    #[test]
    fn a_treasury_yield_strips_the_percent_and_parses_as_a_value() {
        let body = include_str!("../../tests/fixtures/cnbc_classes.json");
        let assets = [rate("us10y")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(4.532));
    }

    #[test]
    fn an_index_alias_maps_to_its_cnbc_quote() {
        let body = include_str!("../../tests/fixtures/cnbc_classes.json");
        let assets = [index("vix")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(21.51));
    }
}
