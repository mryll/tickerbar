use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::platform::http::Http;
use crate::platform::model::*;

pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    let mut quotes: BTreeSet<&str> = BTreeSet::new();
    for a in assets {
        if let AssetSource::Coingecko { id, quote } = &a.source {
            ids.insert(id);
            quotes.insert(quote);
        }
    }
    let ids = ids.into_iter().collect::<Vec<_>>().join(",");
    let vs = quotes.into_iter().collect::<Vec<_>>().join(",");
    let url = reqwest::Url::parse_with_params(
        "https://api.coingecko.com/api/v3/simple/price",
        &[
            ("ids", ids.as_str()),
            ("vs_currencies", vs.as_str()),
            ("include_24hr_change", "true"),
            ("include_last_updated_at", "true"),
        ],
    )
    .map_err(|e| FetchError::Other(format!("url build: {e}")))?;
    let body = http.get(url.as_str())?;
    Ok(parse(&body, assets, now))
}

fn parse(body: &str, assets: &[&Asset], now: DateTime<Utc>) -> Vec<Quote> {
    let root: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            return assets
                .iter()
                .map(|a| Quote::unavailable(a, QuoteState::Error, now))
                .collect()
        }
    };
    assets
        .iter()
        .map(|a| {
            let (id, quote) = match &a.source {
                AssetSource::Coingecko { id, quote } => (id.as_str(), quote.as_str()),
                _ => return Quote::unavailable(a, QuoteState::Error, now),
            };
            let obj = root.get(id);
            let price = obj.and_then(|o| o.get(quote)).and_then(Value::as_f64);
            let change = obj
                .and_then(|o| o.get(format!("{quote}_24h_change")))
                .and_then(Value::as_f64);
            let as_of = obj
                .and_then(|o| o.get("last_updated_at"))
                .and_then(Value::as_i64)
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));
            match price {
                Some(p) if p.is_finite() && p != 0.0 => Quote {
                    label: a.label.clone(),
                    base: id.to_string(),
                    quote: quote.to_string(),
                    native_quote: quote.to_string(),
                    price: Some(p),
                    change_pct: change,
                    change_abs: None,
                    direction: change.map(Direction::from_change),
                    source: ProviderKind::CoinGecko,
                    as_of,
                    fetched_at: now,
                    state: QuoteState::Fresh,
                },
                _ => Quote::unavailable(a, QuoteState::Missing, now),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn asset(label: &str, id: &str, quote: &str) -> Asset {
        Asset {
            label: label.into(),
            source: AssetSource::Coingecko {
                id: id.into(),
                quote: quote.into(),
            },
        }
    }

    #[test]
    fn a_coingecko_response_maps_to_a_quote_with_price_and_24h_change() {
        let body = include_str!("../../tests/fixtures/coingecko_ok.json");
        let assets = vec![asset("BTC", "bitcoin", "usd")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now());
        assert_eq!(qs[0].price, Some(68000.5));
        assert_eq!(qs[0].change_pct, Some(1.23));
        assert_eq!(qs[0].direction, Some(Direction::Up));
        assert_eq!(qs[0].state, QuoteState::Fresh);
        assert!(qs[0].as_of.is_some());
    }

    #[test]
    fn a_zero_price_is_treated_as_missing_not_zero() {
        let body = include_str!("../../tests/fixtures/coingecko_ok.json");
        let assets = vec![asset("ETH", "ethereum", "usd")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now());
        assert_eq!(qs[0].price, None);
        assert_eq!(qs[0].state, QuoteState::Missing);
    }

    #[test]
    fn a_symbol_absent_from_the_batch_is_reported_as_missing() {
        let assets = vec![asset("DOGE", "dogecoin", "usd")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse("{}", &refs, Utc::now());
        assert_eq!(qs[0].state, QuoteState::Missing);
    }

    #[test]
    fn a_malformed_body_yields_error_quotes_without_panicking() {
        let assets = vec![asset("BTC", "bitcoin", "usd")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse("{not json", &refs, Utc::now());
        assert_eq!(qs[0].state, QuoteState::Error);
    }
}
