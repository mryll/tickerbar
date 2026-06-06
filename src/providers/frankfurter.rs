use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::platform::http::Http;
use crate::platform::model::*;

pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    // Frankfurter requires one base currency per request — group accordingly.
    let mut by_base: BTreeMap<String, Vec<&Asset>> = BTreeMap::new();
    for a in assets {
        if let AssetSource::Frankfurter { base, .. } = &a.source {
            by_base.entry(base.to_lowercase()).or_default().push(a);
        }
    }
    let mut out = Vec::new();
    for (base, group) in by_base {
        let quotes: Vec<String> = group
            .iter()
            .filter_map(|a| match &a.source {
                AssetSource::Frankfurter { quote, .. } => Some(quote.to_uppercase()),
                _ => None,
            })
            .collect();
        let url = reqwest::Url::parse_with_params(
            "https://api.frankfurter.dev/v2/rates",
            &[
                ("base", base.to_uppercase().as_str()),
                ("quotes", quotes.join(",").as_str()),
            ],
        )
        .map_err(|e| FetchError::Other(format!("url build: {e}")))?;
        let body = http.get(url.as_str())?;
        out.extend(parse_base(&base, &body, &group, now));
    }
    Ok(out)
}

fn parse_base(base: &str, body: &str, assets: &[&Asset], now: DateTime<Utc>) -> Vec<Quote> {
    let root: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            return assets
                .iter()
                .map(|a| Quote::unavailable(a, QuoteState::Error, now))
                .collect()
        }
    };
    let as_of = root
        .get("date")
        .and_then(Value::as_str)
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
    assets
        .iter()
        .map(|a| {
            let quote = match &a.source {
                AssetSource::Frankfurter { quote, .. } => quote.to_uppercase(),
                _ => return Quote::unavailable(a, QuoteState::Error, now),
            };
            let price = root
                .get("rates")
                .and_then(|r| r.get(&quote))
                .and_then(Value::as_f64)
                .filter(|p| p.is_finite() && *p != 0.0);
            match price {
                Some(p) => Quote {
                    label: a.label.clone(),
                    base: base.to_string(),
                    quote: quote.to_lowercase(),
                    native_quote: quote.to_lowercase(),
                    price: Some(p),
                    change_pct: None,
                    change_abs: None,
                    direction: None,
                    source: ProviderKind::Frankfurter,
                    as_of,
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

    fn asset(base: &str, quote: &str) -> Asset {
        Asset {
            label: format!("{base}/{quote}"),
            source: AssetSource::Frankfurter {
                base: base.into(),
                quote: quote.into(),
            },
        }
    }

    #[test]
    fn a_pair_rate_is_read_for_its_base() {
        let body = include_str!("../../tests/fixtures/frankfurter_eur.json");
        let assets = vec![asset("eur", "usd")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse_base("eur", body, &refs, Utc::now());
        assert_eq!(qs[0].price, Some(1.08));
    }

    #[test]
    fn a_missing_quote_currency_is_missing() {
        let body = include_str!("../../tests/fixtures/frankfurter_eur.json");
        let assets = vec![asset("eur", "jpy")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse_base("eur", body, &refs, Utc::now());
        assert_eq!(qs[0].state, QuoteState::Missing);
    }
}
