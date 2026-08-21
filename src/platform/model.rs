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
    Frankfurter,
    Finnhub,
    Cnbc,
    Data912,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::CoinGecko => "coingecko",
            ProviderKind::DolarApi => "dolarapi",
            ProviderKind::Frankfurter => "frankfurter",
            ProviderKind::Finnhub => "finnhub",
            ProviderKind::Cnbc => "cnbc",
            ProviderKind::Data912 => "data912",
        }
    }
}

/// data912 Argentine market panels, each backed by one endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Panel {
    Acciones,
    Bonos,
    Cedears,
    Corp,
}

impl Panel {
    pub fn endpoint(self) -> &'static str {
        match self {
            Panel::Acciones => "arg_stocks",
            Panel::Bonos => "arg_bonds",
            Panel::Cedears => "arg_cedears",
            Panel::Corp => "arg_corp",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Panel::Acciones => "acciones",
            Panel::Bonos => "bonos",
            Panel::Cedears => "cedears",
            Panel::Corp => "corp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Side {
    Buy,
    #[default]
    Sell,
    Mid,
}

/// Provider-specific asset shape. A tagged enum keyed by the TOML `provider` field, so a
/// CoinGecko asset cannot carry a CNBC `symbol` — invalid states are unrepresentable.
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
    Frankfurter {
        base: String,
        quote: String,
    },
    Finnhub {
        symbol: String,
    },
    Cnbc {
        symbol: String,
    },
    /// Commodity via CNBC (friendly alias or raw symbol). e.g. gold -> @GC.1.
    Commodity {
        symbol: String,
    },
    /// Index via CNBC (friendly alias or raw symbol). e.g. vix -> .VIX, sp500 -> .SPX.
    Index {
        symbol: String,
    },
    /// Interest rate / Treasury yield via CNBC. e.g. us10y -> US10Y. Displayed as a percent.
    Rate {
        symbol: String,
    },
    Data912 {
        panel: Panel,
        symbol: String,
    },
}

impl AssetSource {
    pub fn kind(&self) -> ProviderKind {
        match self {
            AssetSource::Coingecko { .. } => ProviderKind::CoinGecko,
            AssetSource::Dolarapi { .. } => ProviderKind::DolarApi,
            AssetSource::Frankfurter { .. } => ProviderKind::Frankfurter,
            AssetSource::Finnhub { .. } => ProviderKind::Finnhub,
            AssetSource::Cnbc { .. } => ProviderKind::Cnbc,
            // Commodities/indices/rates are served by the CNBC slice (same endpoint).
            AssetSource::Commodity { .. } => ProviderKind::Cnbc,
            AssetSource::Index { .. } => ProviderKind::Cnbc,
            AssetSource::Rate { .. } => ProviderKind::Cnbc,
            AssetSource::Data912 { .. } => ProviderKind::Data912,
        }
    }

    /// Quote currency that is known STATICALLY from the asset definition, before any fetch:
    /// data912 and dolarapi serve ARS by definition; coingecko/frankfurter quote in the
    /// configured currency. `None` where only the feed can say (CNBC reports per-row
    /// currency; finnhub depends on the exchange suffix) — genuinely unknown until
    /// fetched. Used to keep placeholder quotes (`Quote::unavailable`) carrying a real
    /// currency instead of an empty one.
    pub fn quote_currency(&self) -> Option<String> {
        match self {
            AssetSource::Coingecko { quote, .. } => Some(quote.to_lowercase()),
            AssetSource::Frankfurter { quote, .. } => Some(quote.to_lowercase()),
            AssetSource::Dolarapi { .. } | AssetSource::Data912 { .. } => Some("ars".to_string()),
            AssetSource::Finnhub { .. }
            | AssetSource::Cnbc { .. }
            | AssetSource::Commodity { .. }
            | AssetSource::Index { .. }
            | AssetSource::Rate { .. } => None,
        }
    }

    /// Stable per-asset descriptor used (in input order) to build the cache key.
    pub fn cache_descriptor(&self) -> String {
        match self {
            AssetSource::Coingecko { id, quote } => format!("cg:{id}:{quote}"),
            AssetSource::Dolarapi { casa, side } => format!("da:{casa}:{side:?}"),
            AssetSource::Frankfurter { base, quote } => format!("fx:{base}:{quote}"),
            AssetSource::Finnhub { symbol } => format!("fh:{symbol}"),
            AssetSource::Cnbc { symbol } => format!("cb:{symbol}"),
            // Raw symbol as typed (alias resolution stays in cnbc.rs); class-prefixed so the
            // key is collision-safe vs plain `cb:` stocks.
            AssetSource::Commodity { symbol } => format!("cb:com:{symbol}"),
            AssetSource::Index { symbol } => format!("cb:idx:{symbol}"),
            AssetSource::Rate { symbol } => format!("cb:rate:{symbol}"),
            AssetSource::Data912 { panel, symbol } => format!("d9:{}:{symbol}", panel.as_str()),
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
    /// Intraday range, when the provider supplies it (currently CNBC only). Same unit as `price`.
    pub day_high: Option<f64>,
    pub day_low: Option<f64>,
    pub source: ProviderKind,
    pub as_of: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
    pub state: QuoteState,
}

impl Quote {
    /// A non-usable quote for a missing/errored symbol (`price = None`). The quote
    /// currency is still filled in when the asset defines it statically (a closed
    /// BYMA row is priceless, not currencyless).
    pub fn unavailable(asset: &Asset, state: QuoteState, now: DateTime<Utc>) -> Self {
        Quote {
            label: asset.label.clone(),
            base: String::new(),
            quote: asset.source.quote_currency().unwrap_or_default(),
            native_quote: String::new(),
            price: None,
            change_pct: None,
            change_abs: None,
            direction: None,
            day_high: None,
            day_low: None,
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
        let a = AssetSource::Cnbc {
            symbol: "AAPL".into(),
        };
        assert_eq!(a.kind(), ProviderKind::Cnbc);
    }

    #[test]
    fn cnbc_backed_classes_share_the_cnbc_provider_with_distinct_cache_keys() {
        let com = AssetSource::Commodity {
            symbol: "gold".into(),
        };
        let idx = AssetSource::Index {
            symbol: "vix".into(),
        };
        let rate = AssetSource::Rate {
            symbol: "us10y".into(),
        };
        assert_eq!(com.kind(), ProviderKind::Cnbc);
        assert_eq!(idx.kind(), ProviderKind::Cnbc);
        assert_eq!(rate.kind(), ProviderKind::Cnbc);
        assert_eq!(com.cache_descriptor(), "cb:com:gold");
        assert_eq!(idx.cache_descriptor(), "cb:idx:vix");
        assert_eq!(rate.cache_descriptor(), "cb:rate:us10y");
    }

    #[test]
    fn an_unavailable_quote_still_carries_a_statically_known_currency() {
        let byma = Asset {
            label: "ALUA".into(),
            source: AssetSource::Data912 {
                panel: Panel::Acciones,
                symbol: "ALUA".into(),
            },
        };
        let q = Quote::unavailable(&byma, QuoteState::Missing, chrono::Utc::now());
        assert_eq!(q.quote, "ars");
        assert_eq!(q.price, None);

        let cg = Asset {
            label: "BTC/ARS".into(),
            source: AssetSource::Coingecko {
                id: "bitcoin".into(),
                quote: "ARS".into(),
            },
        };
        assert_eq!(
            Quote::unavailable(&cg, QuoteState::Missing, chrono::Utc::now()).quote,
            "ars"
        );

        // Feed-decided currencies stay genuinely unknown until fetched.
        let cnbc = Asset {
            label: "TSLA".into(),
            source: AssetSource::Cnbc {
                symbol: "TSLA".into(),
            },
        };
        assert_eq!(
            Quote::unavailable(&cnbc, QuoteState::Missing, chrono::Utc::now()).quote,
            ""
        );
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
