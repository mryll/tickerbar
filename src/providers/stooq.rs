use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::platform::http::Http;
use crate::platform::model::*;

pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let syms: Vec<String> = assets
        .iter()
        .filter_map(|a| match &a.source {
            AssetSource::Stooq { symbol } => Some(symbol.clone()),
            _ => None,
        })
        .collect();
    let joined = syms.join(",");
    let url = reqwest::Url::parse_with_params(
        "https://stooq.com/q/l/",
        &[("s", joined.as_str()), ("f", "sd2t2ohlcv"), ("e", "csv")],
    )
    .map_err(|e| FetchError::Other(format!("url build: {e}")))?;
    // Stooq expects a bare `h` flag (include header row), not `h=`.
    let full = format!("{url}&h");
    let body = http.get(&full)?;
    Ok(parse(&body, assets, now))
}

fn parse(body: &str, assets: &[&Asset], now: DateTime<Utc>) -> Vec<Quote> {
    // Map lowercased symbol -> close price (column index 6 with f=sd2t2ohlcv).
    let mut prices: HashMap<String, Option<f64>> = HashMap::new();
    for line in body.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 7 {
            continue;
        }
        let sym = cols[0].trim().to_lowercase();
        let price = cols[6]
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|p| p.is_finite() && *p != 0.0);
        prices.insert(sym, price);
    }
    assets
        .iter()
        .map(|a| {
            let sym = match &a.source {
                AssetSource::Stooq { symbol } => symbol.to_lowercase(),
                _ => return Quote::unavailable(a, QuoteState::Error, now),
            };
            match prices.get(&sym).copied().flatten() {
                Some(p) => Quote {
                    label: a.label.clone(),
                    base: sym.clone(),
                    quote: String::new(),
                    native_quote: String::new(),
                    price: Some(p),
                    change_pct: None,
                    change_abs: None,
                    direction: None,
                    source: ProviderKind::Stooq,
                    as_of: None,
                    fetched_at: now,
                    state: QuoteState::Fresh,
                },
                None => Quote::unavailable(a, QuoteState::Missing, now),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn asset(sym: &str) -> Asset {
        Asset {
            label: sym.into(),
            source: AssetSource::Stooq { symbol: sym.into() },
        }
    }

    #[test]
    fn a_close_price_is_parsed_from_the_csv_row() {
        let body = include_str!("../../tests/fixtures/stooq_ok.csv");
        let assets = [asset("aapl.us")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now());
        assert_eq!(qs[0].price, Some(201.5));
    }

    #[test]
    fn an_nd_row_is_treated_as_missing() {
        let body = include_str!("../../tests/fixtures/stooq_ok.csv");
        let assets = [asset("spy.us")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now());
        assert_eq!(qs[0].state, QuoteState::Missing);
    }
}
