use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::platform::http::Http;
use crate::platform::model::*;

const URL: &str = "https://dolarapi.com/v1/dolares";

#[derive(Deserialize)]
struct Rate {
    casa: String,
    compra: Option<f64>,
    venta: Option<f64>,
    #[serde(rename = "fechaActualizacion")]
    fecha: Option<String>,
}

pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let body = http.get(URL)?;
    parse(&body, assets, now)
}

// A whole-body parse failure returns Err so the cache keeps the last good (stale) data.
// A missing `casa` within a valid response stays a per-asset Missing.
fn parse(body: &str, assets: &[&Asset], now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    let rates: Vec<Rate> = serde_json::from_str(body)
        .map_err(|e| FetchError::Other(format!("dolarapi: unexpected response: {e}")))?;
    Ok(assets
        .iter()
        .map(|a| {
            let (casa, side) = match &a.source {
                AssetSource::Dolarapi { casa, side } => (casa.as_str(), *side),
                _ => return Quote::unavailable(a, QuoteState::Error, now),
            };
            let r = rates.iter().find(|r| r.casa.eq_ignore_ascii_case(casa));
            let price = r
                .and_then(|r| match side {
                    Side::Buy => r.compra,
                    Side::Sell => r.venta,
                    Side::Mid => match (r.compra, r.venta) {
                        (Some(c), Some(v)) => Some((c + v) / 2.0),
                        _ => None,
                    },
                })
                .filter(|p| p.is_finite() && *p != 0.0);
            let as_of = r
                .and_then(|r| r.fecha.as_deref())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc));
            match price {
                Some(p) => Quote {
                    label: a.label.clone(),
                    base: "usd".to_string(),
                    quote: "ars".to_string(),
                    native_quote: "ars".to_string(),
                    price: Some(p),
                    change_pct: None,
                    change_abs: None,
                    direction: None,
                    source: ProviderKind::DolarApi,
                    as_of,
                    fetched_at: now,
                    state: QuoteState::Fresh,
                },
                None => Quote::unavailable(a, QuoteState::Missing, now),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn asset(casa: &str, side: Side) -> Asset {
        Asset {
            label: "Blue".into(),
            source: AssetSource::Dolarapi {
                casa: casa.into(),
                side,
            },
        }
    }

    #[test]
    fn the_blue_dollar_sell_side_is_read_from_the_response() {
        let body = include_str!("../../tests/fixtures/dolarapi_ok.json");
        let assets = [asset("blue", Side::Sell)];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(1030.0));
        assert_eq!(qs[0].base, "usd");
        assert_eq!(qs[0].quote, "ars");
        assert!(qs[0].as_of.is_some());
    }

    #[test]
    fn the_buy_side_reads_compra() {
        let body = include_str!("../../tests/fixtures/dolarapi_ok.json");
        let assets = [asset("blue", Side::Buy)];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(1010.0));
    }

    #[test]
    fn an_unknown_casa_is_missing() {
        let body = include_str!("../../tests/fixtures/dolarapi_ok.json");
        let assets = [asset("cripto", Side::Sell)];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse(body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].state, QuoteState::Missing);
    }

    #[test]
    fn a_whole_body_that_is_not_json_is_an_error() {
        let assets = [asset("blue", Side::Sell)];
        let refs: Vec<&Asset> = assets.iter().collect();
        assert!(parse("<html>nope</html>", &refs, Utc::now()).is_err());
    }
}
