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

pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let syms: Vec<&str> = assets
        .iter()
        .filter_map(|a| match &a.source {
            AssetSource::Cnbc { symbol } => Some(symbol.as_str()),
            _ => None,
        })
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

/// "7,383.74" / "$307.34" -> 7383.74 / 307.34
fn parse_amount(s: &str) -> Option<f64> {
    s.trim()
        .trim_start_matches('$')
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
            let symbol = match &a.source {
                AssetSource::Cnbc { symbol } => symbol.clone(),
                _ => return Quote::unavailable(a, QuoteState::Error, now),
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
}
