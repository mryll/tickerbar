use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Flat,
}

impl Direction {
    pub fn from_change(pct: f64) -> Self {
        if pct > 0.0 {
            Direction::Up
        } else if pct < 0.0 {
            Direction::Down
        } else {
            Direction::Flat
        }
    }
}

/// A single state — no contradictory `status` + `stale` combinations.
/// `Stale` is applied on read by the cache layer; it is never stored as a permanent field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteState {
    Fresh,
    Stale,
    Missing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProviderKind {
    CoinGecko,
    DolarApi,
    Stooq,
    Frankfurter,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::CoinGecko => "coingecko",
            ProviderKind::DolarApi => "dolarapi",
            ProviderKind::Stooq => "stooq",
            ProviderKind::Frankfurter => "frankfurter",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
    Mid,
}

impl Default for Side {
    fn default() -> Self {
        Side::Sell
    }
}

/// Provider-specific asset shape. A tagged enum keyed by the TOML `provider` field, so a
/// CoinGecko asset cannot carry a Stooq `symbol` — invalid states are unrepresentable.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum AssetSource {
    Coingecko {
        id: String,
        quote: String,
    },
    Dolarapi {
        casa: String,
        #[serde(default)]
        side: Side,
    },
    Stooq {
        symbol: String,
    },
    Frankfurter {
        base: String,
        quote: String,
    },
}

impl AssetSource {
    pub fn kind(&self) -> ProviderKind {
        match self {
            AssetSource::Coingecko { .. } => ProviderKind::CoinGecko,
            AssetSource::Dolarapi { .. } => ProviderKind::DolarApi,
            AssetSource::Stooq { .. } => ProviderKind::Stooq,
            AssetSource::Frankfurter { .. } => ProviderKind::Frankfurter,
        }
    }

    /// Stable per-asset descriptor used (in input order) to build the cache key.
    pub fn cache_descriptor(&self) -> String {
        match self {
            AssetSource::Coingecko { id, quote } => format!("cg:{id}:{quote}"),
            AssetSource::Dolarapi { casa, side } => format!("da:{casa}:{side:?}"),
            AssetSource::Stooq { symbol } => format!("st:{symbol}"),
            AssetSource::Frankfurter { base, quote } => format!("fx:{base}:{quote}"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub label: String,
    #[serde(flatten)]
    pub source: AssetSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub label: String,
    pub base: String,
    pub quote: String,
    pub native_quote: String,
    pub price: Option<f64>,
    pub change_pct: Option<f64>,
    pub change_abs: Option<f64>,
    pub direction: Option<Direction>,
    pub source: ProviderKind,
    pub as_of: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
    pub state: QuoteState,
}

impl Quote {
    /// A non-usable quote for a missing/errored symbol (`price = None`).
    pub fn unavailable(asset: &Asset, state: QuoteState, now: DateTime<Utc>) -> Self {
        Quote {
            label: asset.label.clone(),
            base: String::new(),
            quote: String::new(),
            native_quote: String::new(),
            price: None,
            change_pct: None,
            change_abs: None,
            direction: None,
            source: asset.source.kind(),
            as_of: None,
            fetched_at: now,
            state,
        }
    }
}

/// Distinguishes rate-limiting (persist a backoff window) from generic failure.
#[derive(Debug)]
pub enum FetchError {
    RateLimited { retry_after: Option<u64> },
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_kind_is_derived_from_the_asset_source() {
        let a = AssetSource::Stooq {
            symbol: "aapl.us".into(),
        };
        assert_eq!(a.kind(), ProviderKind::Stooq);
    }

    #[test]
    fn a_positive_change_is_up_and_a_negative_change_is_down() {
        assert_eq!(Direction::from_change(1.2), Direction::Up);
        assert_eq!(Direction::from_change(-0.1), Direction::Down);
    }

    #[test]
    fn a_zero_change_is_flat() {
        assert_eq!(Direction::from_change(0.0), Direction::Flat);
    }
}
