use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::platform::http::Http;
use crate::platform::model::*;

/// Optional keyed stock provider. The token comes from `FINNHUB_TOKEN` at runtime and is
/// sent via the `X-Finnhub-Token` header — never in the URL, so it cannot leak into error
/// strings or logs. With no token, every symbol is reported as Missing (not an error).
pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let token = std::env::var("FINNHUB_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    let token = match token {
        Some(t) => t,
        None => {
            return Ok(assets
                .iter()
                .map(|a| Quote::unavailable(a, QuoteState::Missing, now))
                .collect())
        }
    };

    // Finnhub /quote is one symbol per request.
    let mut out = Vec::with_capacity(assets.len());
    for a in assets {
        let symbol = match &a.source {
            AssetSource::Finnhub { symbol } => symbol.clone(),
            _ => {
                out.push(Quote::unavailable(a, QuoteState::Error, now));
                continue;
            }
        };
        let url = reqwest::Url::parse_with_params(
            "https://finnhub.io/api/v1/quote",
            &[("symbol", symbol.as_str())],
        )
        .map_err(|e| FetchError::Other(format!("url build: {e}")))?;
        let body = http.get_with_header(url.as_str(), Some(("X-Finnhub-Token", &token)))?;
        // A malformed body is a provider failure → propagate Err so the cache keeps stale.
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| FetchError::Other(format!("finnhub: unexpected response: {e}")))?;
        out.push(parse_one(a, &symbol, &v, now));
    }
    Ok(out)
}

fn parse_one(asset: &Asset, symbol: &str, v: &Value, now: DateTime<Utc>) -> Quote {
    let current = v.get("c").and_then(Value::as_f64);
    let change_pct = v.get("dp").and_then(Value::as_f64);
    let change_abs = v.get("d").and_then(Value::as_f64);
    let epoch = v.get("t").and_then(Value::as_i64);
    match current {
        // `c <= 0` means Finnhub has no data for the symbol.
        Some(p) if p.is_finite() && p > 0.0 => Quote {
            label: asset.label.clone(),
            base: symbol.to_string(),
            quote: String::new(),
            native_quote: String::new(),
            price: Some(p),
            change_pct,
            change_abs,
            direction: change_pct.map(Direction::from_change),
            source: ProviderKind::Finnhub,
            as_of: epoch
                .filter(|&t| t > 0)
                .and_then(|t| DateTime::<Utc>::from_timestamp(t, 0)),
            fetched_at: now,
            state: QuoteState::Fresh,
        },
        _ => Quote::unavailable(asset, QuoteState::Missing, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn asset(sym: &str) -> Asset {
        Asset {
            label: sym.into(),
            source: AssetSource::Finnhub { symbol: sym.into() },
        }
    }

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn a_finnhub_quote_maps_price_and_percent_change() {
        let a = asset("AAPL");
        let v = json(r#"{"c":201.5,"d":2.5,"dp":1.25,"t":1780000000}"#);
        let q = parse_one(&a, "AAPL", &v, Utc::now());
        assert_eq!(q.price, Some(201.5));
        assert_eq!(q.change_pct, Some(1.25));
        assert_eq!(q.direction, Some(Direction::Up));
        assert!(q.as_of.is_some());
    }

    #[test]
    fn a_zero_current_price_is_missing() {
        let a = asset("XYZ");
        let v = json(r#"{"c":0,"d":0,"dp":0,"t":0}"#);
        let q = parse_one(&a, "XYZ", &v, Utc::now());
        assert_eq!(q.state, QuoteState::Missing);
    }

    #[test]
    fn a_response_without_a_price_field_is_missing() {
        let a = asset("AAPL");
        let v = json("{}");
        let q = parse_one(&a, "AAPL", &v, Utc::now());
        assert_eq!(q.state, QuoteState::Missing);
    }
}
