pub mod cnbc;
pub mod coingecko;
pub mod data912;
pub mod dolarapi;
pub mod finnhub;
pub mod frankfurter;
pub mod stooq;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};

use crate::platform::cache::{self, FetchPolicy};
use crate::platform::config::MarketHours;
use crate::platform::http::Http;
use crate::platform::market::{self, Gate};
use crate::platform::model::*;

fn ttl(kind: ProviderKind) -> Duration {
    match kind {
        ProviderKind::CoinGecko => Duration::seconds(60),
        ProviderKind::DolarApi => Duration::seconds(600),
        ProviderKind::Stooq => Duration::seconds(600),
        ProviderKind::Frankfurter => Duration::seconds(3600),
        ProviderKind::Finnhub => Duration::seconds(60),
        ProviderKind::Cnbc => Duration::seconds(120),
        ProviderKind::Data912 => Duration::seconds(300),
    }
}

fn fetch_kind(
    kind: ProviderKind,
    assets: &[&Asset],
    http: &Http,
    now: DateTime<Utc>,
) -> Result<Vec<Quote>, FetchError> {
    match kind {
        ProviderKind::CoinGecko => coingecko::fetch(assets, http, now),
        ProviderKind::DolarApi => dolarapi::fetch(assets, http, now),
        ProviderKind::Stooq => stooq::fetch(assets, http, now),
        ProviderKind::Frankfurter => frankfurter::fetch(assets, http, now),
        ProviderKind::Finnhub => finnhub::fetch(assets, http, now),
        ProviderKind::Cnbc => cnbc::fetch(assets, http, now),
        ProviderKind::Data912 => data912::fetch(assets, http, now),
    }
}

/// Group asset indices by provider, preserving config order within each group.
fn group_indexed(assets: &[Asset]) -> BTreeMap<ProviderKind, Vec<usize>> {
    let mut g: BTreeMap<ProviderKind, Vec<usize>> = BTreeMap::new();
    for (i, a) in assets.iter().enumerate() {
        g.entry(a.source.kind()).or_default().push(i);
    }
    g
}

/// Cache key = kind + descriptors in INPUT order, so a key uniquely implies an order
/// (lets us reassemble by index against a cache hit without mismatch).
fn build_key(kind: ProviderKind, group: &[&Asset]) -> String {
    let parts: Vec<String> = group.iter().map(|a| a.source.cache_descriptor()).collect();
    format!("{}|{}", kind.as_str(), parts.join("|"))
}

/// Place each group's quotes back into config order by index, re-stamping the display label
/// from the current config (so a rename is reflected even on a cache hit). Gaps -> Missing.
fn assemble(
    assets: &[Asset],
    groups: Vec<(Vec<usize>, Vec<Quote>)>,
    now: DateTime<Utc>,
) -> Vec<Quote> {
    let mut out: Vec<Option<Quote>> = (0..assets.len()).map(|_| None).collect();
    for (indices, quotes) in groups {
        for (slot, idx) in indices.iter().enumerate() {
            if let Some(q) = quotes.get(slot) {
                let mut q = q.clone();
                q.label = assets[*idx].label.clone();
                out[*idx] = Some(q);
            }
        }
    }
    out.into_iter()
        .enumerate()
        .map(|(i, o)| o.unwrap_or_else(|| Quote::unavailable(&assets[i], QuoteState::Missing, now)))
        .collect()
}

/// Fetch policy for a provider group. The group is ONE batched HTTP call, so it is fetched if
/// ANY asset's market is open (e.g. a CNBC batch with a 24/5 commodity stays live even when the
/// equity session is closed). Closed only when every asset is closed, at the latest close seen.
fn group_policy(group: &[&Asset], now: DateTime<Utc>, market: &MarketHours) -> FetchPolicy {
    let mut last_close = None;
    for a in group {
        match market::gate(&a.source, now, market) {
            Gate::Open => return FetchPolicy::Normal,
            Gate::Closed { last_close: lc } => last_close = Some(lc),
        }
    }
    match last_close {
        Some(lc) => FetchPolicy::Closed { last_close: lc },
        None => FetchPolicy::Normal,
    }
}

/// Fetch every configured asset, grouped by provider, each group through the cache.
/// Closed markets (per `market`) are not re-fetched; their last close is served from cache.
pub fn fetch_all(
    assets: &[Asset],
    http: &Http,
    now: DateTime<Utc>,
    market: &MarketHours,
) -> Vec<Quote> {
    let dir = cache::cache_dir();
    let grouped = group_indexed(assets);
    let mut results: Vec<(Vec<usize>, Vec<Quote>)> = Vec::new();
    for (kind, indices) in grouped {
        let group: Vec<&Asset> = indices.iter().map(|i| &assets[*i]).collect();
        let key = build_key(kind, &group);
        let group_owned: Vec<Asset> = group.iter().map(|a| (*a).clone()).collect();
        let policy = group_policy(&group, now, market);
        let quotes = cache::get_or_fetch(&dir, &key, ttl(kind), now, policy, || {
            let refs: Vec<&Asset> = group_owned.iter().collect();
            fetch_kind(kind, &refs, http, now)
        });
        results.push((indices, quotes));
    }
    assemble(assets, results, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn cg(label: &str, id: &str) -> Asset {
        Asset {
            label: label.into(),
            source: AssetSource::Coingecko {
                id: id.into(),
                quote: "usd".into(),
            },
        }
    }
    fn st(label: &str, sym: &str) -> Asset {
        Asset {
            label: label.into(),
            source: AssetSource::Stooq { symbol: sym.into() },
        }
    }

    #[test]
    fn assets_are_grouped_by_provider_preserving_order() {
        let assets = vec![
            cg("BTC", "bitcoin"),
            st("AAPL", "aapl.us"),
            cg("ETH", "ethereum"),
        ];
        let g = group_indexed(&assets);
        assert_eq!(g[&ProviderKind::CoinGecko], vec![0, 2]);
        assert_eq!(g[&ProviderKind::Stooq], vec![1]);
    }

    #[test]
    fn the_cache_key_is_deterministic_and_order_sensitive() {
        let a1 = cg("BTC", "bitcoin");
        let a2 = cg("ETH", "ethereum");
        let g_ab: Vec<&Asset> = vec![&a1, &a2];
        let g_ba: Vec<&Asset> = vec![&a2, &a1];
        assert_eq!(
            build_key(ProviderKind::CoinGecko, &g_ab),
            build_key(ProviderKind::CoinGecko, &g_ab)
        );
        assert_ne!(
            build_key(ProviderKind::CoinGecko, &g_ab),
            build_key(ProviderKind::CoinGecko, &g_ba)
        );
    }

    #[test]
    fn assemble_places_quotes_in_config_order_and_restamps_labels() {
        let assets = vec![cg("BTC", "bitcoin"), st("AAPL", "aapl.us")];
        let now = Utc::now();
        let mut qcg = Quote::unavailable(&assets[0], QuoteState::Fresh, now);
        qcg.label = "OLD".into();
        qcg.price = Some(1.0);
        let qst = Quote::unavailable(&assets[1], QuoteState::Fresh, now);
        let groups = vec![(vec![0], vec![qcg]), (vec![1], vec![qst])];
        let out = assemble(&assets, groups, now);
        assert_eq!(out[0].label, "BTC");
        assert_eq!(out[1].label, "AAPL");
    }

    #[test]
    fn a_missing_group_quote_is_filled_as_unavailable() {
        let assets = vec![cg("BTC", "bitcoin")];
        let now = Utc::now();
        let groups = vec![(vec![0], vec![])];
        let out = assemble(&assets, groups, now);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].state, QuoteState::Missing));
    }

    #[test]
    fn duplicate_labels_do_not_collide() {
        let assets = vec![cg("X", "bitcoin"), cg("X", "ethereum")];
        let now = Utc::now();
        let mut q0 = Quote::unavailable(&assets[0], QuoteState::Fresh, now);
        q0.price = Some(1.0);
        let mut q1 = Quote::unavailable(&assets[1], QuoteState::Fresh, now);
        q1.price = Some(2.0);
        let groups = vec![(vec![0, 1], vec![q0, q1])];
        let out = assemble(&assets, groups, now);
        assert_eq!(out[0].price, Some(1.0));
        assert_eq!(out[1].price, Some(2.0));
    }
}
