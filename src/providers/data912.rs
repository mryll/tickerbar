use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::platform::http::Http;
use crate::platform::model::*;

// No-key Argentine market (BYMA) quotes from data912.com, in ARS, ~2h delayed. Each panel
// (acciones/bonos/cedears/corp) is one endpoint returning the whole panel as an array.
const BASE: &str = "https://data912.com/live/";

#[derive(Deserialize)]
struct Row {
    symbol: String,
    c: Option<f64>,
    pct_change: Option<f64>,
}

type PanelMap = HashMap<String, (Option<f64>, Option<f64>)>;

/// Parse one panel body into `symbol(upper) -> (last, pct_change)`. Whole-body failure -> Err.
fn panel_map(body: &str) -> Result<PanelMap, FetchError> {
    let rows: Vec<Row> = serde_json::from_str(body)
        .map_err(|e| FetchError::Other(format!("data912: unexpected response: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|r| (r.symbol.to_uppercase(), (r.c, r.pct_change)))
        .collect())
}

fn mk_quote(
    asset: &Asset,
    symbol: &str,
    c: Option<f64>,
    pct: Option<f64>,
    now: DateTime<Utc>,
) -> Quote {
    match c.filter(|p| p.is_finite() && *p != 0.0) {
        Some(p) => Quote {
            label: asset.label.clone(),
            base: symbol.to_string(),
            quote: "ars".to_string(),
            native_quote: "ars".to_string(),
            price: Some(p),
            change_pct: pct,
            change_abs: None,
            direction: pct.map(Direction::from_change),
            source: ProviderKind::Data912,
            as_of: None,
            fetched_at: now,
            state: QuoteState::Fresh,
        },
        None => Quote::unavailable(asset, QuoteState::Missing, now),
    }
}

pub fn fetch(assets: &[&Asset], http: &Http, now: DateTime<Utc>) -> Result<Vec<Quote>, FetchError> {
    // Fetch each distinct panel once (the endpoint returns the whole panel), then emit quotes
    // in the SAME order as the input assets (fetch_all reassembles by index).
    let mut panels: Vec<Panel> = Vec::new();
    for a in assets {
        if let AssetSource::Data912 { panel, .. } = &a.source {
            if !panels.contains(panel) {
                panels.push(*panel);
            }
        }
    }
    let mut maps: HashMap<&'static str, PanelMap> = HashMap::new();
    for panel in panels {
        let url = format!("{BASE}{}", panel.endpoint());
        let body = http.get(&url)?;
        maps.insert(panel.as_str(), panel_map(&body)?);
    }

    Ok(assets
        .iter()
        .map(|a| {
            let (panel, symbol) = match &a.source {
                AssetSource::Data912 { panel, symbol } => (*panel, symbol.clone()),
                _ => return Quote::unavailable(a, QuoteState::Error, now),
            };
            match maps
                .get(panel.as_str())
                .and_then(|m| m.get(&symbol.to_uppercase()))
            {
                Some((c, pct)) => mk_quote(a, &symbol, *c, *pct, now),
                None => Quote::unavailable(a, QuoteState::Missing, now),
            }
        })
        .collect())
}

/// Single-panel parse used by tests (all `assets` must belong to `panel`).
#[cfg(test)]
fn parse_panel(
    panel: Panel,
    body: &str,
    assets: &[&Asset],
    now: DateTime<Utc>,
) -> Result<Vec<Quote>, FetchError> {
    let map = panel_map(body)?;
    Ok(assets
        .iter()
        .map(|a| match &a.source {
            AssetSource::Data912 { panel: p, symbol } if *p == panel => {
                match map.get(&symbol.to_uppercase()) {
                    Some((c, pct)) => mk_quote(a, symbol, *c, *pct, now),
                    None => Quote::unavailable(a, QuoteState::Missing, now),
                }
            }
            _ => Quote::unavailable(a, QuoteState::Error, now),
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
            source: AssetSource::Data912 {
                panel: Panel::Acciones,
                symbol: sym.into(),
            },
        }
    }

    #[test]
    fn an_acciones_row_maps_price_and_percent_change() {
        let body = include_str!("../../tests/fixtures/data912_stocks.json");
        let assets = [asset("ALUA")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse_panel(Panel::Acciones, body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].price, Some(3290.0));
        assert_eq!(qs[0].change_pct, Some(-3.23));
        assert_eq!(qs[0].direction, Some(Direction::Down));
        assert_eq!(qs[0].quote, "ars");
    }

    #[test]
    fn a_zero_price_is_missing_but_a_zero_change_is_flat() {
        let body = include_str!("../../tests/fixtures/data912_stocks.json");
        let dead = [asset("DEAD")];
        let dead_refs: Vec<&Asset> = dead.iter().collect();
        assert_eq!(
            parse_panel(Panel::Acciones, body, &dead_refs, Utc::now()).unwrap()[0].state,
            QuoteState::Missing
        );
        let flat = [asset("FLAT")];
        let flat_refs: Vec<&Asset> = flat.iter().collect();
        let q = &parse_panel(Panel::Acciones, body, &flat_refs, Utc::now()).unwrap()[0];
        assert_eq!(q.price, Some(100.0));
        assert_eq!(q.direction, Some(Direction::Flat));
    }

    #[test]
    fn an_unknown_symbol_is_missing() {
        let body = include_str!("../../tests/fixtures/data912_stocks.json");
        let assets = [asset("NOPE")];
        let refs: Vec<&Asset> = assets.iter().collect();
        let qs = parse_panel(Panel::Acciones, body, &refs, Utc::now()).unwrap();
        assert_eq!(qs[0].state, QuoteState::Missing);
    }

    #[test]
    fn a_whole_body_that_is_not_json_is_an_error() {
        let assets = [asset("ALUA")];
        let refs: Vec<&Asset> = assets.iter().collect();
        assert!(parse_panel(Panel::Acciones, "<html>nope</html>", &refs, Utc::now()).is_err());
    }
}
